//! RFC 5322 / MIME parsing with limits (threat T09).
//!
//! `mail-parser` does the decoding (transfer encodings, charsets, RFC 2047/2231). It walks
//! multiparts with an explicit stack and caps nested `message/rfc822` parsing itself, so hostile
//! nesting cannot exhaust the call stack; this module then enforces our own limits on the
//! result and reduces it to a small model that holds nothing unsanitised.
use crate::filename::safe_filename;
use crate::sanitize::{InlineImages, sanitize};
use mail_parser::{Addr, Message, MessageParser, MimeHeaders, PartType};
use mailbox_core::body::{AttachmentMeta, BodyRecord};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_message_bytes: usize,
    pub max_depth: usize,
    pub max_parts: usize,
    pub max_attachment_bytes: usize,
    pub max_text_bytes: usize,
    pub max_html_bytes: usize,
    pub max_inline_image_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_message_bytes: 25 * 1024 * 1024,
            max_depth: 10,
            max_parts: 200,
            max_attachment_bytes: 25 * 1024 * 1024,
            max_text_bytes: 1024 * 1024,
            max_html_bytes: 2 * 1024 * 1024,
            max_inline_image_bytes: 2 * 1024 * 1024,
        }
    }
}

/// Why a message was refused. Carries numbers only, never content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MimeError {
    /// Raw message larger than `max_message_bytes`.
    TooLarge,
    /// MIME nesting deeper than `max_depth`.
    TooDeep,
    /// More MIME parts than `max_parts`.
    TooManyParts,
    /// The requested attachment decodes to more than `max_attachment_bytes`.
    AttachmentTooLarge,
    /// No attachment at that position.
    NoSuchAttachment,
    /// Not parseable as a mail message at all.
    Malformed,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Address {
    pub name: String,
    pub address: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Headers {
    pub subject: String,
    pub from: Vec<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    /// Seconds since the Unix epoch (UTC) from the `Date` header.
    pub date: Option<i64>,
    pub message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// Position for [`attachment_bytes`].
    pub part: u32,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub content_id: Option<String>,
    pub inline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedMessage {
    pub headers: Headers,
    pub text: String,
    /// True when `text` comes from a `text/plain` part (not derived from HTML).
    pub text_is_plain: bool,
    pub html: Option<String>,
    pub html_with_remote_images: Option<String>,
    pub remote_blocked: bool,
    pub truncated: bool,
    pub attachments: Vec<Attachment>,
}

impl ParsedMessage {
    /// The storable form: text, sanitized HTML and attachment metadata. No raw content.
    pub fn into_record(self, message_id: &str) -> BodyRecord {
        BodyRecord {
            message_id: message_id.to_string(),
            text: self.text,
            html: self.html,
            html_remote: self.html_with_remote_images,
            remote_blocked: self.remote_blocked,
            truncated: self.truncated,
            attachments: self
                .attachments
                .into_iter()
                .map(|a| AttachmentMeta {
                    part: a.part,
                    filename: a.filename,
                    media_type: a.content_type,
                    size_bytes: a.size_bytes,
                    content_id: a.content_id,
                    inline: a.inline,
                })
                .collect(),
        }
    }
}

const MAX_ADDRESSES: usize = 100;
const MAX_HEADER_CHARS: usize = 998;
/// One inline image larger than this is listed as an attachment but not embedded.
const MAX_ONE_INLINE_IMAGE: usize = 1024 * 1024;

/// Text safe to store and show: no NUL or other control characters except line breaks and tabs.
fn clean_text(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}

fn clean_line(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(MAX_HEADER_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

fn truncate(mut s: String, max: usize) -> (String, bool) {
    if s.len() <= max {
        return (s, false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    (s, true)
}

fn addresses(value: Option<&mail_parser::Address<'_>>) -> Vec<Address> {
    let convert = |a: &Addr<'_>| Address {
        name: clean_line(a.name().unwrap_or("")),
        address: clean_line(a.address().unwrap_or("")),
    };
    value
        .map(|list| list.iter().take(MAX_ADDRESSES).map(convert).collect())
        .unwrap_or_default()
}

/// Enforce depth and part-count limits over a parsed message, nested messages included.
/// `depth` is the nesting level of the message's root part.
fn check_structure(
    message: &Message<'_>,
    depth: usize,
    parts: &mut usize,
    limits: &Limits,
) -> Result<(), MimeError> {
    if depth > limits.max_depth {
        return Err(MimeError::TooDeep);
    }
    *parts += message.parts.len();
    if *parts > limits.max_parts {
        return Err(MimeError::TooManyParts);
    }
    let mut depths = vec![depth; message.parts.len()];
    for (i, part) in message.parts.iter().enumerate() {
        match &part.body {
            PartType::Multipart(children) => {
                for &child in children {
                    let child = child as usize;
                    // Children always come after their parent; anything else is not a tree.
                    if child <= i || child >= depths.len() {
                        return Err(MimeError::Malformed);
                    }
                    depths[child] = depths[i] + 1;
                    if depths[child] > limits.max_depth {
                        return Err(MimeError::TooDeep);
                    }
                }
            }
            PartType::Message(nested) => check_structure(nested, depths[i] + 1, parts, limits)?,
            _ => {}
        }
    }
    Ok(())
}

fn parse_checked<'x>(raw: &'x [u8], limits: &Limits) -> Result<Message<'x>, MimeError> {
    if raw.len() > limits.max_message_bytes {
        return Err(MimeError::TooLarge);
    }
    let message = MessageParser::default()
        .parse(raw)
        .ok_or(MimeError::Malformed)?;
    if message.parts.is_empty() {
        return Err(MimeError::Malformed);
    }
    check_structure(&message, 1, &mut 0, limits)?;
    Ok(message)
}

fn media_type(part: &mail_parser::MessagePart<'_>) -> String {
    let token = |s: &str| {
        !s.is_empty()
            && s.len() <= 64
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'+' | b'.' | b'_'))
    };
    match part.content_type() {
        Some(ct) if token(ct.ctype()) && ct.subtype().is_some_and(token) => {
            format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or("")).to_ascii_lowercase()
        }
        _ if part.is_message() => "message/rfc822".into(),
        _ => "application/octet-stream".into(),
    }
}

