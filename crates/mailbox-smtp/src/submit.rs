//! Full-envelope submission (MB-151): several recipients, per-recipient results, SIZE,
//! 8BITMIME and SMTPUTF8.
//!
//! Sending rules, enforced here and covered by `tests/submit.rs`:
//! - Everything that can be checked locally is checked before any byte is written: envelope
//!   addresses (no control characters, spaces or angle brackets, so they cannot add commands),
//!   bare CR/LF in the message, the advertised SIZE limit, and the extensions the message needs.
//! - Each recipient gets its own result. By default one refused recipient stops the whole send
//!   before DATA, so the user can fix the address and nothing was sent to anybody.
//! - Up to DATA a failure is definite ("not sent"). Once the message content has been written,
//!   a missing final reply is [`SendOutcome::Unknown`]. Nothing in this module ever retries.
//! - Reports carry reply codes only, never server text (which may quote addresses).
use crate::{Dialogue, SendOutcome, SmtpError, SmtpInfo, check_credentials};
use mailbox_core::MailServerEndpoint;
use mailbox_core::secrets::Secret;
use tokio::io::{AsyncRead, AsyncWrite};

const MAX_RECIPIENTS: usize = 100;
const CHUNK: usize = 64 * 1024;

/// One message and its envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    /// Envelope sender (bare address).
    pub from: String,
    /// Envelope recipients (bare addresses): To, Cc and Bcc together.
    pub recipients: Vec<String>,
    /// The message with CRLF line endings, not dot-stuffed. May contain 8-bit bytes.
    pub data: Vec<u8>,
    /// ASCII-only rendering of the same message, used when `data` has 8-bit bytes and the
    /// server does not offer 8BITMIME.
    pub fallback_7bit: Option<Vec<u8>>,
    /// An envelope address is not ASCII: the server must offer SMTPUTF8.
    pub smtputf8: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialPolicy {
    /// One refused recipient stops the send before DATA. Nothing is sent.
    AbortIfAnyRejected,
    /// Send to the recipients the server accepted; the refused ones are in the report.
    SendToAccepted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RcptStatus {
    Accepted,
    Rejected { code: u16 },
}

impl RcptStatus {
    /// 5xx: do not retry unchanged. 4xx: may work later.
    pub fn permanent(self) -> bool {
        matches!(self, RcptStatus::Rejected { code } if code >= 500)
    }
}

/// Where a refusal or an unknown outcome happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Mail,
    Rcpt,
    Data,
    /// After the message content was written.
    Content,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendReport {
    pub outcome: SendOutcome,
    /// One entry per recipient that was offered to the server, in envelope order.
    pub recipients: Vec<(String, RcptStatus)>,
    /// Set unless the message was accepted.
    pub stage: Option<Stage>,
    /// Reply code behind a refusal, when there was one.
    pub code: Option<u16>,
    /// The 8-bit rendering was sent (`BODY=8BITMIME`).
    pub used_8bit: bool,
}

impl SendReport {
    pub fn rejected(&self) -> Vec<&str> {
        self.recipients
            .iter()
            .filter(|(_, s)| *s != RcptStatus::Accepted)
            .map(|(a, _)| a.as_str())
            .collect()
    }
}

/// Reasons a submission did not reach DATA. In every case nothing was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitError {
    /// Connection, TLS or authentication problem.
    Session(SmtpError),
    /// Sender or a recipient is not an acceptable envelope address, or there is no recipient.
    InvalidEnvelope,
    /// The message contains a bare CR or LF.
    InvalidMessage,
    /// Larger than the limit the server advertises.
    TooLarge { limit: u64, actual: u64 },
    /// An address is not ASCII and the server does not offer SMTPUTF8.
    SmtpUtf8Unavailable,
    /// 8-bit content, no 8BITMIME on the server and no ASCII rendering to fall back to.
    EightBitUnavailable,
}

impl From<SmtpError> for SubmitError {
    fn from(e: SmtpError) -> Self {
        SubmitError::Session(e)
    }
}

