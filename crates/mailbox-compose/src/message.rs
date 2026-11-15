//! RFC 5322 / MIME rendering of one outgoing message.
use crate::address::{Mailbox, validate_address};
use crate::encode::{base64_lines, fold, percent_encode, phrase, quoted_printable, unstructured};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// File name; any directory part is dropped.
    pub filename: String,
    /// `type/subtype`. `None` (or a malformed value) picks the type from the file extension.
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub from: Mailbox,
    pub to: Vec<Mailbox>,
    pub cc: Vec<Mailbox>,
    /// Envelope only. Never rendered into the message.
    pub bcc: Vec<Mailbox>,
    pub subject: String,
    pub text: String,
    /// When present the body is `multipart/alternative` (text first, HTML second).
    pub html: Option<String>,
    /// `<id@domain>`, generated once per draft and kept across retries.
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    /// Seconds since the Unix epoch (UTC).
    pub date_epoch: u64,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Total size of all attachments before encoding.
    pub max_attachment_bytes: u64,
    /// Size of the rendered message.
    pub max_message_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_attachment_bytes: 25 * 1024 * 1024,
            max_message_bytes: 40 * 1024 * 1024,
        }
    }
}

/// How text parts may be transferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    /// Only ASCII on the wire: non-ASCII text becomes quoted-printable.
    SevenBit,
    /// Non-ASCII text may be sent as `8bit` (needs the SMTP `8BITMIME` extension).
    EightBit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildError {
    NoRecipients,
    InvalidAddress,
    /// A control character (CR, LF, NUL, ...) in the named field.
    HeaderInjection(&'static str),
    InvalidMessageId,
    InvalidFilename,
    AttachmentsTooLarge {
        limit: u64,
        actual: u64,
    },
    MessageTooLarge {
        limit: u64,
        actual: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    /// The message with CRLF line endings, before SMTP dot-stuffing.
    pub bytes: Vec<u8>,
    pub envelope_from: String,
    /// To + Cc + Bcc addresses, in that order, without duplicates.
    pub recipients: Vec<String>,
    /// An address is not ASCII: the server must offer SMTPUTF8.
    pub needs_smtputf8: bool,
    /// The message contains bytes above 127: the server must offer 8BITMIME.
    pub has_8bit: bool,
}

/// `<unique@domain-of-sender>`. `unique` must already be unique (time + randomness) and may only
/// contain letters, digits, `.`, `-` and `_`. Returns `None` for unusable input.
pub fn new_message_id(unique: &str, from_address: &str) -> Option<String> {
    let safe = !unique.is_empty()
        && unique.len() <= 96
        && unique
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if !safe || validate_address(from_address.trim()).is_err() {
        return None;
    }
    let domain = from_address.trim().rsplit('@').next()?.to_ascii_lowercase();
    let domain = if domain.is_ascii() {
        domain.as_str()
    } else {
        "mailbox.invalid"
    };
    Some(format!("<{unique}@{domain}>"))
}

/// A message id as it appears in headers: `<left@right>`, ASCII, no spaces.
pub fn is_valid_message_id(id: &str) -> bool {
    let inner = id.strip_prefix('<').and_then(|s| s.strip_suffix('>'));
    inner.is_some_and(|inner| {
        id.len() <= 250
            && inner
                .split_once('@')
                .is_some_and(|(l, r)| !l.is_empty() && !r.is_empty())
            && inner
                .bytes()
                .all(|b| b.is_ascii_graphic() && !matches!(b, b'<' | b'>' | b'"' | b'\\'))
    })
}

/// Media type by file extension; unknown extensions are `application/octet-stream`.
pub fn content_type_for(filename: &str) -> &'static str {
    let ext = match filename.rsplit_once('.') {
        Some((_, e)) => e.to_ascii_lowercase(),
        None => return "application/octet-stream",
    };
    match ext.as_str() {
        "pdf" => "application/pdf",
        "txt" | "log" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "md" => "text/markdown",
        "json" => "application/json",
        "xml" => "application/xml",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "7z" => "application/x-7z-compressed",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "heic" => "image/heic",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "ics" => "text/calendar",
        "eml" => "message/rfc822",
        _ => "application/octet-stream",
    }
}

/// Base name of an attachment: no directories, no control characters.
pub fn safe_filename(filename: &str) -> Result<String, BuildError> {
    if filename.chars().any(char::is_control) {
        return Err(BuildError::HeaderInjection("filename"));
    }
    let base = filename.rsplit(['/', '\\']).next().unwrap_or("").trim();
    if base.is_empty() || base == "." || base == ".." || base.len() > 255 {
        return Err(BuildError::InvalidFilename);
    }
    Ok(base.to_string())
}

fn has_control(s: &str) -> bool {
    s.chars().any(|c| c.is_control() && c != '\t')
}

fn rfc5322_date(epoch: u64) -> String {
    let (days, secs) = (epoch / 86_400, epoch % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
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

fn check_mailbox(m: &Mailbox) -> Result<(), BuildError> {
    validate_address(&m.address).map_err(|_| BuildError::InvalidAddress)?;
    if m.name.chars().any(char::is_control) {
        return Err(BuildError::HeaderInjection("name"));
    }
    Ok(())
}

fn address_tokens(list: &[Mailbox]) -> Vec<String> {
    let mut tokens = Vec::new();
    for (i, m) in list.iter().enumerate() {
        let mut one = phrase(&m.name);
        one.push(if m.name.is_empty() {
            m.address.clone()
        } else {
            format!("<{}>", m.address)
        });
        if i + 1 < list.len()
            && let Some(last) = one.last_mut()
        {
            last.push(',');
        }
        tokens.extend(one);
    }
    tokens
}

/// CRLF line endings, ending with exactly one line break when there is any text.
fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str("\r\n");
            }
            '\n' => out.push_str("\r\n"),
            _ => out.push(c),
        }
    }
    if !out.ends_with("\r\n") {
        out.push_str("\r\n");
    }
    out
}

