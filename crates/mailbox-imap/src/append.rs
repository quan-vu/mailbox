//! Writing to the server for sent mail and drafts (MB-151, MB-133): `APPEND`, a `Message-ID`
//! search to avoid duplicate copies, and a UID-scoped delete.
//!
//! These are the only operations in this crate's `Session` that change a mailbox, and each one
//! is an explicit call. Rules covered by `tests/append.rs`:
//! - `APPEND` sends the message as one synchronizing literal, byte for byte. A refusal is
//!   reported; nothing is retried here.
//! - The duplicate check opens the folder read-only (`EXAMINE`).
//! - Delete only ever touches the UIDs it is given: it needs `UIDPLUS` (`UID EXPUNGE`) and
//!   refuses to fall back to a plain `EXPUNGE`, which would also remove messages that another
//!   client had marked as deleted.
use crate::parse::{ListEntry, decode_mutf7};
use crate::{ConnectError, Session, quote_mailbox};
use tokio::io::{AsyncRead, AsyncWrite};

const CHUNK: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendError {
    /// The server said NO/BAD (quota, missing folder, size limit).
    Refused,
    /// Empty message or one containing NUL, which IMAP literals cannot carry.
    InvalidMessage,
    /// Not of the form `<id@domain>` or not safe to quote.
    InvalidMessageId,
    /// The server lacks an extension this operation needs to be safe (UIDPLUS).
    Unsupported,
    Connection(ConnectError),
}

