//! Older mail on demand (MB-200) and lazy header backfill (MB-201).
//!
//! The first sync mirrors only the newest window of a folder. "Load older" walks down the UID
//! space below the lowest cached UID in bounded batches until UID 1 is covered, and says when the
//! start of the folder is reached. Everything is read-only on the server (`UID FETCH` with
//! `BODY.PEEK`), and every UID is used only together with the UIDVALIDITY it was fetched under.
use crate::MailboxError;
use crate::search::PageStore;
use crate::store::{FolderRecord, MailStore, MessageRecord};
use crate::sync::{RemoteEnvelope, RemoteMailbox};
use crate::threading::{HeaderRow, ThreadStore};

/// UID-addressed envelope fetches on the folder last examined.
#[allow(async_fn_in_trait)]
pub trait RemoteHistory {
    /// Envelopes with `lo <= UID <= hi`.
    async fn fetch_uid_range(
        &mut self,
        lo: u32,
        hi: u32,
    ) -> Result<Vec<RemoteEnvelope>, MailboxError>;
    /// Envelopes of exactly these UIDs (missing ones are simply absent).
    async fn fetch_uid_set(&mut self, uids: &[u32]) -> Result<Vec<RemoteEnvelope>, MailboxError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OlderReport {
    pub stored: usize,
    /// UID 1 is covered: there is nothing older on the server.
    pub reached_start: bool,
}

/// Widening rounds per call; each doubles the UID span.
const ROUNDS: u32 = 4;

/// Cache record + header row of a fetched envelope, with the same id scheme as `sync_folder`.
pub fn records_for(
    folder: &FolderRecord,
    uid_validity: u64,
    envelopes: &[RemoteEnvelope],
) -> (Vec<MessageRecord>, Vec<HeaderRow>) {
    let mut records = Vec::with_capacity(envelopes.len());
    let mut headers = Vec::with_capacity(envelopes.len());
    for e in envelopes {
        let id = format!("{}#{uid_validity}#{}", folder.id, e.uid);
        headers.push(HeaderRow {
            message_id: id.clone(),
            account_id: folder.account_id.clone(),
            headers: e.headers.clone(),
        });
        records.push(MessageRecord {
            id,
            account_id: folder.account_id.clone(),
            folder_id: folder.id.clone(),
            uid_validity,
            imap_uid: e.uid,
            message_id_header: e.message_id.clone(),
            subject: e.subject.clone(),
            from_display: e.from_display.clone(),
            from_address: e.from_address.clone(),
            preview: String::new(),
            internal_date: e.internal_date,
            size_bytes: e.size_bytes,
            seen: e.seen,
            flagged: e.flagged,
        });
    }
    (records, headers)
}

/// Fetch up to about `batch` envelopes older than the lowest cached UID of `folder`.
///
/// Refuses with `UidValidityChanged` when the server's UIDVALIDITY differs from the cached one:
/// the normal sync must rebuild the folder first. The sync cursor (highest UID) is not touched.
pub async fn fetch_older<St, R>(
    store: &St,
    remote: &mut R,
    owner_id: &str,
    folder: &FolderRecord,
    batch: u32,
) -> Result<OlderReport, MailboxError>
where
    St: MailStore + PageStore,
    R: RemoteMailbox + RemoteHistory,
{
    let batch = batch.clamp(1, 500);
    let cursor = store
        .load_cursor(owner_id, &folder.account_id, &folder.id)
        .await?
        .ok_or(MailboxError::NotFound)?;
    let status = remote.examine(&folder.remote_name).await?;
    if status.uid_validity != Some(cursor.uid_validity) {
        return Err(MailboxError::UidValidityChanged);
    }
    let uid_validity = cursor.uid_validity;
    let lowest = store
        .lowest_uid(owner_id, &folder.account_id, &folder.id, uid_validity)
        .await?;
    let Some(lowest) = lowest else {
        // Nothing cached: an empty folder, or one never synced.
        return Ok(OlderReport {
            stored: 0,
            reached_start: status.exists == 0,
        });
    };
    let mut hi = lowest.saturating_sub(1);
    let mut span = batch;
    let mut stored = 0;
    for _ in 0..ROUNDS {
        if hi == 0 {
            break;
        }
        let lo = hi.saturating_sub(span - 1).max(1);
        let envelopes: Vec<RemoteEnvelope> = remote
            .fetch_uid_range(lo, hi)
            .await?
            .into_iter()
            .filter(|e| e.uid >= lo && e.uid <= hi)
            .collect();
        if !envelopes.is_empty() {
            let (records, headers) = records_for(folder, uid_validity, &envelopes);
            store.upsert_messages(owner_id, &records).await?;
            store.store_headers(owner_id, &headers).await?;
            stored += records.len();
        }
        hi = lo - 1;
        if stored > 0 {
            break;
        }
        // An empty stretch of UIDs (deleted mail): look further down with a wider span.
        span = span.saturating_mul(2).min(batch * 8);
    }
    Ok(OlderReport {
        stored,
        reached_start: hi == 0,
    })
}

/// Fetch and store headers for up to `max` cached messages of `folder` that were synced before
/// headers were kept (lazy backfill). Returns the number of messages updated.
pub async fn backfill_headers<St, R>(
    store: &St,
    remote: &mut R,
    owner_id: &str,
    folder: &FolderRecord,
    max: u32,
) -> Result<usize, MailboxError>
where
    St: MailStore + ThreadStore,
    R: RemoteMailbox + RemoteHistory,
{
    let Some(cursor) = store
        .load_cursor(owner_id, &folder.account_id, &folder.id)
        .await?
    else {
        return Ok(0);
    };
    let missing = store
        .uids_missing_headers(
            owner_id,
            &folder.account_id,
            &folder.id,
            cursor.uid_validity,
            max,
        )
        .await?;
    if missing.is_empty() {
        return Ok(0);
    }
    let status = remote.examine(&folder.remote_name).await?;
    if status.uid_validity != Some(cursor.uid_validity) {
        return Err(MailboxError::UidValidityChanged);
    }
    let mut updated = 0;
    for chunk in missing.chunks(100) {
        let envelopes: Vec<RemoteEnvelope> = remote
            .fetch_uid_set(chunk)
            .await?
            .into_iter()
            .filter(|e| chunk.contains(&e.uid))
            .collect();
        let (records, mut headers) = records_for(folder, cursor.uid_validity, &envelopes);
        // Gone from the server since: mark as tried with empty headers so it is not asked for
        // again on every sync (the next flag scan or rebuild removes the row).
        for uid in chunk
            .iter()
            .filter(|u| !envelopes.iter().any(|e| e.uid == **u))
        {
            headers.push(HeaderRow {
                message_id: format!("{}#{}#{uid}", folder.id, cursor.uid_validity),
                account_id: folder.account_id.clone(),
                headers: Default::default(),
            });
        }
        store.upsert_messages(owner_id, &records).await?;
        store.store_headers(owner_id, &headers).await?;
        updated += records.len();
    }
    Ok(updated)
}
