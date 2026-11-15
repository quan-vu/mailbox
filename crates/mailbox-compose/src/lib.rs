//! Outgoing mail construction (MB-150): address validation and RFC 5322 / MIME building.
//!
//! Pure functions, no I/O, no clock and no randomness: the caller passes the date and the
//! `Message-ID`, so the same draft always renders to the same bytes. That is what makes a
//! duplicate detectable after an uncertain SMTP outcome.
//!
//! Security properties, covered by tests:
//! - No header value, address, message id or file name can contain CR, LF or NUL: such input
//!   is rejected, never "cleaned", so nothing can add a header or a recipient.
//! - `Bcc` recipients are part of the SMTP envelope only and never appear in the message.
//! - Every line of the output is at most 998 bytes (RFC 5322); long text is transfer-encoded.
//! - Attachment file names are reduced to their base name (no directories).
pub mod address;
mod encode;
pub mod message;
pub mod reply;

#[cfg(test)]
mod tests;
