//! Folder discovery and envelope sync (MB-121). The mail server stays the source of truth
//! (ADR 0002): this only mirrors what the server reports and never writes to it.
//!
//! Cursor safety: messages are stored first and the cursor is advanced last, so after a crash
//! the cursor is never ahead of the stored data — the worst case is re-fetching, and message
//! ids are deterministic so re-fetching cannot duplicate rows.
use crate::store::{FolderRecord, MailStore};
use crate::{ImapCursor, MailboxError, SyncDecision};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFolder {
    pub name: String,
    pub delimiter: Option<String>,
    pub special_use: Option<String>,
    pub selectable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteStatus {
    pub exists: u32,
    pub uid_validity: Option<u64>,
    pub uid_next: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoteEnvelope {
    pub uid: u32,
    pub seen: bool,
    pub flagged: bool,
    pub internal_date: i64,
    pub size_bytes: u64,
    pub subject: String,
    pub from_display: String,
    pub from_address: String,
    pub message_id: Option<String>,
    /// `In-Reply-To`, `References`, `To`, `Cc` (MB-201).
    pub headers: crate::threading::EnvelopeHeaders,
}

/// Read-only view of one account on a mail server.
#[allow(async_fn_in_trait)]
pub trait RemoteMailbox {
    async fn list_folders(&mut self) -> Result<Vec<RemoteFolder>, MailboxError>;
    /// Open `folder` read-only; later fetches refer to it.
    async fn examine(&mut self, folder: &str) -> Result<RemoteStatus, MailboxError>;
    async fn fetch_latest(
        &mut self,
        exists: u32,
        count: u32,
    ) -> Result<Vec<RemoteEnvelope>, MailboxError>;
    async fn fetch_from_uid(&mut self, from_uid: u32) -> Result<Vec<RemoteEnvelope>, MailboxError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncKind {
    /// No cursor yet: newest window only.
    Initial,
    /// Same UIDVALIDITY: only UIDs above the cursor.
    Incremental,
    /// UIDVALIDITY changed: cached rows of the old epoch were discarded and the folder rebuilt.
    Rebuilt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncReport {
    pub kind: SyncKind,
    pub stored: usize,
}

pub fn folder_id(account_id: &str, remote_name: &str) -> String {
    format!("{account_id}/{remote_name}")
}

/// Mirror the server's folder list into the store. Non-selectable containers are skipped.
pub async fn discover_folders(
    store: &impl MailStore,
    remote: &mut impl RemoteMailbox,
    owner_id: &str,
    account_id: &str,
) -> Result<Vec<FolderRecord>, MailboxError> {
    let mut out = Vec::new();
    for f in remote
        .list_folders()
        .await?
        .into_iter()
        .filter(|f| f.selectable)
    {
        let record = FolderRecord {
            id: folder_id(account_id, &f.name),
            account_id: account_id.to_string(),
            remote_name: f.name,
            delimiter: f.delimiter,
            special_use: f.special_use,
            uid_validity: None,
            uid_next: None,
        };
        store.upsert_folder(owner_id, &record).await?;
        out.push(record);
    }
    Ok(out)
}

/// Sync one folder's envelopes. `window` bounds the first sync and a rebuild.
pub async fn sync_folder(
    store: &impl MailStore,
    remote: &mut impl RemoteMailbox,
    owner_id: &str,
    folder: &FolderRecord,
    window: u32,
) -> Result<SyncReport, MailboxError> {
    let status = remote.examine(&folder.remote_name).await?;
    // Without UIDVALIDITY, UIDs cannot be trusted across sessions: refuse rather than guess.
    let uid_validity = status
        .uid_validity
        .ok_or(MailboxError::InconsistentData("server sent no UIDVALIDITY"))?;
    let cursor = store
        .load_cursor(owner_id, &folder.account_id, &folder.id)
        .await?;
    let (kind, envelopes, floor) = match cursor
        .as_ref()
        .map(|c| (c.decide(uid_validity), c.last_uid))
    {
        None => (
            SyncKind::Initial,
            remote.fetch_latest(status.exists, window).await?,
            0,
        ),
        Some((SyncDecision::Incremental, last)) => (
            SyncKind::Incremental,
            remote.fetch_from_uid(last.saturating_add(1)).await?,
            last,
        ),
        Some((SyncDecision::FullResync, _)) => {
            // UID 5 under the old epoch is not UID 5 now: drop the old mapping before rebuilding.
            store
                .reset_folder(owner_id, &folder.account_id, &folder.id)
                .await?;
            (
                SyncKind::Rebuilt,
                remote.fetch_latest(status.exists, window).await?,
                0,
            )
        }
    };
    let fresh: Vec<RemoteEnvelope> = envelopes.into_iter().filter(|e| e.uid > floor).collect();
    let (records, headers) = crate::history::records_for(folder, uid_validity, &fresh);
    let mut updated = folder.clone();
    updated.uid_validity = Some(uid_validity);
    updated.uid_next = status.uid_next;
    store.upsert_folder(owner_id, &updated).await?;
    store.upsert_messages(owner_id, &records).await?;
    store.store_headers(owner_id, &headers).await?;
    // Cursor last. See module docs.
    let last_uid = records
        .iter()
        .map(|r| r.imap_uid)
        .max()
        .unwrap_or(0)
        .max(floor);
    store
        .save_cursor(
            owner_id,
            &ImapCursor {
                account_id: folder.account_id.clone(),
                folder_id: folder.id.clone(),
                uid_validity,
                last_uid,
            },
        )
        .await?;
    Ok(SyncReport {
        kind,
        stored: records.len(),
    })
}
