//! Bounded MIME parsing and HTML sanitising for received mail (MB-130, MB-131).
//!
//! Everything that enters here is hostile: it was written by whoever sent the mail. The rules:
//! - Limits first. Message size is checked before parsing; nesting depth, part count and decoded
//!   sizes right after. Violations are typed errors, never panics.
//! - HTML never leaves this crate unsanitised ([`sanitize`]): allow-list of tags and attributes,
//!   no scripts, forms, frames or style sheets, and no remote reference that loads by itself.
//! - File names from the message are reduced to one harmless path component ([`safe_filename`]).
//! - Nothing here logs, and errors carry no message content.
mod filename;
mod parse;
pub mod sanitize;

pub use filename::safe_filename;
pub use parse::{
    Address, Attachment, Headers, Limits, MimeError, ParsedMessage, attachment_bytes, parse,
};
