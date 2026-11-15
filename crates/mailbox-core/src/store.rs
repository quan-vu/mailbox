//! Storage port for local mailbox metadata (MB-120). Implementations live in `mailbox-store`.
//! Every operation is scoped by `owner_id`; an account that belongs to another owner behaves
//! exactly like one that does not exist. Records never contain credentials, only `credential_ref`.
use crate::{ImapCursor, MailboxError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRecord {
    pub id: String,
    pub owner_id: String,
    pub display_name: String,
    pub email_address: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    /// e.g. `password`, `oauth2`. Describes the scheme, never the secret.
    pub auth_type: String,
    /// Opaque pointer into the OS keychain or the encrypted vault.
    pub credential_ref: String,
    /// `disabled`, `active`, `needs_attention`.
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderRecord {
    pub id: String,
    pub account_id: String,
    pub remote_name: String,
    pub delimiter: Option<String>,
    pub special_use: Option<String>,
    pub uid_validity: Option<u64>,
    pub uid_next: Option<u64>,
}

/// Envelope-level cache row. A message is identified by
/// `(account, folder, uid_validity, imap_uid)`, never by the UID alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRecord {
    pub id: String,
    pub account_id: String,
    pub folder_id: String,
    pub uid_validity: u64,
    pub imap_uid: u32,
    pub message_id_header: Option<String>,
    pub subject: String,
    pub from_display: String,
    pub from_address: String,
    pub preview: String,
    /// Seconds since the Unix epoch, UTC.
    pub internal_date: i64,
    pub size_bytes: u64,
    pub seen: bool,
    pub flagged: bool,
}

#[allow(async_fn_in_trait)]
pub trait MailStore {
    async fn upsert_account(&self, account: &AccountRecord) -> Result<(), MailboxError>;
    async fn list_accounts(&self, owner_id: &str) -> Result<Vec<AccountRecord>, MailboxError>;
    async fn get_account(
        &self,
        owner_id: &str,
        account_id: &str,
    ) -> Result<AccountRecord, MailboxError>;
    /// Removes local metadata only. It never touches the mail server.
    async fn delete_account(&self, owner_id: &str, account_id: &str) -> Result<(), MailboxError>;

    async fn upsert_folder(
        &self,
        owner_id: &str,
        folder: &FolderRecord,
    ) -> Result<(), MailboxError>;
    async fn list_folders(
        &self,
        owner_id: &str,
        account_id: &str,
    ) -> Result<Vec<FolderRecord>, MailboxError>;

    async fn upsert_messages(
        &self,
        owner_id: &str,
        messages: &[MessageRecord],
    ) -> Result<(), MailboxError>;
    /// Newest first, at most `limit` rows.
    async fn list_messages(
        &self,
        owner_id: &str,
        account_id: &str,
        folder_id: &str,
        limit: u32,
    ) -> Result<Vec<MessageRecord>, MailboxError>;

    /// Discard the cached messages and cursor of one folder (UIDVALIDITY changed). Local only.
    async fn reset_folder(
        &self,
        owner_id: &str,
        account_id: &str,
        folder_id: &str,
    ) -> Result<(), MailboxError>;

    async fn save_cursor(&self, owner_id: &str, cursor: &ImapCursor) -> Result<(), MailboxError>;

    /// Threading and recipient headers of cached messages (MB-201), stored next to them.
    /// Added after the first release of this trait, hence the default: a store that does not
    /// keep headers simply ignores them (conversations then fall back to one row per message).
    /// Both stores in `mailbox-store` implement it.
    async fn store_headers(
        &self,
        owner_id: &str,
        rows: &[crate::threading::HeaderRow],
    ) -> Result<(), MailboxError> {
        let _ = (owner_id, rows);
        Ok(())
    }
    async fn load_cursor(
        &self,
        owner_id: &str,
        account_id: &str,
        folder_id: &str,
    ) -> Result<Option<ImapCursor>, MailboxError>;
}