const BOUNDARY_MARK: &str = "=_mb_";

/// Headers (without the blank line) and body of one text part.
fn text_part(subtype: &str, text: &str, transfer: Transfer) -> (String, String) {
    let body = normalize(text);
    let clean = body.split("\r\n").all(|l| {
        l.len() <= 998
            && !l.ends_with([' ', '\t'])
            && !l.bytes().any(|b| (b < 0x20 && b != b'\t') || b == 0x7f)
    }) && !body.contains(BOUNDARY_MARK);
    let (encoding, body) = if clean && body.is_ascii() {
        ("7bit", body)
    } else if clean && transfer == Transfer::EightBit {
        ("8bit", body)
    } else {
        ("quoted-printable", quoted_printable(&body))
    };
    (
        format!(
            "Content-Type: text/{subtype}; charset=utf-8\r\nContent-Transfer-Encoding: {encoding}\r\n"
        ),
        body,
    )
}

fn disposition(filename: &str) -> String {
    let fallback: String = filename
        .chars()
        .take(60)
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if filename.is_ascii() && fallback == filename {
        return format!("Content-Disposition: attachment; filename=\"{fallback}\"\r\n");
    }
    // RFC 2231: UTF-8, percent-encoded, in numbered sections short enough for one line each.
    let mut sections = vec![String::from("UTF-8''")];
    for unit in percent_encode(filename) {
        if sections.last().is_some_and(|s| s.len() + unit.len() > 58) {
            sections.push(String::new());
        }
        if let Some(last) = sections.last_mut() {
            last.push_str(&unit);
        }
    }
    let mut out = format!("Content-Disposition: attachment;\r\n filename=\"{fallback}\"");
    if sections.len() == 1 {
        out.push_str(&format!(";\r\n filename*={}", sections[0]));
    } else {
        for (i, s) in sections.iter().enumerate() {
            out.push_str(&format!(";\r\n filename*{i}*={s}"));
        }
    }
    out.push_str("\r\n");
    out
}