fn attachment_name(part: &mail_parser::MessagePart<'_>, index: usize) -> String {
    match part.attachment_name() {
        Some(name) if !name.trim().is_empty() => safe_filename(name),
        _ if part.is_message() => format!("message-{}.eml", index + 1),
        _ => format!("attachment-{}", index + 1),
    }
}

fn content_id(part: &mail_parser::MessagePart<'_>) -> Option<String> {
    let id = clean_line(part.content_id()?);
    let id = id.trim_matches(|c| c == '<' || c == '>').to_string();
    (!id.is_empty() && id.len() <= 255).then_some(id)
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Parse a raw message into the display model. HTML in the result is already sanitized.
pub fn parse(raw: &[u8], limits: &Limits) -> Result<ParsedMessage, MimeError> {
    let message = parse_checked(raw, limits)?;
    let headers = Headers {
        subject: clean_line(message.subject().unwrap_or("")),
        from: addresses(message.from()),
        to: addresses(message.to()),
        cc: addresses(message.cc()),
        date: message.date().map(|d| d.to_timestamp()),
        message_id: message
            .message_id()
            .map(clean_line)
            .filter(|s| !s.is_empty()),
    };

    let mut attachments = Vec::new();
    let mut inline = InlineImages::new();
    let mut inline_budget = limits.max_inline_image_bytes;
    for (index, &id) in message.attachments.iter().enumerate() {
        let Some(part) = message.parts.get(id as usize) else {
            return Err(MimeError::Malformed);
        };
        let content_type = media_type(part);
        let cid = content_id(part);
        let is_inline = matches!(part.body, PartType::InlineBinary(_))
            || part.content_disposition().is_some_and(|d| d.is_inline());
        let bytes = part.contents();
        if let Some(cid) = &cid
            && matches!(
                content_type.as_str(),
                "image/png" | "image/jpeg" | "image/gif" | "image/webp"
            )
            && bytes.len() <= MAX_ONE_INLINE_IMAGE.min(inline_budget)
        {
            inline_budget -= bytes.len();
            inline.insert(
                cid.clone(),
                format!("data:{content_type};base64,{}", base64(bytes)),
            );
        }
        attachments.push(Attachment {
            part: index as u32,
            filename: attachment_name(part, index),
            content_type,
            size_bytes: part.len() as u64,
            content_id: cid,
            inline: is_inline,
        });
    }

    // Text: every text body in order; a body that only exists as HTML is converted to text.
    let mut text_is_plain = false;
    let mut pieces = Vec::new();
    for (pos, &id) in message.text_body.iter().enumerate() {
        if matches!(
            message.parts.get(id as usize).map(|p| &p.body),
            Some(PartType::Text(_))
        ) {
            text_is_plain = true;
        }
        if let Some(t) = message.body_text(pos) {
            pieces.push(clean_text(&t));
        }
    }
    let (text, text_cut) = truncate(
        pieces.join("\n\n").trim().to_string(),
        limits.max_text_bytes,
    );

    // HTML: only real text/html parts (plain text is never "upgraded" to HTML).
    let mut raw_html = String::new();
    let mut html_too_large = false;
    for &id in &message.html_body {
        if let Some(PartType::Html(h)) = message.parts.get(id as usize).map(|p| &p.body) {
            if raw_html.len() + h.len() > limits.max_html_bytes {
                html_too_large = true;
                break;
            }
            raw_html.push_str(h);
        }
    }
    let (html, html_with_remote_images, remote_blocked) = if html_too_large || raw_html.is_empty() {
        (None, None, false)
    } else {
        let s = sanitize(&raw_html, &inline);
        (
            Some(clean_text(&s.html)),
            s.html_with_remote_images.map(|h| clean_text(&h)),
            s.remote_blocked,
        )
    };

    Ok(ParsedMessage {
        headers,
        text,
        text_is_plain,
        html,
        html_with_remote_images,
        remote_blocked,
        truncated: text_cut || html_too_large,
        attachments,
    })
}

/// Decoded bytes and safe file name of one attachment (`part` from [`Attachment::part`]).
pub fn attachment_bytes(
    raw: &[u8],
    part: u32,
    limits: &Limits,
) -> Result<(String, Vec<u8>), MimeError> {
    let message = parse_checked(raw, limits)?;
    let index = part as usize;
    let found = message
        .attachments
        .get(index)
        .and_then(|&id| message.parts.get(id as usize))
        .ok_or(MimeError::NoSuchAttachment)?;
    if found.len() > limits.max_attachment_bytes {
        return Err(MimeError::AttachmentTooLarge);
    }
    Ok((attachment_name(found, index), found.contents().to_vec()))
}
