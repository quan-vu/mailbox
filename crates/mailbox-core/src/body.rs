//! Storage port for fetched message bodies (MB-130). Implementations live in `mailbox-store`.
//!
//! A body record holds only what is safe to show: decoded text, HTML that already went through
//! the allow-list sanitizer, and attachment *metadata*. Attachment bytes are never stored; they
//! are fetched again from the server when the user asks to save one.
use crate::MailboxError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentMeta {
    /// Position in the message's attachment list; stable for the same raw message.
    pub part: u32,
    /// Display name, already reduced to a single safe path component.
    pub filename: String,
    pub media_type: String,
    /// Decoded size in bytes.
    pub size_bytes: u64,
    pub content_id: Option<String>,
    pub inline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BodyRecord {
    /// `MessageRecord::id` this body belongs to.
    pub message_id: String,
    /// Plain text to show (from `text/plain`, or derived from HTML when there is none).
    pub text: String,
    /// Sanitized HTML with every remote reference removed. `None` when the message has no HTML.
    pub html: Option<String>,
    /// Sanitized HTML that keeps remote `<img>` sources, for the explicit per-message
    /// "show images" action. `None` when the message references no remote image.
    pub html_remote: Option<String>,
    /// True when the sanitizer removed at least one remote reference.
    pub remote_blocked: bool,
    /// True when text or HTML was cut or dropped because of a size limit.
    pub truncated: bool,
    pub attachments: Vec<AttachmentMeta>,
}

/// Where a cached message lives on the server; what is needed to fetch its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageLocator {
    pub account_id: String,
    pub folder_remote_name: String,
    pub uid_validity: u64,
    pub imap_uid: u32,
    pub size_bytes: u64,
}

/// Every operation is scoped by `owner_id`; a message of another owner behaves like a missing one.
#[allow(async_fn_in_trait)]
pub trait BodyStore {
    async fn locate_message(
        &self,
        owner_id: &str,
        message_id: &str,
    ) -> Result<MessageLocator, MailboxError>;
    /// Insert or replace the cached body of one message, including its attachment list.
    async fn save_body(&self, owner_id: &str, body: &BodyRecord) -> Result<(), MailboxError>;
    async fn load_body(
        &self,
        owner_id: &str,
        message_id: &str,
    ) -> Result<Option<BodyRecord>, MailboxError>;
    /// Drop one cached body. Local only; the server is never touched.
    async fn delete_body(&self, owner_id: &str, message_id: &str) -> Result<(), MailboxError>;
}
