//! Server-backed message actions for real accounts on Desktop (MB-132 flags, MB-144 archive /
//! trash / move, MB-122/143 folder list). The types are shared with the Web build so the UI
//! compiles everywhere; the functions exist on native targets only.
//!
//! Every action is two steps: `queue_*` changes the local cache and records a pending operation
//! (fast, no network), `flush` sends what is queued. One gate serialises all server work, so an
//! undo never races the operation it undoes.
// The Web build has the types but no caller that constructs them yet.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]
use super::connect::CheckFailure;
use crate::i18n::tr;

/// A real folder as the sidebar shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealFolder {
    /// What `MessageSummary::folder_id` holds for this folder's mail: the role key for role
    /// folders (`inbox`, `sent`, ...), the folder id for the user's own folders.
    pub key: String,
    pub folder_id: String,
    pub account_id: String,
    /// Decoded leaf name for display.
    pub label: String,
    pub role: Option<String>,
}

/// State of a message's last change, shown on its row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowOp {
    /// Changed here, not yet confirmed by the mail server.
    Pending,
    /// The mail server refused; the change was undone locally.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    Archive,
    Trash,
    /// A folder id.
    Folder(String),
}

/// Why an action could not even be queued.
// Constructed on native targets only.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    NotFound,
    NoArchiveFolder,
    NoTrashFolder,
    SameFolder,
    NotInTrash,
    CannotUndo,
    Storage,
}

impl Problem {
    /// What happened and what the user can do. Never pretends the action took place.
    pub fn message(self) -> &'static str {
        match self {
            Problem::NotFound => {
                tr("Không tìm thấy thư này trên máy. Hãy đồng bộ lại rồi thử lại.")
            }
            Problem::NoArchiveFolder => tr(
                "Tài khoản này không có thư mục Lưu trữ trên máy chủ. MailBox không tự tạo thư mục; hãy dùng \"Chuyển đến…\" để chọn thư mục khác.",
            ),
            Problem::NoTrashFolder => tr(
                "Tài khoản này không có thư mục Thùng rác trên máy chủ nên chưa thể xóa thư từ MailBox.",
            ),
            Problem::SameFolder => tr("Thư đã ở trong thư mục này."),
            Problem::NotInTrash => tr("Chỉ xóa vĩnh viễn được thư đang nằm trong Thùng rác."),
            Problem::CannotUndo => tr(
                "Không hoàn tác được: không xác định được thư ở thư mục mới. Hãy mở thư mục đó và chuyển thư lại bằng tay.",
            ),
            Problem::Storage => tr("Không ghi được dữ liệu trên máy này. Thử lại sau."),
        }
    }
}

/// What happened to the queued operations of one account.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Flush {
    pub confirmed: usize,
    /// Refused by the server and rolled back locally.
    pub rejected: usize,
    /// Moved or deleted, but the original is only flagged deleted (server has no UIDPLUS).
    pub left_deleted: usize,
    /// Dropped because the folder was rebuilt on the server (UIDVALIDITY changed).
    pub dropped: usize,
    /// Still queued for the next connection.
    pub waiting: usize,
    /// Why the server could not be reached, when it could not.
    pub offline: Option<CheckFailure>,
}

impl Flush {
    /// The one thing worth telling the user, most serious first. `None` when all went through.
    pub fn message(&self) -> Option<&'static str> {
        if self.rejected > 0 {
            Some(tr(
                "Máy chủ thư từ chối thay đổi. MailBox đã hoàn tác trên máy này.",
            ))
        } else if self.dropped > 0 {
            Some(tr(
                "Thư mục đã được tạo lại trên máy chủ nên thay đổi bị hủy. MailBox sẽ tải lại thư mục ở lần đồng bộ tới.",
            ))
        } else if self.offline.is_some() || self.waiting > 0 {
            Some(tr(
                "Chưa gửi được thay đổi lên máy chủ thư. MailBox giữ lại và sẽ gửi ở lần đồng bộ tới.",
            ))
        } else if self.left_deleted > 0 {
            Some(tr(
                "Máy chủ không hỗ trợ gỡ từng thư (UIDPLUS): thư gốc mới được đánh dấu đã xóa, chưa bị gỡ hẳn.",
            ))
        } else {
            None
        }
    }
}

