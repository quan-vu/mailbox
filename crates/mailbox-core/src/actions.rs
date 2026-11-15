//! Server-backed message state (MB-132 flags, MB-144 archive / trash / move).
//!
//! A change is applied to the local cache at once, recorded as a pending operation, and replayed
//! on the next connection. The mail server stays the source of truth (ADR 0002):
//! - an operation carries the UIDVALIDITY it was queued under; if the folder's epoch differs at
//!   replay time the operation is dropped and the folder's cache discarded, because the UID now
//!   names another message;
//! - a refusal rolls the local change back and leaves a visible failed operation;
//! - a lost connection leaves the operation queued; a move is preceded by an existence check so a
//!   move that already reached the server is never performed twice;
//! - while an operation waits, its flag is not overwritten by what the server reports; once it is
//!   confirmed or rejected the server wins.
use crate::MailboxError;
use crate::folders::{FolderRole, role_of};
use crate::store::{FolderRecord, MailStore, MessageRecord};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageFlag {
    Seen,
    Flagged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpKind {
    SetFlag {
        flag: MessageFlag,
        on: bool,
    },
    Move {
        dest_folder_id: String,
    },
    /// Permanent delete. Only queued for messages in the trash folder.
    Expunge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpState {
    Queued,
    /// The server refused. Carries a short code, never server text.
    Failed(String),
}

/// What to record. The message's identity is copied in full: UID alone means nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOp {
    pub account_id: String,
    pub message_id: String,
    pub folder_id: String,
    pub uid_validity: u64,
    pub imap_uid: u32,
    pub kind: OpKind,
    /// For flag changes: the value before the change, used to roll back.
    pub previous: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingOp {
    pub id: String,
    /// Replay order.
    pub seq: u64,
    pub account_id: String,
    pub message_id: String,
    pub folder_id: String,
    pub uid_validity: u64,
    pub imap_uid: u32,
    pub kind: OpKind,
    pub previous: Option<bool>,
    pub state: OpState,
}

/// Storage for pending operations and the single-message access they need. Owner-scoped;
/// operations disappear with their message, folder or account.
#[allow(async_fn_in_trait)]
pub trait PendingOpStore {
    async fn get_message(
        &self,
        owner_id: &str,
        message_id: &str,
    ) -> Result<MessageRecord, MailboxError>;
    /// Change cached flags; `None` leaves a flag as it is. Local only.
    async fn set_local_flags(
        &self,
        owner_id: &str,
        message_id: &str,
        seen: Option<bool>,
        flagged: Option<bool>,
    ) -> Result<(), MailboxError>;
    /// Drop one cached message. Local only.
    async fn remove_message(&self, owner_id: &str, message_id: &str) -> Result<(), MailboxError>;
    /// Every cached message of a folder.
    async fn folder_messages(
        &self,
        owner_id: &str,
        account_id: &str,
        folder_id: &str,
    ) -> Result<Vec<MessageRecord>, MailboxError>;

    async fn enqueue_op(&self, owner_id: &str, op: &NewOp) -> Result<PendingOp, MailboxError>;
    /// All operations of an account in replay order, failed ones included.
    async fn list_ops(
        &self,
        owner_id: &str,
        account_id: &str,
    ) -> Result<Vec<PendingOp>, MailboxError>;
    /// Persist a changed `kind` or `state` of an existing operation.
    async fn update_op(&self, owner_id: &str, op: &PendingOp) -> Result<(), MailboxError>;
    /// Returns false when there was nothing to remove.
    async fn remove_op(&self, owner_id: &str, op_id: &str) -> Result<bool, MailboxError>;
}

/// Why a server call did not succeed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteFailure {
    /// The server answered NO or BAD: the change will not happen.
    Refused,
    /// No answer: nothing is known about the outcome.
    ConnectionLost,
    /// The answer could not be understood.
    Protocol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selected {
    pub uid_validity: Option<u64>,
    pub read_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moved {
    /// `(UIDVALIDITY, UID)` in the destination, when the server reported it (COPYUID).
    pub new_uid: Option<(u64, u32)>,
    /// The copy exists but the original could only be flagged `\Deleted` (no UIDPLUS).
    pub source_left_deleted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expunged {
    Removed,
    /// Flagged `\Deleted` only: without UIDPLUS a single message cannot be expunged safely.
    LeftDeleted,
}

/// A read-write view of one account. All calls after `select` refer to the selected folder.
#[allow(async_fn_in_trait)]
pub trait MutableMailbox {
    async fn select(&mut self, folder: &str) -> Result<Selected, RemoteFailure>;
    async fn uid_exists(&mut self, uid: u32) -> Result<bool, RemoteFailure>;
    async fn store_flag(
        &mut self,
        uid: u32,
        flag: MessageFlag,
        on: bool,
    ) -> Result<(), RemoteFailure>;
    async fn move_message(&mut self, uid: u32, dest_folder: &str) -> Result<Moved, RemoteFailure>;
    async fn expunge_message(&mut self, uid: u32) -> Result<Expunged, RemoteFailure>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteFlags {
    pub uid: u32,
    pub seen: bool,
    pub flagged: bool,
}

/// Flags of the messages with `first_uid <= UID <= last_uid` in the currently opened folder.
#[allow(async_fn_in_trait)]
pub trait FlagSource {
    async fn fetch_flags(
        &mut self,
        first_uid: u32,
        last_uid: u32,
    ) -> Result<Vec<RemoteFlags>, MailboxError>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum ActionError {
    /// Unknown message or folder.
    NotFound,
    /// The account has no folder with this role; nothing is invented.
    NoFolderForRole(FolderRole),
    SameFolder,
    /// Permanent delete was asked for a message outside the trash.
    NotInTrash,
    Store(MailboxError),
}

impl From<MailboxError> for ActionError {
    fn from(e: MailboxError) -> Self {
        match e {
            MailboxError::NotFound => ActionError::NotFound,
            other => ActionError::Store(other),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Queued {
    /// The message already has this value.
    NoChange,
    Op(PendingOp),
    /// The change undid a still-queued one, so nothing needs to be sent.
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveTarget {
    Role(FolderRole),
    Folder(String),
}

fn flag_of(m: &MessageRecord, flag: MessageFlag) -> bool {
    match flag {
        MessageFlag::Seen => m.seen,
        MessageFlag::Flagged => m.flagged,
    }
}

async fn write_flag(
    store: &impl PendingOpStore,
    owner_id: &str,
    message_id: &str,
    flag: MessageFlag,
    on: bool,
) -> Result<(), MailboxError> {
    match flag {
        MessageFlag::Seen => {
            store
                .set_local_flags(owner_id, message_id, Some(on), None)
                .await
        }
        MessageFlag::Flagged => {
            store
                .set_local_flags(owner_id, message_id, None, Some(on))
                .await
        }
    }
}

fn new_op(m: &MessageRecord, kind: OpKind, previous: Option<bool>) -> NewOp {
    NewOp {
        account_id: m.account_id.clone(),
        message_id: m.id.clone(),
        folder_id: m.folder_id.clone(),
        uid_validity: m.uid_validity,
        imap_uid: m.imap_uid,
        kind,
        previous,
    }
}

/// Forget earlier failures of a message: the user is acting on it again.
async fn clear_failed(
    store: &impl PendingOpStore,
    owner_id: &str,
    ops: &[PendingOp],
    message_id: &str,
) -> Result<(), MailboxError> {
    for op in ops
        .iter()
        .filter(|o| o.message_id == message_id && matches!(o.state, OpState::Failed(_)))
    {
        store.remove_op(owner_id, &op.id).await?;
    }
    Ok(())
}

/// Set `\Seen` or `\Flagged`: local cache now, server on the next replay.
pub async fn queue_flag(
    store: &impl PendingOpStore,
    owner_id: &str,
    message_id: &str,
    flag: MessageFlag,
    on: bool,
) -> Result<Queued, ActionError> {
    let m = store.get_message(owner_id, message_id).await?;
    let ops = store.list_ops(owner_id, &m.account_id).await?;
    clear_failed(store, owner_id, &ops, message_id).await?;
    let current = flag_of(&m, flag);
    if current == on {
        return Ok(Queued::NoChange);
    }
    write_flag(store, owner_id, message_id, flag, on).await?;
    let waiting = ops.iter().find(|o| {
        o.message_id == message_id
            && o.state == OpState::Queued
            && matches!(&o.kind, OpKind::SetFlag { flag: f, .. } if *f == flag)
    });
    if let Some(op) = waiting {
        // A flag has two values, so a second change while one is waiting takes it back.
        store.remove_op(owner_id, &op.id).await?;
        return Ok(Queued::Cancelled);
    }
    Ok(Queued::Op(
        store
            .enqueue_op(
                owner_id,
                &new_op(&m, OpKind::SetFlag { flag, on }, Some(current)),
            )
            .await?,
    ))
}

/// Queue a move to a role folder (archive, trash, ...) or to a specific folder.
pub async fn queue_move<St>(
    store: &St,
    owner_id: &str,
    message_id: &str,
    target: MoveTarget,
) -> Result<PendingOp, ActionError>
where
    St: MailStore + PendingOpStore,
{
    let m = store.get_message(owner_id, message_id).await?;
    let folders = store.list_folders(owner_id, &m.account_id).await?;
    let dest = match &target {
        MoveTarget::Role(role) => folders
            .iter()
            .find(|f| role_of(f) == Some(*role))
            .ok_or(ActionError::NoFolderForRole(*role))?,
        MoveTarget::Folder(id) => folders
            .iter()
            .find(|f| &f.id == id)
            .ok_or(ActionError::NotFound)?,
    };
    if dest.id == m.folder_id {
        return Err(ActionError::SameFolder);
    }
    let ops = store.list_ops(owner_id, &m.account_id).await?;
    clear_failed(store, owner_id, &ops, message_id).await?;
    let kind = OpKind::Move {
        dest_folder_id: dest.id.clone(),
    };
    // One destination per message: a newer choice replaces a move that has not been sent yet.
    if let Some(waiting) = ops.iter().find(|o| {
        o.message_id == message_id
            && o.state == OpState::Queued
            && matches!(o.kind, OpKind::Move { .. })
    }) {
        let mut changed = waiting.clone();
        changed.kind = kind;
        store.update_op(owner_id, &changed).await?;
        return Ok(changed);
    }
    Ok(store.enqueue_op(owner_id, &new_op(&m, kind, None)).await?)
}

/// Queue a permanent delete. Refused unless the message is in the account's trash folder.
pub async fn queue_expunge<St>(
    store: &St,
    owner_id: &str,
    message_id: &str,
) -> Result<PendingOp, ActionError>
where
    St: MailStore + PendingOpStore,
{
    let m = store.get_message(owner_id, message_id).await?;
    let folders = store.list_folders(owner_id, &m.account_id).await?;
    let in_trash = folders
        .iter()
        .any(|f| f.id == m.folder_id && role_of(f) == Some(FolderRole::Trash));
    if !in_trash {
        return Err(ActionError::NotInTrash);
    }
    let ops = store.list_ops(owner_id, &m.account_id).await?;
    clear_failed(store, owner_id, &ops, message_id).await?;
    Ok(store
        .enqueue_op(owner_id, &new_op(&m, OpKind::Expunge, None))
        .await?)
}

/// Take back an operation that has not been sent. Returns false when it is no longer queued
/// (already replayed, failed or unknown). A flag change is reverted locally.
pub async fn cancel_op(
    store: &impl PendingOpStore,
    owner_id: &str,
    account_id: &str,
    op_id: &str,
) -> Result<bool, MailboxError> {
    let ops = store.list_ops(owner_id, account_id).await?;
    let Some(op) = ops
        .iter()
        .find(|o| o.id == op_id && o.state == OpState::Queued)
    else {
        return Ok(false);
    };
    if !store.remove_op(owner_id, op_id).await? {
        return Ok(false);
    }
    if let (OpKind::SetFlag { flag, .. }, Some(previous)) = (&op.kind, op.previous) {
        write_flag(store, owner_id, &op.message_id, *flag, previous).await?;
    }
    Ok(true)
}

/// Remove the failed operations of a message (the user has seen the error).
pub async fn dismiss_failed(
    store: &impl PendingOpStore,
    owner_id: &str,
    account_id: &str,
    message_id: &str,
) -> Result<(), MailboxError> {
    let ops = store.list_ops(owner_id, account_id).await?;
    clear_failed(store, owner_id, &ops, message_id).await
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpResult {
    /// The server stored the flag.
    Confirmed,
    Moved {
        /// Cache id of the message in its new folder, when the server reported the new UID.
        new_message_id: Option<String>,
        source_left_deleted: bool,
    },
    Expunged,
    /// Permanent delete could only flag the message `\Deleted` (no UIDPLUS).
    LeftDeleted,
    /// The message was no longer in the folder: nothing was sent.
    Gone,
    /// Refused by the server; the local change was rolled back. Carries a short code.
    Rejected(String),
    /// The folder's UIDVALIDITY changed since the operation was queued.
    DroppedEpochChanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpReport {
    pub op: PendingOp,
    pub result: OpResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReplayReport {
    pub done: Vec<OpReport>,
    pub still_queued: usize,
    /// The connection failed part-way; the remaining operations wait for the next session.
    pub interrupted: bool,
}

enum Step {
    Done(OpResult),
    /// Stop replaying: the connection is unusable.
    Halt,
}

async fn reject(
    store: &impl PendingOpStore,
    owner_id: &str,
    op: &PendingOp,
    code: &str,
) -> Result<Step, MailboxError> {
    if let (OpKind::SetFlag { flag, .. }, Some(previous)) = (&op.kind, op.previous) {
        write_flag(store, owner_id, &op.message_id, *flag, previous).await?;
    }
    let mut failed = op.clone();
    failed.state = OpState::Failed(code.to_string());
    store.update_op(owner_id, &failed).await?;
    Ok(Step::Done(OpResult::Rejected(code.to_string())))
}

fn settle<T>(result: Result<T, RemoteFailure>) -> Result<Result<T, &'static str>, ()> {
    match result {
        Ok(v) => Ok(Ok(v)),
        Err(RemoteFailure::Refused) => Ok(Err("refused")),
        Err(RemoteFailure::ConnectionLost | RemoteFailure::Protocol) => Err(()),
    }
}

async fn run_op<St>(
    store: &St,
    remote: &mut impl MutableMailbox,
    owner_id: &str,
    op: &PendingOp,
    folders: &[FolderRecord],
) -> Result<Step, MailboxError>
where
    St: MailStore + PendingOpStore,
{
    match &op.kind {
        OpKind::SetFlag { flag, on } => {
            match settle(remote.store_flag(op.imap_uid, *flag, *on).await) {
                Err(()) => Ok(Step::Halt),
                Ok(Err(code)) => reject(store, owner_id, op, code).await,
                Ok(Ok(())) => {
                    store.remove_op(owner_id, &op.id).await?;
                    Ok(Step::Done(OpResult::Confirmed))
                }
            }
        }
        OpKind::Move { .. } | OpKind::Expunge => {
            // A move is not idempotent. If an earlier attempt reached the server before the
            // connection dropped, the message is already gone and must not be acted on again.
            match settle(remote.uid_exists(op.imap_uid).await) {
                Err(()) => return Ok(Step::Halt),
                Ok(Err(code)) => return reject(store, owner_id, op, code).await,
                Ok(Ok(false)) => {
                    store.remove_message(owner_id, &op.message_id).await?;
                    return Ok(Step::Done(OpResult::Gone));
                }
                Ok(Ok(true)) => {}
            }
            let OpKind::Move { dest_folder_id } = &op.kind else {
                return match settle(remote.expunge_message(op.imap_uid).await) {
                    Err(()) => Ok(Step::Halt),
                    Ok(Err(code)) => reject(store, owner_id, op, code).await,
                    Ok(Ok(outcome)) => {
                        store.remove_message(owner_id, &op.message_id).await?;
                        Ok(Step::Done(match outcome {
                            Expunged::Removed => OpResult::Expunged,
                            Expunged::LeftDeleted => OpResult::LeftDeleted,
                        }))
                    }
                };
            };
            let Some(dest) = folders.iter().find(|f| &f.id == dest_folder_id) else {
                return reject(store, owner_id, op, "folder_missing").await;
            };
            match settle(remote.move_message(op.imap_uid, &dest.remote_name).await) {
                Err(()) => Ok(Step::Halt),
                Ok(Err(code)) => reject(store, owner_id, op, code).await,
                Ok(Ok(moved)) => {
                    let mut new_message_id = None;
                    if let Some((uid_validity, uid)) = moved.new_uid {
                        // Show the message in its new folder right away, under its new identity.
                        let mut copy = store.get_message(owner_id, &op.message_id).await?;
                        copy.id = format!("{}#{uid_validity}#{uid}", dest.id);
                        copy.folder_id = dest.id.clone();
                        copy.uid_validity = uid_validity;
                        copy.imap_uid = uid;
                        new_message_id = Some(copy.id.clone());
                        store.upsert_messages(owner_id, &[copy]).await?;
                    }
                    store.remove_message(owner_id, &op.message_id).await?;
                    Ok(Step::Done(OpResult::Moved {
                        new_message_id,
                        source_left_deleted: moved.source_left_deleted,
                    }))
                }
            }
        }
    }
}

/// Send the queued operations of an account to the server, oldest first.
pub async fn replay<St>(
    store: &St,
    remote: &mut impl MutableMailbox,
    owner_id: &str,
    account_id: &str,
) -> Result<ReplayReport, MailboxError>
where
    St: MailStore + PendingOpStore,
{
    let queued: Vec<PendingOp> = store
        .list_ops(owner_id, account_id)
        .await?
        .into_iter()
        .filter(|o| o.state == OpState::Queued)
        .collect();
    let folders = store.list_folders(owner_id, account_id).await?;
    let mut report = ReplayReport::default();
    let mut selected: Option<(String, Selected)> = None;
    let mut dropped_folders: BTreeSet<String> = BTreeSet::new();

    for (index, op) in queued.iter().enumerate() {
        if dropped_folders.contains(&op.folder_id) {
            report.done.push(OpReport {
                op: op.clone(),
                result: OpResult::DroppedEpochChanged,
            });
            continue;
        }
        // An earlier operation may have removed this message (and with it this operation).
        match store.get_message(owner_id, &op.message_id).await {
            Ok(_) => {}
            Err(MailboxError::NotFound) => continue,
            Err(e) => return Err(e),
        }
        let step = 'op: {
            let Some(folder) = folders.iter().find(|f| f.id == op.folder_id) else {
                break 'op reject(store, owner_id, op, "folder_missing").await?;
            };
            // Re-select for every folder change, and re-check the epoch before every mutation.
            let status = match &selected {
                Some((id, status)) if id == &folder.id => *status,
                _ => match settle(remote.select(&folder.remote_name).await) {
                    Err(()) => break 'op Step::Halt,
                    Ok(Err(_)) => {
                        selected = None;
                        break 'op reject(store, owner_id, op, "folder_missing").await?;
                    }
                    Ok(Ok(status)) => {
                        selected = Some((folder.id.clone(), status));
                        status
                    }
                },
            };
            if status.uid_validity != Some(op.uid_validity) {
                // The UID belongs to a previous life of this folder. Discard what we cached
                // about it; the next sync rebuilds it.
                store.reset_folder(owner_id, account_id, &folder.id).await?;
                dropped_folders.insert(folder.id.clone());
                break 'op Step::Done(OpResult::DroppedEpochChanged);
            }
            if status.read_only {
                break 'op reject(store, owner_id, op, "read_only").await?;
            }
            run_op(store, remote, owner_id, op, &folders).await?
        };
        match step {
            Step::Done(result) => report.done.push(OpReport {
                op: op.clone(),
                result,
            }),
            Step::Halt => {
                report.interrupted = true;
                report.still_queued = queued.len() - index;
                break;
            }
        }
    }
    Ok(report)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FlagPull {
    /// Messages whose cached flags were updated.
    pub changed: usize,
    /// Cached messages that no longer exist on the server and were dropped.
    pub vanished: usize,
}

/// Bring the cached flags of one folder in line with the server and drop messages that were
/// deleted or moved elsewhere. The folder must be open on `remote`, and `uid_validity` must be
/// the epoch the server just reported for it.
pub async fn pull_flags(
    store: &impl PendingOpStore,
    remote: &mut impl FlagSource,
    owner_id: &str,
    folder: &FolderRecord,
    uid_validity: u64,
) -> Result<FlagPull, MailboxError> {
    let cached: Vec<MessageRecord> = store
        .folder_messages(owner_id, &folder.account_id, &folder.id)
        .await?
        .into_iter()
        .filter(|m| m.uid_validity == uid_validity)
        .collect();
    let (Some(first), Some(last)) = (
        cached.iter().map(|m| m.imap_uid).min(),
        cached.iter().map(|m| m.imap_uid).max(),
    ) else {
        return Ok(FlagPull::default());
    };
    let remote_flags = remote.fetch_flags(first, last).await?;
    let waiting: Vec<PendingOp> = store
        .list_ops(owner_id, &folder.account_id)
        .await?
        .into_iter()
        .filter(|o| o.state == OpState::Queued)
        .collect();
    let is_waiting = |message_id: &str, flag: MessageFlag| {
        waiting.iter().any(|o| {
            o.message_id == message_id
                && matches!(&o.kind, OpKind::SetFlag { flag: f, .. } if *f == flag)
        })
    };
    let mut pull = FlagPull::default();
    for m in &cached {
        let Some(server) = remote_flags.iter().find(|r| r.uid == m.imap_uid) else {
            store.remove_message(owner_id, &m.id).await?;
            pull.vanished += 1;
            continue;
        };
        let seen =
            (!is_waiting(&m.id, MessageFlag::Seen) && server.seen != m.seen).then_some(server.seen);
        let flagged = (!is_waiting(&m.id, MessageFlag::Flagged) && server.flagged != m.flagged)
            .then_some(server.flagged);
        if seen.is_some() || flagged.is_some() {
            store
                .set_local_flags(owner_id, &m.id, seen, flagged)
                .await?;
            pull.changed += 1;
        }
    }
    Ok(pull)
}
