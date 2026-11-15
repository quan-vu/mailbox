//! SMTP submission check (MB-112) and a minimal plain-text sender.
//!
//! Security properties, enforced here and covered by tests:
//! - Encryption is mandatory: implicit TLS (465) or STARTTLS (587) with full certificate
//!   validation. If a server does not offer STARTTLS the check stops; it never continues in
//!   plaintext. Capabilities seen before TLS are discarded and bytes buffered across the
//!   STARTTLS boundary are rejected (command-injection defence).
//! - Credentials are only sent after TLS is verified, never logged, and errors contain no
//!   server text, user name or secret.
//! - A check never sends mail. Sending is a separate, explicit call. If the connection is lost
//!   after the message was handed over, the outcome is `Unknown` and nothing is retried.
pub mod submit;

use mailbox_core::secrets::Secret;
use mailbox_core::{MailServerEndpoint, TlsMode};
use mailbox_net::{NetError, step};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

const MAX_LINE: usize = 4 * 1024;
const MAX_REPLY_LINES: usize = 64;
/// Name sent in EHLO. An address literal is always syntactically valid and reveals nothing.
const EHLO_NAME: &str = "[127.0.0.1]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtpError {
    InvalidEndpoint,
    Unreachable,
    Timeout,
    CertificateInvalid,
    TlsFailed,
    /// The peer is not an SMTP server or broke the protocol.
    NotSmtp,
    /// Port 587 without STARTTLS support. The check stops instead of continuing unencrypted.
    StartTlsUnavailable,
    /// The server offers no password mechanism we support (PLAIN, LOGIN).
    AuthUnsupported,
    AuthFailed,
    /// Sender or recipient address is not acceptable (checked locally before any I/O).
    InvalidAddress,
}

impl From<NetError> for SmtpError {
    fn from(e: NetError) -> Self {
        match e {
            NetError::InvalidEndpoint => SmtpError::InvalidEndpoint,
            NetError::Unreachable => SmtpError::Unreachable,
            NetError::Timeout => SmtpError::Timeout,
            NetError::CertificateInvalid => SmtpError::CertificateInvalid,
            NetError::TlsFailed => SmtpError::TlsFailed,
        }
    }
}

/// What the server advertised after TLS. Contains no account data.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SmtpInfo {
    pub capabilities: Vec<String>,
}

impl SmtpInfo {
    pub fn has(&self, keyword: &str) -> bool {
        self.capabilities.iter().any(|c| {
            c.split(' ')
                .next()
                .is_some_and(|k| k.eq_ignore_ascii_case(keyword))
        })
    }

    /// Password mechanisms offered (`AUTH PLAIN LOGIN ...`).
    pub fn auth_mechanisms(&self) -> Vec<String> {
        self.capabilities
            .iter()
            .filter(|c| c.len() > 5 && c[..5].eq_ignore_ascii_case("AUTH "))
            .flat_map(|c| c[5..].split_ascii_whitespace())
            .map(|m| m.to_ascii_uppercase())
            .collect()
    }

    /// Maximum message size in bytes, if the server declares one.
    pub fn max_size(&self) -> Option<u64> {
        self.capabilities
            .iter()
            .find(|c| c.len() >= 4 && c[..4].eq_ignore_ascii_case("SIZE"))
            .and_then(|c| c[4..].trim().parse().ok())
    }
}

/// Result of handing a message to the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendOutcome {
    /// The server accepted responsibility for the message (250 after DATA).
    Accepted,
    /// The server refused it; nothing was sent. `permanent` distinguishes 5xx from 4xx.
    Rejected { permanent: bool },
    /// The message was transmitted but no final reply arrived. It may or may not have been
    /// sent: check the Sent folder / recipient before sending again. Never auto-retried.
    Unknown,
}

/// Sender identity policy (ACC-06): the envelope/From address should be the login identity
/// unless the provider is known to allow aliases. Used to warn before sending.
pub fn identity_matches_login(login: &str, from_address: &str) -> bool {
    login.trim().eq_ignore_ascii_case(from_address.trim())
}