// Constructed on native targets only.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedOp {
    pub op_id: String,
    pub account_id: String,
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use crate::adapters::account::{OWNER, WINDOW, failure, open_store};
    use mailbox_core::actions::{
        ActionError, MessageFlag, MoveTarget, OpKind, OpResult, OpState, PendingOpStore, Queued,
        ReplayReport, cancel_op, dismiss_failed, replay,
    };
    use mailbox_core::folders::{FolderRole, role_of};
    use mailbox_core::secrets::SecretStore;
    use mailbox_core::store::{FolderRecord, MailStore};
    use mailbox_core::sync::sync_folder;
    use mailbox_core::{MailServerEndpoint, MailboxError, TlsMode};
    use mailbox_imap::mutate::MutableSession;
    use mailbox_secrets::KeychainStore;
    use mailbox_store::SqliteStore;
    use std::collections::{BTreeMap, BTreeSet};

    static GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// Where a queued move went, kept for the undo toast of this session.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct MoveTrace {
        source_folder_id: String,
        dest_folder_id: String,
        /// `Message-ID` header of the moved message, to find it again without COPYUID.
        header: Option<String>,
        done: bool,
        new_message_id: Option<String>,
    }

    static MOVES: std::sync::Mutex<BTreeMap<String, MoveTrace>> =
        std::sync::Mutex::new(BTreeMap::new());
    const MAX_TRACES: usize = 64;

    /// Held while talking to a mail server on behalf of pending operations or a sync.
    pub async fn gate() -> tokio::sync::MutexGuard<'static, ()> {
        GATE.lock().await
    }

    /// Record the outcome of replayed moves so they can be undone.
    pub fn remember(report: &ReplayReport) {
        let Ok(mut moves) = MOVES.lock() else { return };
        for done in &report.done {
            let Some(trace) = moves.get_mut(&done.op.id) else {
                continue;
            };
            match &done.result {
                OpResult::Moved { new_message_id, .. } => {
                    trace.done = true;
                    trace.new_message_id = new_message_id.clone();
                }
                _ => {
                    moves.remove(&done.op.id);
                }
            }
        }
    }

    pub fn view_key(folder: &FolderRecord) -> String {
        role_of(folder).map_or_else(|| folder.id.clone(), |r| r.key().to_string())
    }

    fn leaf(folder: &FolderRecord) -> String {
        let name = mailbox_imap::parse::decode_mutf7(&folder.remote_name);
        match folder.delimiter.as_deref().filter(|d| !d.is_empty()) {
            Some(d) => name.rsplit(d).next().unwrap_or(&name).to_string(),
            None => name,
        }
    }

    pub async fn folders_of(store: &impl MailStore) -> Result<Vec<RealFolder>, MailboxError> {
        let mut out = Vec::new();
        for account in store.list_accounts(OWNER).await? {
            for f in store.list_folders(OWNER, &account.id).await? {
                out.push(RealFolder {
                    key: view_key(&f),
                    label: leaf(&f),
                    role: role_of(&f).map(|r| r.key().to_string()),
                    folder_id: f.id,
                    account_id: account.id.clone(),
                });
            }
        }
        Ok(out)
    }

    /// Row states and the ids of messages that are on their way out of their folder.
    pub async fn op_view<St: MailStore + PendingOpStore>(
        store: &St,
    ) -> Result<(BTreeMap<String, RowOp>, BTreeSet<String>), MailboxError> {
        let (mut rows, mut hidden) = (BTreeMap::new(), BTreeSet::new());
        for account in store.list_accounts(OWNER).await? {
            for op in store.list_ops(OWNER, &account.id).await? {
                match (&op.state, &op.kind) {
                    (OpState::Failed(_), _) => {
                        rows.insert(op.message_id, RowOp::Failed);
                    }
                    (OpState::Queued, OpKind::Move { .. } | OpKind::Expunge) => {
                        hidden.insert(op.message_id);
                    }
                    (OpState::Queued, OpKind::SetFlag { .. }) => {
                        rows.entry(op.message_id).or_insert(RowOp::Pending);
                    }
                }
            }
        }
        Ok((rows, hidden))
    }

    pub fn summarise(report: &ReplayReport) -> Flush {
        let mut out = Flush {
            waiting: report.still_queued,
            offline: report.interrupted.then_some(CheckFailure::Unreachable),
            ..Flush::default()
        };
        for done in &report.done {
            match &done.result {
                OpResult::Confirmed | OpResult::Expunged | OpResult::Gone => out.confirmed += 1,
                OpResult::Moved {
                    source_left_deleted,
                    ..
                } => {
                    out.confirmed += 1;
                    out.left_deleted += usize::from(*source_left_deleted);
                }
                OpResult::LeftDeleted => {
                    out.confirmed += 1;
                    out.left_deleted += 1;
                }
                OpResult::Rejected(_) => out.rejected += 1,
                OpResult::DroppedEpochChanged => out.dropped += 1,
            }
        }
        out
    }

    fn problem(e: ActionError) -> Problem {
        match e {
            ActionError::NotFound => Problem::NotFound,
            ActionError::NoFolderForRole(FolderRole::Archive) => Problem::NoArchiveFolder,
            ActionError::NoFolderForRole(_) => Problem::NoTrashFolder,
            ActionError::SameFolder => Problem::SameFolder,
            ActionError::NotInTrash => Problem::NotInTrash,
            ActionError::Store(_) => Problem::Storage,
        }
    }

    async fn store() -> Result<SqliteStore, Problem> {
        open_store().await.map_err(|_| Problem::Storage)
    }

    pub async fn real_folders() -> Vec<RealFolder> {
        match open_store().await {
            Ok(store) => folders_of(&store).await.unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    pub async fn row_ops() -> BTreeMap<String, RowOp> {
        match open_store().await {
            Ok(store) => op_view(&store).await.map(|v| v.0).unwrap_or_default(),
            Err(_) => BTreeMap::new(),
        }
    }

    /// Queue a `\Seen` / `\Flagged` change. Returns the account to flush, or `None` when there
    /// is nothing to send.
    pub async fn queue_flag(
        message_id: &str,
        flag: MessageFlag,
        on: bool,
    ) -> Result<Option<String>, Problem> {
        let store = store().await?;
        match mailbox_core::actions::queue_flag(&store, OWNER, message_id, flag, on)
            .await
            .map_err(problem)?
        {
            Queued::Op(op) => Ok(Some(op.account_id)),
            Queued::NoChange | Queued::Cancelled => Ok(None),
        }
    }

    pub async fn queue_move(message_id: &str, dest: Destination) -> Result<QueuedOp, Problem> {
        let store = store().await?;
        let header = store
            .get_message(OWNER, message_id)
            .await
            .map_err(|_| Problem::NotFound)?
            .message_id_header;
        let target = match dest {
            Destination::Archive => MoveTarget::Role(FolderRole::Archive),
            Destination::Trash => MoveTarget::Role(FolderRole::Trash),
            Destination::Folder(id) => MoveTarget::Folder(id),
        };
        let op = mailbox_core::actions::queue_move(&store, OWNER, message_id, target)
            .await
            .map_err(problem)?;
        if let (Ok(mut moves), OpKind::Move { dest_folder_id }) = (MOVES.lock(), &op.kind) {
            while moves.len() >= MAX_TRACES {
                moves.pop_first();
            }
            moves.insert(
                op.id.clone(),
                MoveTrace {
                    source_folder_id: op.folder_id.clone(),
                    dest_folder_id: dest_folder_id.clone(),
                    header,
                    done: false,
                    new_message_id: None,
                },
            );
        }
        Ok(QueuedOp {
            op_id: op.id,
            account_id: op.account_id,
        })
    }

    /// Queue a permanent delete. Only accepted for mail in the trash folder.
    pub async fn queue_delete_forever(message_id: &str) -> Result<QueuedOp, Problem> {
        let store = store().await?;
        let op = mailbox_core::actions::queue_expunge(&store, OWNER, message_id)
            .await
            .map_err(problem)?;
        Ok(QueuedOp {
            op_id: op.id,
            account_id: op.account_id,
        })
    }

    async fn flush_locked(store: &SqliteStore, account_id: &str) -> Flush {
        let queued = match store.list_ops(OWNER, account_id).await {
            Ok(ops) => ops.iter().filter(|o| o.state == OpState::Queued).count(),
            Err(_) => 0,
        };
        if queued == 0 {
            return Flush::default();
        }
        let waiting = |why: CheckFailure| Flush {
            waiting: queued,
            offline: Some(why),
            ..Flush::default()
        };
        // MB-140: with a live worker the account's open connection is borrowed (no new TLS
        // handshake). While the worker is offline the operations stay queued: it replays them
        // itself as soon as the connection is back.
        if let Some(live) = crate::adapters::worker::handle(account_id) {
            let Some(mut rw) = live.session().await else {
                return waiting(crate::adapters::worker::offline_reason(live.state().state));
            };
            let flush = flush_on(store, &mut rw, account_id).await;
            if flush.offline == Some(CheckFailure::Unreachable) {
                // The borrowed connection broke: never reuse it.
                rw.discard();
            }
            return flush;
        }
        let Ok(account) = store.get_account(OWNER, account_id).await else {
            return waiting(CheckFailure::Storage);
        };
        let Ok(password) = KeychainStore::default().get(&account.credential_ref) else {
            return waiting(CheckFailure::AuthFailed);
        };
        let endpoint = MailServerEndpoint {
            hostname: account.imap_host.clone(),
            port: account.imap_port,
            tls_mode: TlsMode::ImplicitTls,
        };
        let mut rw =
            match MutableSession::connect(&endpoint, &account.email_address, &password).await {
                Ok(rw) => rw,
                Err(e) => return waiting(failure(e.into())),
            };
        let flush = flush_on(store, &mut rw, account_id).await;
        rw.into_reader().logout().await;
        flush
    }

    /// Replay the queued operations of an account over an open read-write session.
    async fn flush_on<S>(store: &SqliteStore, rw: &mut MutableSession<S>, account_id: &str) -> Flush
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let report = replay(store, rw, OWNER, account_id).await;
        let Ok(report) = report else {
            return Flush {
                offline: Some(CheckFailure::Storage),
                ..Flush::default()
            };
        };
        remember(&report);
        // Where the server did not say which UID the moved message got, read the destination
        // again so the message shows up there (and can be found for an undo).
        let unknown: BTreeSet<&str> = report
            .done
            .iter()
            .filter_map(|d| match (&d.op.kind, &d.result) {
                (
                    OpKind::Move { dest_folder_id },
                    OpResult::Moved {
                        new_message_id: None,
                        ..
                    },
                ) => Some(dest_folder_id.as_str()),
                _ => None,
            })
            .collect();
        if !unknown.is_empty()
            && !report.interrupted
            && let Ok(folders) = store.list_folders(OWNER, account_id).await
        {
            for folder in folders.iter().filter(|f| unknown.contains(f.id.as_str())) {
                let _ = sync_folder(store, rw.reader_mut(), OWNER, folder, WINDOW).await;
            }
        }
        summarise(&report)
    }

    /// Send the queued operations of an account to its mail server.
    pub async fn flush(account_id: &str) -> Flush {
        let Ok(store) = open_store().await else {
            return Flush {
                offline: Some(CheckFailure::Storage),
                ..Flush::default()
            };
        };
        let _gate = gate().await;
        flush_locked(&store, account_id).await
    }

    /// Undo a move: take it back if it has not been sent, otherwise move the message back from
    /// where it went.
    pub async fn undo(op_id: &str, account_id: &str) -> Result<Flush, Problem> {
        let store = store().await?;
        let _gate = gate().await;
        if cancel_op(&store, OWNER, account_id, op_id)
            .await
            .map_err(|_| Problem::Storage)?
        {
            if let Ok(mut moves) = MOVES.lock() {
                moves.remove(op_id);
            }
            return Ok(Flush::default());
        }
        let trace = MOVES
            .lock()
            .ok()
            .and_then(|mut moves| moves.remove(op_id))
            .filter(|t| t.done)
            .ok_or(Problem::CannotUndo)?;
        let mut current = None;
        if let Some(id) = &trace.new_message_id
            && store.get_message(OWNER, id).await.is_ok()
        {
            current = Some(id.clone());
        }
        if current.is_none()
            && let Some(header) = &trace.header
        {
            current = store
                .folder_messages(OWNER, account_id, &trace.dest_folder_id)
                .await
                .map_err(|_| Problem::Storage)?
                .into_iter()
                .filter(|m| m.message_id_header.as_ref() == Some(header))
                .max_by_key(|m| m.imap_uid)
                .map(|m| m.id);
        }
        let current = current.ok_or(Problem::CannotUndo)?;
        mailbox_core::actions::queue_move(
            &store,
            OWNER,
            &current,
            MoveTarget::Folder(trace.source_folder_id),
        )
        .await
        .map_err(problem)?;
        Ok(flush_locked(&store, account_id).await)
    }

    /// The user has seen a failed change on this message.
    pub async fn dismiss(message_id: &str) {
        let Ok(store) = open_store().await else {
            return;
        };
        if let Ok(m) = store.get_message(OWNER, message_id).await {
            let _ = dismiss_failed(&store, OWNER, &m.account_id, message_id).await;
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use mailbox_core::actions::{NewOp, OpReport, PendingOp};
        use mailbox_core::store::{AccountRecord, MessageRecord};

        fn rt() -> tokio::runtime::Runtime {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
        }

        async fn seeded() -> SqliteStore {
            let store = SqliteStore::in_memory().await.unwrap();
            store
                .upsert_account(&AccountRecord {
                    id: "acc-t".into(),
                    owner_id: OWNER.into(),
                    display_name: "example.com".into(),
                    email_address: "t@example.com".into(),
                    imap_host: "imap.example.com".into(),
                    imap_port: 993,
                    smtp_host: "smtp.example.com".into(),
                    smtp_port: 465,
                    auth_type: "password".into(),
                    credential_ref: "imap.acc-t".into(),
                    status: "active".into(),
                })
                .await
                .unwrap();
            for (name, role) in [
                ("INBOX", Some("inbox")),
                ("INBOX.Trash", Some("trash")),
                ("INBOX.D&AbA- &AOE-n.Kh&AOE-ch", None),
            ] {
                store
                    .upsert_folder(
                        OWNER,
                        &FolderRecord {
                            id: format!("acc-t/{name}"),
                            account_id: "acc-t".into(),
                            remote_name: name.into(),
                            delimiter: Some(".".into()),
                            special_use: role.map(str::to_string),
                            uid_validity: Some(1),
                            uid_next: None,
                        },
                    )
                    .await
                    .unwrap();
            }
            let rows: Vec<MessageRecord> = (1..=3)
                .map(|uid| MessageRecord {
                    id: format!("m{uid}"),
                    account_id: "acc-t".into(),
                    folder_id: "acc-t/INBOX".into(),
                    uid_validity: 1,
                    imap_uid: uid,
                    message_id_header: None,
                    subject: format!("S{uid}"),
                    from_display: String::new(),
                    from_address: "a@example.org".into(),
                    preview: String::new(),
                    internal_date: i64::from(uid),
                    size_bytes: 1,
                    seen: false,
                    flagged: false,
                })
                .collect();
            store.upsert_messages(OWNER, &rows).await.unwrap();
            store
        }

        #[test]
        fn folders_get_role_keys_or_ids_and_readable_leaf_names() {
            rt().block_on(async {
                let store = seeded().await;
                let folders = folders_of(&store).await.unwrap();
                let got: Vec<_> = folders
                    .iter()
                    .map(|f| (f.key.as_str(), f.label.as_str(), f.role.as_deref()))
                    .collect();
                assert_eq!(
                    got,
                    vec![
                        ("inbox", "INBOX", Some("inbox")),
                        ("acc-t/INBOX.D&AbA- &AOE-n.Kh&AOE-ch", "Khách", None),
                        ("trash", "Trash", Some("trash")),
                    ]
                );
            });
        }

        #[test]
        fn queued_flags_show_as_pending_failed_ones_as_failed_and_moves_hide_the_row() {
            rt().block_on(async {
                let store = seeded().await;
                mailbox_core::actions::queue_flag(&store, OWNER, "m1", MessageFlag::Flagged, true)
                    .await
                    .unwrap();
                mailbox_core::actions::queue_move(
                    &store,
                    OWNER,
                    "m2",
                    MoveTarget::Role(FolderRole::Trash),
                )
                .await
                .unwrap();
                let mut failed = store
                    .enqueue_op(
                        OWNER,
                        &NewOp {
                            account_id: "acc-t".into(),
                            message_id: "m3".into(),
                            folder_id: "acc-t/INBOX".into(),
                            uid_validity: 1,
                            imap_uid: 3,
                            kind: OpKind::SetFlag {
                                flag: MessageFlag::Seen,
                                on: true,
                            },
                            previous: Some(false),
                        },
                    )
                    .await
                    .unwrap();
                failed.state = OpState::Failed("refused".into());
                store.update_op(OWNER, &failed).await.unwrap();

                let (rows, hidden) = op_view(&store).await.unwrap();
                assert_eq!(rows.get("m1"), Some(&RowOp::Pending));
                assert_eq!(rows.get("m3"), Some(&RowOp::Failed));
                assert!(!rows.contains_key("m2"));
                assert_eq!(hidden.into_iter().collect::<Vec<_>>(), vec!["m2"]);
            });
        }

        fn report(result: OpResult) -> OpReport {
            OpReport {
                op: PendingOp {
                    id: "op".into(),
                    seq: 1,
                    account_id: "a".into(),
                    message_id: "m".into(),
                    folder_id: "f".into(),
                    uid_validity: 1,
                    imap_uid: 1,
                    kind: OpKind::Expunge,
                    previous: None,
                    state: OpState::Queued,
                },
                result,
            }
        }

        #[test]
        fn the_summary_names_the_most_serious_outcome_and_is_silent_on_success() {
            let all_good = summarise(&ReplayReport {
                done: vec![report(OpResult::Confirmed), report(OpResult::Gone)],
                still_queued: 0,
                interrupted: false,
            });
            assert_eq!((all_good.confirmed, all_good.message()), (2, None));

            let rejected = summarise(&ReplayReport {
                done: vec![
                    report(OpResult::Rejected("refused".into())),
                    report(OpResult::LeftDeleted),
                ],
                still_queued: 0,
                interrupted: false,
            });
            assert_eq!((rejected.rejected, rejected.left_deleted), (1, 1));
            assert!(rejected.message().unwrap().contains("từ chối"));

            let offline = summarise(&ReplayReport {
                done: Vec::new(),
                still_queued: 2,
                interrupted: true,
            });
            assert_eq!(offline.waiting, 2);
            assert!(offline.message().unwrap().contains("lần đồng bộ tới"));

            let left = summarise(&ReplayReport {
                done: vec![report(OpResult::Moved {
                    new_message_id: None,
                    source_left_deleted: true,
                })],
                still_queued: 0,
                interrupted: false,
            });
            assert!(left.message().unwrap().contains("UIDPLUS"));
        }

        #[test]
        fn problems_say_what_is_missing_without_claiming_success() {
            assert!(
                Problem::NoArchiveFolder
                    .message()
                    .contains("không có thư mục Lưu trữ")
            );
            assert!(Problem::NoArchiveFolder.message().contains("không tự tạo"));
            for p in [
                Problem::NotFound,
                Problem::NoTrashFolder,
                Problem::SameFolder,
                Problem::NotInTrash,
                Problem::CannotUndo,
                Problem::Storage,
            ] {
                assert!(!p.message().starts_with("Đã "), "{p:?}");
            }
        }
    }
}