impl From<ConnectError> for AppendError {
    fn from(e: ConnectError) -> Self {
        AppendError::Connection(e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendFlag {
    Seen,
    Draft,
}

/// Where the server stored the message, when it says so (`APPENDUID`, UIDPLUS).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Appended {
    pub uid_validity: Option<u64>,
    pub uid: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SentCopy {
    Appended {
        uid: Option<u32>,
    },
    /// A message with this `Message-ID` is already in the folder (the provider saved it).
    AlreadyThere,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderRole {
    Sent,
    Drafts,
}

/// Pick the folder for a role: SPECIAL-USE attribute first (RFC 6154), then a well-known name.
/// Returns `None` rather than guessing. Swap this for persisted folder roles once they exist.
pub fn find_folder(entries: &[ListEntry], role: FolderRole) -> Option<&ListEntry> {
    let (attribute, names): (&str, &[&str]) = match role {
        FolderRole::Sent => (
            "sent",
            &[
                "sent",
                "sent items",
                "sent messages",
                "sent mail",
                "đã gửi",
                "thư đã gửi",
            ],
        ),
        FolderRole::Drafts => ("drafts", &["drafts", "draft", "bản nháp", "thư nháp"]),
    };
    let usable = || entries.iter().filter(|e| e.selectable());
    usable()
        .find(|e| e.special_use() == Some(attribute))
        .or_else(|| {
            usable().find(|e| {
                let decoded = decode_mutf7(&e.name).to_lowercase();
                let leaf = match e.delimiter.as_deref() {
                    Some(d) if !d.is_empty() => decoded.rsplit(d).next().unwrap_or(&decoded),
                    _ => decoded.as_str(),
                };
                names.contains(&leaf)
            })
        })
}

fn message_id_ok(id: &str) -> bool {
    id.len() <= 250
        && id
            .strip_prefix('<')
            .and_then(|s| s.strip_suffix('>'))
            .and_then(|inner| inner.split_once('@'))
            .is_some_and(|(l, r)| !l.is_empty() && !r.is_empty())
        && id
            .bytes()
            .all(|b| b.is_ascii_graphic() && !matches!(b, b'"' | b'\\' | b'{' | b'}'))
        && id.matches(['<', '>']).count() == 2
}

fn uid_set(uids: &[u32]) -> String {
    uids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// `[APPENDUID validity uid]` from a tagged OK line.
fn parse_appenduid(line: &str) -> Appended {
    let upper = line.to_ascii_uppercase();
    let Some(start) = upper.find("[APPENDUID ") else {
        return Appended::default();
    };
    let mut parts = line[start + 11..].split([' ', ']']);
    Appended {
        uid_validity: parts.next().and_then(|v| v.parse().ok()),
        uid: parts.next().and_then(|v| v.parse().ok()),
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> Session<S> {
    /// Ask for capabilities again. Many servers only list extensions such as UIDPLUS after login.
    pub async fn refresh_capabilities(&mut self) -> Result<(), ConnectError> {
        self.info = self.d.capabilities().await?;
        Ok(())
    }

    /// Store `message` (RFC 5322 bytes, CRLF line endings) in `folder`.
    pub async fn append(
        &mut self,
        folder: &str,
        flags: &[AppendFlag],
        message: &[u8],
    ) -> Result<Appended, AppendError> {
        if message.is_empty() || message.contains(&0) {
            return Err(AppendError::InvalidMessage);
        }
        let mailbox = quote_mailbox(folder)?;
        let flags: Vec<&str> = flags
            .iter()
            .map(|f| match f {
                AppendFlag::Seen => "\\Seen",
                AppendFlag::Draft => "\\Draft",
            })
            .collect();
        let tag = self.d.next_tag();
        self.d
            .send(
                format!(
                    "{tag} APPEND {mailbox} ({}) {{{}}}\r\n",
                    flags.join(" "),
                    message.len()
                )
                .as_bytes(),
            )
            .await?;
        // Wait for the go-ahead; a tagged reply here means the server will not take the message.
        loop {
            let line = self.d.line().await?;
            if line.starts_with('+') {
                break;
            }
            if line.starts_with(&format!("{tag} ")) {
                return Err(AppendError::Refused);
            }
            if !line.starts_with('*') {
                return Err(ConnectError::NotImap.into());
            }
        }
        for chunk in message.chunks(CHUNK) {
            self.d.send(chunk).await?;
        }
        self.d.send(b"\r\n").await?;
        loop {
            let logical = self.d.logical().await?;
            let line = logical.texts.first().map(String::as_str).unwrap_or("");
            if let Some(rest) = line
                .strip_prefix(tag.as_str())
                .and_then(|r| r.strip_prefix(' '))
            {
                return if rest.to_ascii_uppercase().starts_with("OK") {
                    Ok(parse_appenduid(rest))
                } else {
                    Err(AppendError::Refused)
                };
            }
            if !line.starts_with('*') {
                return Err(ConnectError::NotImap.into());
            }
        }
    }

    /// UIDs of the messages in `folder` whose `Message-ID` header contains `message_id`.
    /// Opens the folder read-only.
    pub async fn find_by_message_id(
        &mut self,
        folder: &str,
        message_id: &str,
    ) -> Result<Vec<u32>, AppendError> {
        if !message_id_ok(message_id) {
            return Err(AppendError::InvalidMessageId);
        }
        self.examine(folder).await?;
        let lines = self
            .d
            .run(&format!("UID SEARCH HEADER Message-ID \"{message_id}\""))
            .await?;
        Ok(lines
            .iter()
            .filter_map(|l| l.texts.first())
            .filter_map(|t| {
                let upper = t.to_ascii_uppercase();
                upper.starts_with("* SEARCH").then(|| t[8..].to_string())
            })
            .flat_map(|rest| {
                rest.split_ascii_whitespace()
                    .filter_map(|n| n.parse().ok())
                    .collect::<Vec<u32>>()
            })
            .collect())
    }

    /// Permanently remove exactly these UIDs from `folder`. Needs UIDPLUS.
    pub async fn delete_uids(&mut self, folder: &str, uids: &[u32]) -> Result<(), AppendError> {
        if uids.is_empty() {
            return Ok(());
        }
        if !self.info.has("UIDPLUS") {
            return Err(AppendError::Unsupported);
        }
        let set = uid_set(uids);
        self.d
            .run(&format!("SELECT {}", quote_mailbox(folder)?))
            .await?;
        self.d
            .run(&format!("UID STORE {set} +FLAGS.SILENT (\\Deleted)"))
            .await?;
        self.d.run(&format!("UID EXPUNGE {set}")).await?;
        Ok(())
    }
}

/// Keep one copy of a sent message in `folder`: skip when a message with the same `Message-ID`
/// is already there, otherwise APPEND it marked as read.
pub async fn save_sent_copy<S: AsyncRead + AsyncWrite + Unpin>(
    session: &mut Session<S>,
    folder: &str,
    message_id: &str,
    message: &[u8],
) -> Result<SentCopy, AppendError> {
    if !session
        .find_by_message_id(folder, message_id)
        .await?
        .is_empty()
    {
        return Ok(SentCopy::AlreadyThere);
    }
    let appended = session.append(folder, &[AppendFlag::Seen], message).await?;
    Ok(SentCopy::Appended { uid: appended.uid })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appenduid_is_parsed_and_optional() {
        let a = parse_appenduid("OK [APPENDUID 38505 3955] done");
        assert_eq!((a.uid_validity, a.uid), (Some(38505), Some(3955)));
        assert_eq!(parse_appenduid("OK done"), Appended::default());
        assert_eq!(parse_appenduid("OK [APPENDUID x]"), Appended::default());
    }

    #[test]
    fn message_ids_must_be_safe_to_quote() {
        assert!(message_id_ok("<a.b-1@example.com>"));
        for bad in [
            "<a@b>x",
            "a@b",
            "<a\"@b>",
            "<a@b> <c@d>",
            "<a@b\\>",
            "<a{3}@b>",
            "<@b>",
            "<a@>",
        ] {
            assert!(!message_id_ok(bad), "{bad}");
        }
    }
}