fn valid_address(addr: &str) -> bool {
    let a = addr.trim();
    let Some((local, domain)) = a.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && a.len() <= 254
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && a.bytes().all(|b| {
            b.is_ascii_graphic() && !matches!(b, b'<' | b'>' | b',' | b';' | b'"' | b'(' | b')')
        })
}

fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        out.push(T[(b[0] >> 2) as usize] as char);
        out.push(T[(((b[0] & 3) << 4) | (b[1] >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[(((b[1] & 15) << 2) | (b[2] >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(b[2] & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn wipe(mut s: String) {
    // SAFETY: zero bytes are valid UTF-8.
    for byte in unsafe { s.as_bytes_mut() } {
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
}

struct Reply {
    code: u16,
    lines: Vec<String>,
}

struct Dialogue<S> {
    stream: BufReader<S>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Dialogue<S> {
    fn new(stream: S) -> Self {
        Self {
            stream: BufReader::new(stream),
        }
    }

    async fn line(&mut self) -> Result<String, SmtpError> {
        step(async {
            let mut buf = Vec::new();
            let mut limited = (&mut self.stream).take(MAX_LINE as u64);
            let n = limited
                .read_until(b'\n', &mut buf)
                .await
                .map_err(|_| SmtpError::NotSmtp)?;
            if n == 0 || !buf.ends_with(b"\n") {
                return Err(SmtpError::NotSmtp);
            }
            Ok(String::from_utf8_lossy(&buf).trim_end().to_string())
        })
        .await
    }

    async fn reply(&mut self) -> Result<Reply, SmtpError> {
        let mut lines = Vec::new();
        for _ in 0..MAX_REPLY_LINES {
            let line = self.line().await?;
            let bytes = line.as_bytes();
            if bytes.len() < 3 || !bytes[..3].iter().all(u8::is_ascii_digit) {
                return Err(SmtpError::NotSmtp);
            }
            let code: u16 = line[..3].parse().map_err(|_| SmtpError::NotSmtp)?;
            let last = bytes.get(3) != Some(&b'-');
            lines.push(line.get(4..).unwrap_or("").to_string());
            if last {
                return Ok(Reply { code, lines });
            }
        }
        Err(SmtpError::NotSmtp)
    }

    async fn send(&mut self, bytes: &[u8]) -> Result<(), SmtpError> {
        step(async {
            let w = self.stream.get_mut();
            w.write_all(bytes).await.map_err(|_| SmtpError::NotSmtp)?;
            w.flush().await.map_err(|_| SmtpError::NotSmtp)
        })
        .await
    }

    async fn command(&mut self, line: &str) -> Result<Reply, SmtpError> {
        self.send(format!("{line}\r\n").as_bytes()).await?;
        self.reply().await
    }

    async fn greeting(&mut self) -> Result<(), SmtpError> {
        if self.reply().await?.code == 220 {
            Ok(())
        } else {
            Err(SmtpError::NotSmtp)
        }
    }

    async fn ehlo(&mut self) -> Result<SmtpInfo, SmtpError> {
        let reply = self.command(&format!("EHLO {EHLO_NAME}")).await?;
        if reply.code != 250 {
            return Err(SmtpError::NotSmtp);
        }
        let capabilities = reply
            .lines
            .into_iter()
            .skip(1)
            .filter(|c| {
                !c.is_empty()
                    && c.len() <= 256
                    && c.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
            })
            .collect();
        Ok(SmtpInfo { capabilities })
    }

    async fn auth(
        &mut self,
        info: &SmtpInfo,
        username: &str,
        password: &Secret,
    ) -> Result<(), SmtpError> {
        let mechanisms = info.auth_mechanisms();
        let reply = if mechanisms.iter().any(|m| m == "PLAIN") {
            let mut raw = Vec::with_capacity(username.len() + password.expose().len() + 2);
            raw.push(0);
            raw.extend_from_slice(username.as_bytes());
            raw.push(0);
            raw.extend_from_slice(password.expose().as_bytes());
            let token = base64(&raw);
            raw.fill(0);
            let line = format!("AUTH PLAIN {token}");
            wipe(token);
            let result = self.command(&line).await;
            wipe(line);
            result?
        } else if mechanisms.iter().any(|m| m == "LOGIN") {
            if self.command("AUTH LOGIN").await?.code != 334 {
                return Err(SmtpError::AuthFailed);
            }
            if self.command(&base64(username.as_bytes())).await?.code != 334 {
                return Err(SmtpError::AuthFailed);
            }
            let token = base64(password.expose().as_bytes());
            let result = self.command(&token).await;
            wipe(token);
            result?
        } else {
            return Err(SmtpError::AuthUnsupported);
        };
        if reply.code == 235 {
            Ok(())
        } else {
            Err(SmtpError::AuthFailed)
        }
    }

    async fn quit(&mut self) {
        let _ = self.command("QUIT").await;
    }
}

fn check_credentials(username: &str, password: &Secret) -> Result<(), SmtpError> {
    let bad = |s: &str| {
        s.is_empty() || s.len() > 512 || s.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0)
    };
    if bad(username) || bad(password.expose()) {
        Err(SmtpError::AuthFailed)
    } else {
        Ok(())
    }
}

/// Plain-text phase on port 587: greeting, EHLO, STARTTLS. Returns the raw stream ready for the
/// TLS handshake. Fails if STARTTLS is not offered or if the server sent anything beyond its
/// STARTTLS reply (which would otherwise be read as if it came from inside the TLS session).
pub async fn negotiate_starttls<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
) -> Result<S, SmtpError> {
    let mut d = Dialogue::new(stream);
    d.greeting().await?;
    let info = d.ehlo().await?;
    if !info.has("STARTTLS") {
        d.quit().await;
        return Err(SmtpError::StartTlsUnavailable);
    }
    if d.command("STARTTLS").await?.code != 220 {
        return Err(SmtpError::StartTlsUnavailable);
    }
    if !d.stream.buffer().is_empty() {
        return Err(SmtpError::NotSmtp);
    }
    Ok(d.stream.into_inner())
}

/// Over an already-secured stream: greeting (when `expect_greeting`), EHLO, optional AUTH, QUIT.
/// Never issues MAIL, RCPT or DATA.
pub async fn check_stream<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    expect_greeting: bool,
    credentials: Option<(&str, &Secret)>,
) -> Result<SmtpInfo, SmtpError> {
    if let Some((u, p)) = credentials {
        check_credentials(u, p)?;
    }
    let mut d = Dialogue::new(stream);
    if expect_greeting {
        d.greeting().await?;
    }
    let info = d.ehlo().await?;
    let result = match credentials {
        Some((u, p)) => d.auth(&info, u, p).await,
        None => Ok(()),
    };
    d.quit().await;
    result.map(|()| info)
}

/// A minimal plain-text message. Headers are generated here; the body is always base64-encoded
/// so any text is transported safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextMessage {
    pub from: String,
    pub to: String,
    pub subject: String,
    pub body: String,
}

fn rfc5322_date(epoch: u64) -> String {
    let days = epoch / 86_400;
    let secs = epoch % 86_400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    const DOW: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MON: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} +0000",
        DOW[(days % 7) as usize],
        day,
        MON[(month - 1) as usize],
        year,
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

impl TextMessage {
    /// RFC 5322 text with CRLF line endings, ready for DATA (before dot-stuffing).
    pub fn render(&self, now_epoch: u64, unique: u64) -> Result<String, SmtpError> {
        if !valid_address(&self.from) || !valid_address(&self.to) {
            return Err(SmtpError::InvalidAddress);
        }
        if self.subject.bytes().any(|b| b == b'\r' || b == b'\n') {
            return Err(SmtpError::InvalidAddress);
        }
        let domain = self.from.trim().rsplit('@').next().unwrap_or("localhost");
        let subject = if self.subject.is_ascii() {
            self.subject.clone()
        } else {
            format!("=?UTF-8?B?{}?=", base64(self.subject.as_bytes()))
        };
        let encoded = base64(self.body.as_bytes());
        let mut body = String::new();
        for chunk in encoded.as_bytes().chunks(76) {
            body.push_str(std::str::from_utf8(chunk).unwrap_or(""));
            body.push_str("\r\n");
        }
        Ok(format!(
            "From: <{from}>\r\nTo: <{to}>\r\nSubject: {subject}\r\nDate: {date}\r\nMessage-ID: <{now_epoch}.{unique}@{domain}>\r\n\
             MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{body}",
            from = self.from.trim(),
            to = self.to.trim(),
            date = rfc5322_date(now_epoch),
        ))
    }
}

/// Over an already-secured, greeted-or-not stream: EHLO, AUTH, MAIL, RCPT, DATA, QUIT.
pub async fn send_stream<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    expect_greeting: bool,
    username: &str,
    password: &Secret,
    message: &TextMessage,
) -> Result<SendOutcome, SmtpError> {
    check_credentials(username, password)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_secs(), u64::from(d.subsec_nanos())))
        .unwrap_or((0, 0));
    let text = message.render(now.0, now.1)?;
    let mut d = Dialogue::new(stream);
    if expect_greeting {
        d.greeting().await?;
    }
    let info = d.ehlo().await?;
    d.auth(&info, username, password).await?;

    let rejected = |code: u16| SendOutcome::Rejected {
        permanent: code >= 500,
    };
    let reply = d
        .command(&format!("MAIL FROM:<{}>", message.from.trim()))
        .await?;
    if reply.code != 250 {
        d.quit().await;
        return Ok(rejected(reply.code));
    }
    let reply = d
        .command(&format!("RCPT TO:<{}>", message.to.trim()))
        .await?;
    if reply.code != 250 && reply.code != 251 {
        d.quit().await;
        return Ok(rejected(reply.code));
    }
    let reply = d.command("DATA").await?;
    if reply.code != 354 {
        d.quit().await;
        return Ok(rejected(reply.code));
    }
    // Dot-stuffing: a line starting with "." gets an extra "." so it cannot end the message.
    let mut payload = String::with_capacity(text.len() + 8);
    for line in text.split_inclusive("\r\n") {
        if line.starts_with('.') {
            payload.push('.');
        }
        payload.push_str(line);
    }
    payload.push_str(".\r\n");
    // From here on the message may have been delivered even if we see an error.
    if d.send(payload.as_bytes()).await.is_err() {
        return Ok(SendOutcome::Unknown);
    }
    let outcome = match d.reply().await {
        Ok(reply) if reply.code == 250 => SendOutcome::Accepted,
        Ok(reply) if (400..600).contains(&reply.code) => rejected(reply.code),
        _ => SendOutcome::Unknown,
    };
    d.quit().await;
    Ok(outcome)
}

enum Secured {
    Implicit(mailbox_net::Tls),
    Upgraded(mailbox_net::Tls),
}

async fn secure(endpoint: &MailServerEndpoint) -> Result<Secured, SmtpError> {
    match endpoint.tls_mode {
        TlsMode::ImplicitTls => Ok(Secured::Implicit(mailbox_net::connect_tls(endpoint).await?)),
        TlsMode::StartTls => {
            let tcp = negotiate_starttls(mailbox_net::connect_tcp(endpoint).await?).await?;
            Ok(Secured::Upgraded(
                mailbox_net::upgrade_tls(tcp, endpoint).await?,
            ))
        }
    }
}

/// [`check`] over addresses that were already resolved and checked by the caller (SSRF guard).
/// The host name is not resolved again; the certificate is validated for that name.
pub async fn check_pinned(
    endpoint: &MailServerEndpoint,
    addrs: &[std::net::SocketAddr],
    credentials: Option<(&str, &Secret)>,
) -> Result<SmtpInfo, SmtpError> {
    let tcp = mailbox_net::connect_tcp_pinned(endpoint, addrs).await?;
    match endpoint.tls_mode {
        TlsMode::ImplicitTls => {
            let tls = mailbox_net::upgrade_tls(tcp, endpoint).await?;
            check_stream(tls, true, credentials).await
        }
        TlsMode::StartTls => {
            let tcp = negotiate_starttls(tcp).await?;
            let tls = mailbox_net::upgrade_tls(tcp, endpoint).await?;
            check_stream(tls, false, credentials).await
        }
    }
}

/// Connect with verified TLS (implicit or STARTTLS), read capabilities and, when credentials are
/// given, check that they are accepted. Never sends mail.
pub async fn check(
    endpoint: &MailServerEndpoint,
    credentials: Option<(&str, &Secret)>,
) -> Result<SmtpInfo, SmtpError> {
    match secure(endpoint).await? {
        Secured::Implicit(tls) => check_stream(tls, true, credentials).await,
        Secured::Upgraded(tls) => check_stream(tls, false, credentials).await,
    }
}

/// Send one plain-text message. Explicit call only; see [`SendOutcome`] for `Unknown`.
pub async fn send(
    endpoint: &MailServerEndpoint,
    username: &str,
    password: &Secret,
    message: &TextMessage,
) -> Result<SendOutcome, SmtpError> {
    match secure(endpoint).await? {
        Secured::Implicit(tls) => send_stream(tls, true, username, password, message).await,
        Secured::Upgraded(tls) => send_stream(tls, false, username, password, message).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (i, o) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(i.as_bytes()), o);
        }
    }

    #[test]
    fn dates_are_rfc5322() {
        assert_eq!(rfc5322_date(0), "Thu, 01 Jan 1970 00:00:00 +0000");
        assert_eq!(
            rfc5322_date(1_794_614_400),
            "Sat, 14 Nov 2026 00:00:00 +0000"
        );
        assert_eq!(
            rfc5322_date(951_782_400 + 86_399),
            "Tue, 29 Feb 2000 23:59:59 +0000"
        );
    }

    #[test]
    fn capabilities_expose_auth_size_and_keywords() {
        let info = SmtpInfo {
            capabilities: vec![
                "SIZE 52428800".into(),
                "AUTH PLAIN LOGIN".into(),
                "8BITMIME".into(),
            ],
        };
        assert!(info.has("8bitmime") && info.has("AUTH") && !info.has("STARTTLS"));
        assert_eq!(info.auth_mechanisms(), vec!["PLAIN", "LOGIN"]);
        assert_eq!(info.max_size(), Some(52_428_800));
    }

    #[test]
    fn sender_identity_must_match_login() {
        assert!(identity_matches_login(
            "No-Reply@Example.com",
            " no-reply@example.com "
        ));
        assert!(!identity_matches_login(
            "no-reply@example.com",
            "ceo@example.com"
        ));
    }

    #[test]
    fn message_rendering_rejects_header_injection_and_encodes_text() {
        let mut m = TextMessage {
            from: "a@example.com".into(),
            to: "b@example.org".into(),
            subject: "Thử nghiệm".into(),
            body: "Xin chào\n.\nhết".into(),
        };
        let text = m.render(0, 7).unwrap();
        assert!(
            text.contains("Subject: =?UTF-8?B?") && text.contains("Message-ID: <0.7@example.com>")
        );
        assert!(text.contains("Content-Transfer-Encoding: base64\r\n\r\n"));
        assert!(!text.contains("Xin chào"), "body is encoded");
        m.subject = "x\r\nBcc: victim@example.com".into();
        assert_eq!(m.render(0, 0), Err(SmtpError::InvalidAddress));
        m.subject = "ok".into();
        m.to = "b@example.org>\r\nRCPT TO:<c@example.org".into();
        assert_eq!(m.render(0, 0), Err(SmtpError::InvalidAddress));
    }
}