fn envelope_address_ok(address: &str, allow_utf8: bool) -> bool {
    !address.is_empty()
        && address.len() <= 254
        && address
            .split_once('@')
            .is_some_and(|(l, d)| !l.is_empty() && !d.is_empty())
        && (allow_utf8 || address.is_ascii())
        && address
            .chars()
            .all(|c| !c.is_control() && !c.is_whitespace() && !matches!(c, '<' | '>'))
}

/// Every LF is preceded by CR and every CR followed by LF.
fn crlf_only(data: &[u8]) -> bool {
    data.iter().enumerate().all(|(i, &b)| match b {
        b'\n' => i > 0 && data[i - 1] == b'\r',
        b'\r' => data.get(i + 1) == Some(&b'\n'),
        _ => true,
    })
}

/// Dot-stuff and terminate: a line starting with "." gets another "." so it cannot end the message.
fn stuffed(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 64 + 8);
    let mut line_start = true;
    for &b in data {
        if line_start && b == b'.' {
            out.push(b'.');
        }
        out.push(b);
        line_start = b == b'\n';
    }
    if !out.ends_with(b"\r\n") {
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b".\r\n");
    out
}

fn validate(s: &Submission) -> Result<(), SubmitError> {
    let ok = !s.recipients.is_empty()
        && s.recipients.len() <= MAX_RECIPIENTS
        && envelope_address_ok(&s.from, s.smtputf8)
        && s.recipients
            .iter()
            .all(|r| envelope_address_ok(r, s.smtputf8));
    if !ok {
        return Err(SubmitError::InvalidEnvelope);
    }
    let fallback_ok = s
        .fallback_7bit
        .as_ref()
        .is_none_or(|f| f.is_ascii() && crlf_only(f));
    if !crlf_only(&s.data) || !fallback_ok {
        return Err(SubmitError::InvalidMessage);
    }
    Ok(())
}

/// Pick the rendering the server can take. Returns (bytes, sent as 8-bit).
fn choose<'a>(s: &'a Submission, info: &SmtpInfo) -> Result<(&'a [u8], bool), SubmitError> {
    let needs_utf8 = s.smtputf8 || !s.from.is_ascii() || s.recipients.iter().any(|r| !r.is_ascii());
    if needs_utf8 && !info.has("SMTPUTF8") {
        return Err(SubmitError::SmtpUtf8Unavailable);
    }
    let (data, eight_bit) = if s.data.is_ascii() {
        (s.data.as_slice(), false)
    } else if info.has("8BITMIME") {
        (s.data.as_slice(), true)
    } else {
        match &s.fallback_7bit {
            // Internationalised headers cannot be down-converted.
            Some(f) if !needs_utf8 => (f.as_slice(), false),
            _ => return Err(SubmitError::EightBitUnavailable),
        }
    };
    if let Some(limit) = info.max_size().filter(|l| *l > 0)
        && data.len() as u64 > limit
    {
        return Err(SubmitError::TooLarge {
            limit,
            actual: data.len() as u64,
        });
    }
    Ok((data, eight_bit))
}

