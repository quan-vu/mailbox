//! Core boundaries for a future native Rust IMAP/SMTP implementation.
//! This crate deliberately does not open sockets or send messages yet.
pub mod actions;
pub mod body;
pub mod demo;
pub mod folders;
pub mod history;
pub mod link;
pub mod live;
pub mod notify;
pub mod outbox;
pub mod prefs;
pub mod search;
pub mod secrets;
pub mod signature;
pub mod store;
pub mod sync;
pub mod threading;

use mailbox_contracts::MessageSummary;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MailboxError {
    #[error("account requires reauthentication")]
    ReauthenticationRequired,
    #[error("mail server is temporarily unavailable")]
    TransientConnectionFailure,
    #[error("mailbox UIDVALIDITY changed; full resync required")]
    UidValidityChanged,
    #[error("operation not yet implemented")]
    NotImplemented,
    #[error("stored data is inconsistent: {0}")]
    InconsistentData(&'static str),
    #[error("not found")]
    NotFound,
    /// Storage failed. Carries a short static code only, never SQL text or row data.
    #[error("storage error: {0}")]
    Storage(&'static str),
}

/// UID is scoped to a mailbox and an epoch (`UIDVALIDITY`), not globally unique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapCursor {
    pub account_id: String,
    pub folder_id: String,
    pub uid_validity: u64,
    pub last_uid: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDecision {
    Incremental,
    FullResync,
}

impl ImapCursor {
    pub fn decide(&self, server_uid_validity: u64) -> SyncDecision {
        if self.uid_validity == server_uid_validity {
            SyncDecision::Incremental
        } else {
            SyncDecision::FullResync
        }
    }
}

/// Ports and TLS policies are validated before any network access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TlsMode {
    ImplicitTls,
    StartTls,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailServerEndpoint {
    pub hostname: String,
    pub port: u16,
    pub tls_mode: TlsMode,
}

impl MailServerEndpoint {
    /// Only a structural guard. DNS/IP SSRF policy MUST be enforced in the network adapter.
    pub fn validate_shape(&self) -> Result<(), &'static str> {
        let host = self.hostname.trim();
        if host.is_empty()
            || host.len() > 253
            || host.chars().any(|c| matches!(c, '/' | '\\' | '@' | ' '))
        {
            return Err("invalid hostname");
        }
        if self.port == 0 {
            return Err("invalid port");
        }
        Ok(())
    }
}

/// The presentation layer must not depend directly on a vendor-specific IMAP library.
pub trait MailReadPort {
    fn cached_messages(&self, account_id: &str) -> Result<Vec<MessageSummary>, MailboxError>;
}

/// This is a boundary contract, not a working SMTP sender.
pub trait MailSendPort {
    fn enqueue_draft(&self, draft_id: &str) -> Result<(), MailboxError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uid_validity_change_requires_full_resync() {
        let c = ImapCursor {
            account_id: "a".into(),
            folder_id: "inbox".into(),
            uid_validity: 12,
            last_uid: 88,
        };
        assert_eq!(c.decide(12), SyncDecision::Incremental);
        assert_eq!(c.decide(13), SyncDecision::FullResync);
    }
    #[test]
    fn rejects_invalid_endpoint_shapes() {
        assert!(
            MailServerEndpoint {
                hostname: "127.0.0.1/evil".into(),
                port: 993,
                tls_mode: TlsMode::ImplicitTls
            }
            .validate_shape()
            .is_err()
        );
        assert!(
            MailServerEndpoint {
                hostname: "imap.example.org".into(),
                port: 993,
                tls_mode: TlsMode::ImplicitTls
            }
            .validate_shape()
            .is_ok()
        );
    }
}