fn fnv1a(input: &str) -> u64 {
    input.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

impl Message {
    /// Render the message. Deterministic: same input, same bytes.
    pub fn build(&self, transfer: Transfer, limits: &Limits) -> Result<Built, BuildError> {
        check_mailbox(&self.from)?;
        for m in self.to.iter().chain(&self.cc).chain(&self.bcc) {
            check_mailbox(m)?;
        }
        let mut recipients: Vec<String> = Vec::new();
        for m in self.to.iter().chain(&self.cc).chain(&self.bcc) {
            if !recipients
                .iter()
                .any(|r| r.eq_ignore_ascii_case(&m.address))
            {
                recipients.push(m.address.clone());
            }
        }
        if recipients.is_empty() {
            return Err(BuildError::NoRecipients);
        }
        if has_control(&self.subject) {
            return Err(BuildError::HeaderInjection("subject"));
        }
        let ids = std::iter::once(&self.message_id)
            .chain(&self.in_reply_to)
            .chain(&self.references);
        if !ids.clone().all(|id| is_valid_message_id(id)) {
            return Err(BuildError::InvalidMessageId);
        }
        let mut attachments = Vec::new();
        let mut total = 0u64;
        for a in &self.attachments {
            let name = safe_filename(&a.filename)?;
            let media = match a.content_type.as_deref() {
                Some(t) if t.chars().any(char::is_control) => {
                    return Err(BuildError::HeaderInjection("content-type"));
                }
                Some(t)
                    if t.split_once('/').is_some_and(|(a, b)| {
                        let token = |s: &str| {
                            !s.is_empty()
                                && s.bytes()
                                    .all(|c| c.is_ascii_alphanumeric() || b"!#$&-^_.+".contains(&c))
                        };
                        token(a) && token(b)
                    }) =>
                {
                    t.to_ascii_lowercase()
                }
                _ => content_type_for(&name).to_string(),
            };
            total += a.data.len() as u64;
            attachments.push((name, media, &a.data));
        }
        if total > limits.max_attachment_bytes {
            return Err(BuildError::AttachmentsTooLarge {
                limit: limits.max_attachment_bytes,
                actual: total,
            });
        }

        let mut out = String::new();
        out.push_str(&format!("Date: {}\r\n", rfc5322_date(self.date_epoch)));
        out.push_str(&fold(
            "From",
            &address_tokens(std::slice::from_ref(&self.from)),
        ));
        if !self.to.is_empty() {
            out.push_str(&fold("To", &address_tokens(&self.to)));
        }
        if !self.cc.is_empty() {
            out.push_str(&fold("Cc", &address_tokens(&self.cc)));
        }
        out.push_str(&fold("Subject", &unstructured(&self.subject)));
        out.push_str(&fold("Message-ID", std::slice::from_ref(&self.message_id)));
        if let Some(id) = &self.in_reply_to {
            out.push_str(&fold("In-Reply-To", std::slice::from_ref(id)));
        }
        if !self.references.is_empty() {
            out.push_str(&fold("References", &self.references));
        }
        out.push_str("MIME-Version: 1.0\r\n");

        let seed = fnv1a(&self.message_id);
        let boundary = |n: u8| format!("{BOUNDARY_MARK}{seed:016x}_{n}");
        // The readable body: plain text, or text + HTML alternatives.
        let (body_headers, body) = match &self.html {
            None => text_part("plain", &self.text, transfer),
            Some(html) => {
                let b = boundary(2);
                let mut multi = String::new();
                for (subtype, text) in [("plain", &self.text), ("html", html)] {
                    let (h, t) = text_part(subtype, text, transfer);
                    multi.push_str(&format!("--{b}\r\n{h}\r\n{t}"));
                }
                multi.push_str(&format!("--{b}--\r\n"));
                (
                    format!("Content-Type: multipart/alternative; boundary=\"{b}\"\r\n"),
                    multi,
                )
            }
        };
        if attachments.is_empty() {
            out.push_str(&body_headers);
            out.push_str("\r\n");
            out.push_str(&body);
        } else {
            let b = boundary(1);
            out.push_str(&format!(
                "Content-Type: multipart/mixed; boundary=\"{b}\"\r\n\r\n"
            ));
            out.push_str(&format!("--{b}\r\n{body_headers}\r\n{body}"));
            for (name, media, data) in attachments {
                out.push_str(&format!(
                    "--{b}\r\nContent-Type: {media}\r\nContent-Transfer-Encoding: base64\r\n{}\r\n{}",
                    disposition(&name),
                    base64_lines(data)
                ));
            }
            out.push_str(&format!("--{b}--\r\n"));
        }

        let bytes = out.into_bytes();
        if bytes.len() as u64 > limits.max_message_bytes {
            return Err(BuildError::MessageTooLarge {
                limit: limits.max_message_bytes,
                actual: bytes.len() as u64,
            });
        }
        Ok(Built {
            has_8bit: !bytes.is_ascii(),
            needs_smtputf8: !self.from.is_ascii() || recipients.iter().any(|r| !r.is_ascii()),
            envelope_from: self.from.address.clone(),
            recipients,
            bytes,
        })
    }
}