/// Over an already-secured stream: EHLO, AUTH, MAIL, RCPT (each recipient), DATA, QUIT.
pub async fn submit_stream<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    expect_greeting: bool,
    username: &str,
    password: &Secret,
    submission: &Submission,
    policy: PartialPolicy,
) -> Result<SendReport, SubmitError> {
    check_credentials(username, password)?;
    validate(submission)?;
    let mut d = Dialogue::new(stream);
    if expect_greeting {
        d.greeting().await?;
    }
    let info = d.ehlo().await?;
    d.auth(&info, username, password).await?;
    let (data, used_8bit) = match choose(submission, &info) {
        Ok(chosen) => chosen,
        Err(e) => {
            d.quit().await;
            return Err(e);
        }
    };

    let mut report = SendReport {
        outcome: SendOutcome::Unknown,
        recipients: Vec::new(),
        stage: None,
        code: None,
        used_8bit,
    };
    let refused = |mut report: SendReport, stage: Stage, code: u16| {
        report.outcome = SendOutcome::Rejected {
            permanent: code >= 500,
        };
        report.stage = Some(stage);
        report.code = Some(code);
        report
    };

    let mut mail = format!("MAIL FROM:<{}>", submission.from);
    if info.has("SIZE") {
        mail.push_str(&format!(" SIZE={}", data.len()));
    }
    if used_8bit {
        mail.push_str(" BODY=8BITMIME");
    }
    if submission.smtputf8 {
        mail.push_str(" SMTPUTF8");
    }
    let reply = d.command(&mail).await?;
    if reply.code != 250 {
        d.quit().await;
        return Ok(refused(report, Stage::Mail, reply.code));
    }

    for recipient in &submission.recipients {
        let reply = d.command(&format!("RCPT TO:<{recipient}>")).await?;
        let status = if matches!(reply.code, 250 | 251) {
            RcptStatus::Accepted
        } else {
            RcptStatus::Rejected { code: reply.code }
        };
        report.recipients.push((recipient.clone(), status));
    }
    let worst = report
        .recipients
        .iter()
        .filter_map(|(_, s)| match s {
            RcptStatus::Rejected { code } => Some(*code),
            RcptStatus::Accepted => None,
        })
        .max();
    let accepted = report
        .recipients
        .iter()
        .any(|(_, s)| *s == RcptStatus::Accepted);
    if let Some(code) = worst
        && (!accepted || policy == PartialPolicy::AbortIfAnyRejected)
    {
        let _ = d.command("RSET").await;
        d.quit().await;
        return Ok(refused(report, Stage::Rcpt, code));
    }

    let reply = d.command("DATA").await?;
    if reply.code != 354 {
        d.quit().await;
        return Ok(refused(report, Stage::Data, reply.code));
    }
    // From here on the message may have been delivered even if we see an error.
    report.stage = Some(Stage::Content);
    for chunk in stuffed(data).chunks(CHUNK) {
        if d.send(chunk).await.is_err() {
            return Ok(report);
        }
    }
    // Content checks can take a while: allow a few timeout periods for the final reply.
    let mut reply = d.reply().await;
    for _ in 0..3 {
        if !matches!(reply, Err(SmtpError::Timeout)) {
            break;
        }
        reply = d.reply().await;
    }
    match reply {
        Ok(r) if r.code == 250 => {
            report.outcome = SendOutcome::Accepted;
            report.stage = None;
        }
        Ok(r) if (400..600).contains(&r.code) => {
            report.outcome = SendOutcome::Rejected {
                permanent: r.code >= 500,
            };
            report.code = Some(r.code);
        }
        _ => return Ok(report),
    }
    d.quit().await;
    Ok(report)
}

/// Connect over verified TLS (implicit or STARTTLS) and submit one message. Explicit call only;
/// never retried here. See [`SendOutcome::Unknown`].
pub async fn submit(
    endpoint: &MailServerEndpoint,
    username: &str,
    password: &Secret,
    submission: &Submission,
    policy: PartialPolicy,
) -> Result<SendReport, SubmitError> {
    // Local checks first, so a bad envelope never opens a connection.
    check_credentials(username, password)?;
    validate(submission)?;
    match crate::secure(endpoint).await? {
        crate::Secured::Implicit(tls) => {
            submit_stream(tls, true, username, password, submission, policy).await
        }
        crate::Secured::Upgraded(tls) => {
            submit_stream(tls, false, username, password, submission, policy).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stuffing_doubles_leading_dots_and_terminates() {
        assert_eq!(stuffed(b".a\r\n.\r\nb"), b"..a\r\n..\r\nb\r\n.\r\n");
        assert_eq!(stuffed(b""), b"\r\n.\r\n");
    }

    #[test]
    fn bare_line_breaks_are_detected() {
        assert!(crlf_only(b"a\r\nb\r\n") && crlf_only(b""));
        assert!(!crlf_only(b"a\nb") && !crlf_only(b"a\rb") && !crlf_only(b"a\r"));
    }
}
