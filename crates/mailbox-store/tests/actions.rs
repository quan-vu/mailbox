//! Use cases for folders, flags and moves (MB-122/143/132/144) against the real SQLite store
//! and a fake mail server implementing the core ports.
use mailbox_core::MailboxError;
use mailbox_core::actions::{
    ActionError, Expunged, FlagSource, MessageFlag, MoveTarget, Moved, MutableMailbox, OpKind,
    OpResult, OpState, PendingOpStore, Queued, RemoteFailure, RemoteFlags, Selected, cancel_op,
    pull_flags, queue_expunge, queue_flag, queue_move, replay,
};
use mailbox_core::folders::{
    FolderRole, FolderRoleStore, FolderSource, ListedFolder, RoleSource, discover_with_roles,
    set_user_role, sync_all_folders,
};
use mailbox_core::store::{AccountRecord, MailStore};
use mailbox_core::sync::{RemoteEnvelope, RemoteFolder, RemoteMailbox, RemoteStatus, SyncKind};
use mailbox_store::SqliteStore;
use std::collections::BTreeMap;

const OWNER: &str = "local";
const ACC: &str = "acc";

#[derive(Debug, Clone)]
struct Msg {
    uid: u32,
    seen: bool,
    flagged: bool,
    deleted: bool,
}

#[derive(Debug, Clone)]
struct Box_ {
    attributes: Vec<String>,
    uid_validity: u64,
    next_uid: u32,
    msgs: Vec<Msg>,
    broken: bool,
}

/// A small in-memory mail server. `calls` records every mutation actually applied.
struct Fake {
    boxes: BTreeMap<String, Box_>,
    selected: Option<String>,
    copyuid: bool,
    uidplus: bool,
    /// Fail the n-th mutating call (1-based) with this failure, once.
    fail_at: Option<(usize, RemoteFailure)>,
    mutations: usize,
    calls: Vec<String>,
}

impl Fake {
    fn new() -> Self {
        let mut boxes = BTreeMap::new();
        for (name, attrs, uids) in [
            ("INBOX", vec![], vec![1u32, 2, 3]),
            ("INBOX.Sent", vec!["\\Sent"], vec![1]),
            ("INBOX.Trash", vec![], vec![]),
            ("INBOX.Lưu trữ", vec![], vec![]),
            ("INBOX.Projects", vec![], vec![7, 8]),
        ] {
            boxes.insert(
                name.to_string(),
                Box_ {
                    attributes: attrs.into_iter().map(String::from).collect(),
                    uid_validity: 10,
                    next_uid: uids.iter().max().copied().unwrap_or(0) + 1,
                    msgs: uids
                        .into_iter()
                        .map(|uid| Msg {
                            uid,
                            seen: false,
                            flagged: false,
                            deleted: false,
                        })
                        .collect(),
                    broken: false,
                },
            );
        }
        Self {
            boxes,
            selected: None,
            copyuid: true,
            uidplus: true,
            fail_at: None,
            mutations: 0,
            calls: Vec::new(),
        }
    }

    fn current(&mut self) -> &mut Box_ {
        let name = self.selected.clone().expect("a folder is selected");
        self.boxes.get_mut(&name).unwrap()
    }

    fn mutation(&mut self) -> Result<(), RemoteFailure> {
        self.mutations += 1;
        match self.fail_at {
            Some((n, failure)) if n == self.mutations => {
                self.fail_at = None;
                Err(failure)
            }
            _ => Ok(()),
        }
    }

    fn msg(&self, folder: &str, uid: u32) -> Option<&Msg> {
        self.boxes[folder].msgs.iter().find(|m| m.uid == uid)
    }
}

impl FolderSource for Fake {
    async fn list_detailed(&mut self) -> Result<Vec<ListedFolder>, MailboxError> {
        let mut out: Vec<ListedFolder> = self
            .boxes
            .iter()
            .map(|(name, b)| ListedFolder {
                name: name.clone(),
                display_name: name.clone(),
                delimiter: Some(".".into()),
                attributes: b.attributes.clone(),
                selectable: true,
            })
            .collect();
        out.push(ListedFolder {
            name: "INBOX.Parent".into(),
            display_name: "INBOX.Parent".into(),
            delimiter: Some(".".into()),
            attributes: vec!["\\Noselect".into()],
            selectable: false,
        });
        Ok(out)
    }
}

impl RemoteMailbox for Fake {
    async fn list_folders(&mut self) -> Result<Vec<RemoteFolder>, MailboxError> {
        unreachable!("multi-folder sync lists folders with attributes")
    }
    async fn examine(&mut self, folder: &str) -> Result<RemoteStatus, MailboxError> {
        let b = self
            .boxes
            .get(folder)
            .ok_or(MailboxError::InconsistentData("imap protocol"))?;
        if b.broken {
            return Err(MailboxError::InconsistentData("imap protocol"));
        }
        let status = RemoteStatus {
            exists: b.msgs.len() as u32,
            uid_validity: Some(b.uid_validity),
            uid_next: Some(u64::from(b.next_uid)),
        };
        self.selected = Some(folder.to_string());
        Ok(status)
    }
    async fn fetch_latest(
        &mut self,
        _exists: u32,
        count: u32,
    ) -> Result<Vec<RemoteEnvelope>, MailboxError> {
        let msgs = self.current().msgs.clone();
        let skip = msgs.len().saturating_sub(count as usize);
        Ok(msgs.iter().skip(skip).map(envelope).collect())
    }
    async fn fetch_from_uid(&mut self, from_uid: u32) -> Result<Vec<RemoteEnvelope>, MailboxError> {
        let msgs = self.current().msgs.clone();
        Ok(msgs
            .iter()
            .filter(|m| m.uid >= from_uid)
            .map(envelope)
            .collect())
    }
}

fn envelope(m: &Msg) -> RemoteEnvelope {
    RemoteEnvelope {
        uid: m.uid,
        seen: m.seen,
        flagged: m.flagged,
        internal_date: i64::from(m.uid) * 10,
        size_bytes: 10,
        subject: format!("Thư {}", m.uid),
        from_display: "A".into(),
        from_address: "a@example.com".into(),
        message_id: Some(format!("<{}@example.com>", m.uid)),
        ..Default::default()
    }
}

impl FlagSource for Fake {
    async fn fetch_flags(
        &mut self,
        first: u32,
        last: u32,
    ) -> Result<Vec<RemoteFlags>, MailboxError> {
        Ok(self
            .current()
            .msgs
            .iter()
            .filter(|m| m.uid >= first && m.uid <= last)
            .map(|m| RemoteFlags {
                uid: m.uid,
                seen: m.seen,
                flagged: m.flagged,
            })
            .collect())
    }
}

impl MutableMailbox for Fake {
    async fn select(&mut self, folder: &str) -> Result<Selected, RemoteFailure> {
        let b = self.boxes.get(folder).ok_or(RemoteFailure::Refused)?;
        let selected = Selected {
            uid_validity: Some(b.uid_validity),
            read_only: false,
        };
        self.selected = Some(folder.to_string());
        Ok(selected)
    }
    async fn uid_exists(&mut self, uid: u32) -> Result<bool, RemoteFailure> {
        Ok(self.current().msgs.iter().any(|m| m.uid == uid))
    }
    async fn store_flag(
        &mut self,
        uid: u32,
        flag: MessageFlag,
        on: bool,
    ) -> Result<(), RemoteFailure> {
        self.mutation()?;
        let folder = self.selected.clone().unwrap();
        self.calls
            .push(format!("store {folder} {uid} {flag:?} {on}"));
        if let Some(m) = self.current().msgs.iter_mut().find(|m| m.uid == uid) {
            match flag {
                MessageFlag::Seen => m.seen = on,
                MessageFlag::Flagged => m.flagged = on,
            }
        }
        Ok(())
    }
    async fn move_message(&mut self, uid: u32, dest: &str) -> Result<Moved, RemoteFailure> {
        self.mutation()?;
        let folder = self.selected.clone().unwrap();
        if !self.boxes.contains_key(dest) {
            return Err(RemoteFailure::Refused);
        }
        let uidplus = self.uidplus;
        let src = self.boxes.get_mut(&folder).unwrap();
        let Some(pos) = src.msgs.iter().position(|m| m.uid == uid) else {
            return Ok(Moved {
                new_uid: None,
                source_left_deleted: false,
            });
        };
        let mut m = if uidplus {
            src.msgs.remove(pos)
        } else {
            src.msgs[pos].deleted = true;
            src.msgs[pos].clone()
        };
        let d = self.boxes.get_mut(dest).unwrap();
        m.uid = d.next_uid;
        m.deleted = false;
        d.next_uid += 1;
        let new_uid = (d.uid_validity, m.uid);
        d.msgs.push(m);
        self.calls.push(format!("move {folder} {uid} -> {dest}"));
        Ok(Moved {
            new_uid: self.copyuid.then_some(new_uid),
            source_left_deleted: !uidplus,
        })
    }
    async fn expunge_message(&mut self, uid: u32) -> Result<Expunged, RemoteFailure> {
        self.mutation()?;
        let folder = self.selected.clone().unwrap();
        self.calls.push(format!("expunge {folder} {uid}"));
        let uidplus = self.uidplus;
        let b = self.current();
        if uidplus {
            b.msgs.retain(|m| m.uid != uid);
            Ok(Expunged::Removed)
        } else {
            if let Some(m) = b.msgs.iter_mut().find(|m| m.uid == uid) {
                m.deleted = true;
            }
            Ok(Expunged::LeftDeleted)
        }
    }
}

async fn setup() -> (SqliteStore, Fake) {
    let store = SqliteStore::in_memory().await.unwrap();
    store
        .upsert_account(&AccountRecord {
            id: ACC.into(),
            owner_id: OWNER.into(),
            display_name: "Test".into(),
            email_address: "t@example.com".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 465,
            auth_type: "password".into(),
            credential_ref: "imap.acc".into(),
            status: "active".into(),
        })
        .await
        .unwrap();
    (store, Fake::new())
}

/// Store + server after a first full sync.
async fn synced() -> (SqliteStore, Fake) {
    let (store, mut fake) = setup().await;
    let report = sync_all_folders(&store, &mut fake, OWNER, ACC, 50, 64)
        .await
        .unwrap();
    assert!(report.folders.iter().all(|f| f.result.is_ok()));
    (store, fake)
}

fn id(folder: &str, uid: u32) -> String {
    format!("{ACC}/{folder}#10#{uid}")
}

async fn local_uids(store: &SqliteStore, folder: &str) -> Vec<u32> {
    let mut v: Vec<u32> = store
        .folder_messages(OWNER, ACC, &format!("{ACC}/{folder}"))
        .await
        .unwrap()
        .iter()
        .map(|m| m.imap_uid)
        .collect();
    v.sort_unstable();
    v
}

// ---------- MB-122 ----------

#[tokio::test]
async fn discovery_persists_roles_from_attributes_then_names() {
    let (store, mut fake) = setup().await;
    let folders = discover_with_roles(&store, &mut fake, OWNER, ACC)
        .await
        .unwrap();
    let mut got: Vec<_> = folders
        .iter()
        .map(|f| (f.remote_name.as_str(), f.special_use.as_deref()))
        .collect();
    got.sort();
    assert_eq!(
        got,
        vec![
            ("INBOX", Some("inbox")),
            ("INBOX.Lưu trữ", Some("archive")),
            ("INBOX.Projects", None),
            ("INBOX.Sent", Some("sent")),
            ("INBOX.Trash", Some("trash")),
        ],
        "the non-selectable container is skipped"
    );
    let roles = store.list_folder_roles(OWNER, ACC).await.unwrap();
    let source = |folder: &str| {
        roles
            .iter()
            .find(|r| r.folder_id == format!("{ACC}/{folder}"))
            .map(|r| (r.role, r.source))
            .unwrap()
    };
    assert_eq!(
        source("INBOX.Sent"),
        (Some(FolderRole::Sent), RoleSource::Attribute)
    );
    assert_eq!(
        source("INBOX.Trash"),
        (Some(FolderRole::Trash), RoleSource::Name)
    );
    assert_eq!(source("INBOX.Projects"), (None, RoleSource::Name));
}

#[tokio::test]
async fn a_user_override_survives_rediscovery_and_takes_the_role_from_the_detected_folder() {
    let (store, mut fake) = setup().await;
    discover_with_roles(&store, &mut fake, OWNER, ACC)
        .await
        .unwrap();
    set_user_role(
        &store,
        OWNER,
        ACC,
        &format!("{ACC}/INBOX.Projects"),
        Some(FolderRole::Trash),
    )
    .await
    .unwrap();
    for _ in 0..2 {
        let folders = discover_with_roles(&store, &mut fake, OWNER, ACC)
            .await
            .unwrap();
        let role = |name: &str| {
            folders
                .iter()
                .find(|f| f.remote_name == name)
                .and_then(|f| f.special_use.clone())
        };
        assert_eq!(role("INBOX.Projects").as_deref(), Some("trash"));
        assert_eq!(role("INBOX.Trash"), None, "one trash per account");
        assert_eq!(role("INBOX.Sent").as_deref(), Some("sent"));
    }
}

#[tokio::test]
async fn folders_deleted_on_the_server_are_pruned_but_never_on_an_empty_listing() {
    let (store, mut fake) = synced().await;
    fake.boxes.remove("INBOX.Projects");
    discover_with_roles(&store, &mut fake, OWNER, ACC)
        .await
        .unwrap();
    let names: Vec<_> = store
        .list_folders(OWNER, ACC)
        .await
        .unwrap()
        .into_iter()
        .map(|f| f.remote_name)
        .collect();
    assert!(!names.contains(&"INBOX.Projects".to_string()));
    assert_eq!(names.len(), 4);

    // A listing without INBOX is not believable: nothing is touched.
    fake.boxes.clear();
    assert!(
        discover_with_roles(&store, &mut fake, OWNER, ACC)
            .await
            .is_err()
    );
    assert_eq!(store.list_folders(OWNER, ACC).await.unwrap().len(), 4);
}

// ---------- MB-143 ----------

#[tokio::test]
async fn every_selectable_folder_syncs_with_its_own_cursor() {
    let (store, mut fake) = setup().await;
    let report = sync_all_folders(&store, &mut fake, OWNER, ACC, 50, 64)
        .await
        .unwrap();
    assert_eq!(report.folders.len(), 5);
    assert_eq!(
        report.folders[0].role,
        Some(FolderRole::Inbox),
        "inbox first"
    );
    assert!(
        report
            .folders
            .iter()
            .all(|f| f.result.as_ref().unwrap().kind == SyncKind::Initial)
    );
    assert_eq!(local_uids(&store, "INBOX").await, vec![1, 2, 3]);
    assert_eq!(local_uids(&store, "INBOX.Projects").await, vec![7, 8]);
    assert_eq!(local_uids(&store, "INBOX.Sent").await, vec![1]);

    // Different epochs per folder are kept apart.
    fake.boxes.get_mut("INBOX.Projects").unwrap().uid_validity = 99;
    fake.boxes.get_mut("INBOX").unwrap().msgs.push(Msg {
        uid: 4,
        seen: false,
        flagged: false,
        deleted: false,
    });
    let report = sync_all_folders(&store, &mut fake, OWNER, ACC, 50, 64)
        .await
        .unwrap();
    let kind = |name: &str| {
        report
            .folders
            .iter()
            .find(|f| f.folder_id == format!("{ACC}/{name}"))
            .unwrap()
            .result
            .as_ref()
            .unwrap()
            .kind
    };
    assert_eq!(kind("INBOX"), SyncKind::Incremental);
    assert_eq!(kind("INBOX.Projects"), SyncKind::Rebuilt);
    assert_eq!(local_uids(&store, "INBOX").await, vec![1, 2, 3, 4]);
    let cursor = store
        .load_cursor(OWNER, ACC, &format!("{ACC}/INBOX.Projects"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cursor.uid_validity, 99);
}

#[tokio::test]
async fn one_failing_folder_does_not_abort_the_others_and_the_window_bounds_each() {
    let (store, mut fake) = setup().await;
    fake.boxes.get_mut("INBOX.Sent").unwrap().broken = true;
    let report = sync_all_folders(&store, &mut fake, OWNER, ACC, 2, 64)
        .await
        .unwrap();
    let failed: Vec<_> = report
        .folders
        .iter()
        .filter(|f| f.result.is_err())
        .map(|f| f.folder_id.as_str())
        .collect();
    assert_eq!(failed, vec!["acc/INBOX.Sent"]);
    assert_eq!(
        local_uids(&store, "INBOX").await,
        vec![2, 3],
        "newest 2 only"
    );
    assert_eq!(local_uids(&store, "INBOX.Projects").await, vec![7, 8]);

    // The folder cap is reported instead of silently dropping folders.
    let (store, mut fake) = setup().await;
    let report = sync_all_folders(&store, &mut fake, OWNER, ACC, 50, 2)
        .await
        .unwrap();
    assert_eq!((report.folders.len(), report.skipped), (2, 3));
}

// ---------- MB-132 ----------

#[tokio::test]
async fn a_flag_change_is_optimistic_queued_and_confirmed_once() {
    let (store, mut fake) = synced().await;
    let queued = queue_flag(&store, OWNER, &id("INBOX", 2), MessageFlag::Flagged, true)
        .await
        .unwrap();
    assert!(matches!(queued, Queued::Op(_)));
    assert!(
        store
            .get_message(OWNER, &id("INBOX", 2))
            .await
            .unwrap()
            .flagged
    );
    assert!(
        fake.calls.is_empty(),
        "nothing reaches the server before replay"
    );

    let report = replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    assert_eq!(report.done.len(), 1);
    assert_eq!(report.done[0].result, OpResult::Confirmed);
    assert!(!report.interrupted);
    assert!(fake.msg("INBOX", 2).unwrap().flagged);

    // No duplicate replay.
    let again = replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    assert!(again.done.is_empty());
    assert_eq!(fake.calls, vec!["store INBOX 2 Flagged true"]);

    // Same value again is not an operation.
    assert_eq!(
        queue_flag(&store, OWNER, &id("INBOX", 2), MessageFlag::Flagged, true)
            .await
            .unwrap(),
        Queued::NoChange
    );
}

#[tokio::test]
async fn toggling_back_before_replay_cancels_the_queued_operation() {
    let (store, mut fake) = synced().await;
    queue_flag(&store, OWNER, &id("INBOX", 1), MessageFlag::Seen, true)
        .await
        .unwrap();
    assert_eq!(
        queue_flag(&store, OWNER, &id("INBOX", 1), MessageFlag::Seen, false)
            .await
            .unwrap(),
        Queued::Cancelled
    );
    assert!(store.list_ops(OWNER, ACC).await.unwrap().is_empty());
    replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    assert!(fake.calls.is_empty());
}

#[tokio::test]
async fn a_refused_flag_change_is_rolled_back_and_kept_as_failed() {
    let (store, mut fake) = synced().await;
    queue_flag(&store, OWNER, &id("INBOX", 3), MessageFlag::Seen, true)
        .await
        .unwrap();
    fake.fail_at = Some((1, RemoteFailure::Refused));
    let report = replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    assert!(matches!(report.done[0].result, OpResult::Rejected(_)));
    assert!(
        !store
            .get_message(OWNER, &id("INBOX", 3))
            .await
            .unwrap()
            .seen,
        "local state went back to what the server has"
    );
    let ops = store.list_ops(OWNER, ACC).await.unwrap();
    assert!(
        matches!(ops[0].state, OpState::Failed(_)),
        "visible until dismissed"
    );
    // A failed operation is never retried on its own.
    replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    assert!(fake.calls.is_empty());
}

#[tokio::test]
async fn a_lost_connection_keeps_the_operation_queued_for_the_next_session() {
    let (store, mut fake) = synced().await;
    queue_flag(&store, OWNER, &id("INBOX", 1), MessageFlag::Seen, true)
        .await
        .unwrap();
    queue_flag(&store, OWNER, &id("INBOX", 2), MessageFlag::Flagged, true)
        .await
        .unwrap();
    fake.fail_at = Some((2, RemoteFailure::ConnectionLost));
    let report = replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    assert!(report.interrupted);
    assert_eq!((report.done.len(), report.still_queued), (1, 1));
    assert!(
        store
            .get_message(OWNER, &id("INBOX", 2))
            .await
            .unwrap()
            .flagged,
        "the optimistic state stays while the operation waits"
    );

    let report = replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    assert!(!report.interrupted);
    assert_eq!(report.done.len(), 1);
    assert_eq!(
        fake.calls,
        vec!["store INBOX 1 Seen true", "store INBOX 2 Flagged true"],
        "each operation reached the server exactly once"
    );
}

#[tokio::test]
async fn an_operation_queued_under_an_old_uidvalidity_is_dropped_and_the_folder_rebuilt() {
    let (store, mut fake) = synced().await;
    queue_flag(&store, OWNER, &id("INBOX", 2), MessageFlag::Flagged, true)
        .await
        .unwrap();
    queue_move(
        &store,
        OWNER,
        &id("INBOX", 3),
        MoveTarget::Role(FolderRole::Trash),
    )
    .await
    .unwrap();
    // The server recreated the folder while we were offline: UID 2 is now another message.
    fake.boxes.get_mut("INBOX").unwrap().uid_validity = 11;
    let report = replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    assert!(fake.calls.is_empty(), "no stale UID was ever sent");
    assert_eq!(report.done.len(), 2);
    assert!(
        report
            .done
            .iter()
            .all(|d| d.result == OpResult::DroppedEpochChanged)
    );
    assert!(store.list_ops(OWNER, ACC).await.unwrap().is_empty());
    assert!(
        local_uids(&store, "INBOX").await.is_empty(),
        "old epoch discarded"
    );

    let sync = sync_all_folders(&store, &mut fake, OWNER, ACC, 50, 64)
        .await
        .unwrap();
    assert!(sync.folders[0].result.is_ok());
    let rows = store
        .folder_messages(OWNER, ACC, "acc/INBOX")
        .await
        .unwrap();
    assert!(rows.iter().all(|m| m.uid_validity == 11 && !m.flagged));
}

#[tokio::test]
async fn server_flags_win_except_where_a_local_change_is_still_waiting() {
    let (store, mut fake) = synced().await;
    // Another client read message 1 and starred message 2; message 3 was deleted there.
    {
        let inbox = fake.boxes.get_mut("INBOX").unwrap();
        inbox.msgs[0].seen = true;
        inbox.msgs[1].flagged = true;
        inbox.msgs.retain(|m| m.uid != 3);
    }
    // Locally, message 2 was marked read and is still queued.
    queue_flag(&store, OWNER, &id("INBOX", 2), MessageFlag::Seen, true)
        .await
        .unwrap();
    let inbox = store
        .list_folders(OWNER, ACC)
        .await
        .unwrap()
        .into_iter()
        .find(|f| f.remote_name == "INBOX")
        .unwrap();
    fake.examine("INBOX").await.unwrap();
    let pull = pull_flags(&store, &mut fake, OWNER, &inbox, 10)
        .await
        .unwrap();
    assert_eq!((pull.changed, pull.vanished), (2, 1));
    let m1 = store.get_message(OWNER, &id("INBOX", 1)).await.unwrap();
    let m2 = store.get_message(OWNER, &id("INBOX", 2)).await.unwrap();
    assert!(m1.seen);
    assert!(m2.flagged, "server's star arrives");
    assert!(m2.seen, "the waiting local read state is not overwritten");
    assert_eq!(local_uids(&store, "INBOX").await, vec![1, 2]);

    // Once the operation is confirmed the server is the only truth again.
    replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    fake.boxes.get_mut("INBOX").unwrap().msgs[1].seen = false;
    fake.examine("INBOX").await.unwrap();
    pull_flags(&store, &mut fake, OWNER, &inbox, 10)
        .await
        .unwrap();
    assert!(
        !store
            .get_message(OWNER, &id("INBOX", 2))
            .await
            .unwrap()
            .seen
    );
}

// ---------- MB-144 ----------

#[tokio::test]
async fn trash_moves_the_message_and_shows_it_in_trash_when_the_server_reports_the_new_uid() {
    let (store, mut fake) = synced().await;
    let op = queue_move(
        &store,
        OWNER,
        &id("INBOX", 2),
        MoveTarget::Role(FolderRole::Trash),
    )
    .await
    .unwrap();
    assert_eq!(
        op.kind,
        OpKind::Move {
            dest_folder_id: "acc/INBOX.Trash".into()
        }
    );
    let report = replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    assert_eq!(
        report.done[0].result,
        OpResult::Moved {
            new_message_id: Some(id("INBOX.Trash", 1)),
            source_left_deleted: false
        }
    );
    assert_eq!(local_uids(&store, "INBOX").await, vec![1, 3]);
    assert_eq!(local_uids(&store, "INBOX.Trash").await, vec![1]);
    assert_eq!(fake.calls, vec!["move INBOX 2 -> INBOX.Trash"]);
    let moved = store
        .get_message(OWNER, &id("INBOX.Trash", 1))
        .await
        .unwrap();
    assert_eq!(moved.subject, "Thư 2");

    // Undo is a move back using the new identity.
    queue_move(
        &store,
        OWNER,
        &id("INBOX.Trash", 1),
        MoveTarget::Folder("acc/INBOX".into()),
    )
    .await
    .unwrap();
    replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    assert!(local_uids(&store, "INBOX.Trash").await.is_empty());
    assert_eq!(local_uids(&store, "INBOX").await, vec![1, 3, 4]);
}

#[tokio::test]
async fn without_copyuid_the_new_identity_is_unknown_and_reported_as_such() {
    let (store, mut fake) = synced().await;
    fake.copyuid = false;
    queue_move(
        &store,
        OWNER,
        &id("INBOX", 1),
        MoveTarget::Role(FolderRole::Archive),
    )
    .await
    .unwrap();
    let report = replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    assert_eq!(
        report.done[0].result,
        OpResult::Moved {
            new_message_id: None,
            source_left_deleted: false
        }
    );
    assert_eq!(local_uids(&store, "INBOX").await, vec![2, 3]);
    assert!(
        local_uids(&store, "INBOX.Lưu trữ").await.is_empty(),
        "appears after the destination is synced"
    );
}

#[tokio::test]
async fn without_uidplus_the_source_is_left_flagged_deleted_and_that_is_reported() {
    let (store, mut fake) = synced().await;
    fake.uidplus = false;
    queue_move(
        &store,
        OWNER,
        &id("INBOX", 1),
        MoveTarget::Role(FolderRole::Trash),
    )
    .await
    .unwrap();
    let report = replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    assert!(matches!(
        report.done[0].result,
        OpResult::Moved {
            source_left_deleted: true,
            ..
        }
    ));
    assert!(fake.msg("INBOX", 1).unwrap().deleted);
}

#[tokio::test]
async fn a_refused_move_puts_the_message_back_with_a_failed_state() {
    let (store, mut fake) = synced().await;
    queue_move(
        &store,
        OWNER,
        &id("INBOX", 1),
        MoveTarget::Role(FolderRole::Trash),
    )
    .await
    .unwrap();
    fake.fail_at = Some((1, RemoteFailure::Refused));
    let report = replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    assert!(matches!(report.done[0].result, OpResult::Rejected(_)));
    assert_eq!(local_uids(&store, "INBOX").await, vec![1, 2, 3]);
    let ops = store.list_ops(OWNER, ACC).await.unwrap();
    assert!(matches!(ops[0].state, OpState::Failed(_)));
}

#[tokio::test]
async fn a_move_interrupted_after_it_reached_the_server_is_not_repeated() {
    let (store, mut fake) = synced().await;
    queue_move(
        &store,
        OWNER,
        &id("INBOX", 2),
        MoveTarget::Role(FolderRole::Trash),
    )
    .await
    .unwrap();
    // The server performed the move but the answer never arrived.
    fake.select("INBOX").await.unwrap();
    fake.move_message(2, "INBOX.Trash").await.unwrap();
    fake.calls.clear();
    let report = replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    assert_eq!(report.done[0].result, OpResult::Gone);
    assert!(fake.calls.is_empty(), "nothing is moved a second time");
    assert_eq!(local_uids(&store, "INBOX").await, vec![1, 3]);
    assert!(store.list_ops(OWNER, ACC).await.unwrap().is_empty());
}

#[tokio::test]
async fn archive_without_an_archive_folder_is_an_explicit_error_not_an_invented_folder() {
    let (store, mut fake) = setup().await;
    fake.boxes.remove("INBOX.Lưu trữ");
    sync_all_folders(&store, &mut fake, OWNER, ACC, 50, 64)
        .await
        .unwrap();
    assert_eq!(
        queue_move(
            &store,
            OWNER,
            &id("INBOX", 1),
            MoveTarget::Role(FolderRole::Archive)
        )
        .await,
        Err(ActionError::NoFolderForRole(FolderRole::Archive))
    );
    assert_eq!(
        queue_move(
            &store,
            OWNER,
            &id("INBOX", 1),
            MoveTarget::Folder("acc/INBOX".into())
        )
        .await,
        Err(ActionError::SameFolder)
    );
    assert_eq!(
        queue_move(
            &store,
            OWNER,
            &id("INBOX", 1),
            MoveTarget::Folder("acc/Nope".into())
        )
        .await,
        Err(ActionError::NotFound)
    );
    assert_eq!(
        queue_move(
            &store,
            OWNER,
            "missing",
            MoveTarget::Role(FolderRole::Trash)
        )
        .await,
        Err(ActionError::NotFound)
    );
    assert!(store.list_ops(OWNER, ACC).await.unwrap().is_empty());
    assert!(fake.calls.is_empty());
}

#[tokio::test]
async fn permanent_delete_is_only_possible_from_trash() {
    let (store, mut fake) = synced().await;
    assert_eq!(
        queue_expunge(&store, OWNER, &id("INBOX", 1)).await,
        Err(ActionError::NotInTrash)
    );
    queue_move(
        &store,
        OWNER,
        &id("INBOX", 1),
        MoveTarget::Role(FolderRole::Trash),
    )
    .await
    .unwrap();
    replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    queue_expunge(&store, OWNER, &id("INBOX.Trash", 1))
        .await
        .unwrap();
    let report = replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    assert_eq!(report.done[0].result, OpResult::Expunged);
    assert!(fake.boxes["INBOX.Trash"].msgs.is_empty());
    assert!(local_uids(&store, "INBOX.Trash").await.is_empty());
    assert_eq!(
        fake.boxes["INBOX"].msgs.len(),
        2,
        "nothing else was expunged"
    );
}

#[tokio::test]
async fn a_queued_move_can_be_cancelled_before_it_reaches_the_server() {
    let (store, mut fake) = synced().await;
    let op = queue_move(
        &store,
        OWNER,
        &id("INBOX", 1),
        MoveTarget::Role(FolderRole::Trash),
    )
    .await
    .unwrap();
    assert!(cancel_op(&store, OWNER, ACC, &op.id).await.unwrap());
    assert!(!cancel_op(&store, OWNER, ACC, &op.id).await.unwrap());
    replay(&store, &mut fake, OWNER, ACC).await.unwrap();
    assert!(fake.calls.is_empty());

    // Cancelling a queued flag change restores the previous local value.
    let Queued::Op(flag) = queue_flag(&store, OWNER, &id("INBOX", 1), MessageFlag::Flagged, true)
        .await
        .unwrap()
    else {
        panic!("expected an operation")
    };
    assert!(cancel_op(&store, OWNER, ACC, &flag.id).await.unwrap());
    assert!(
        !store
            .get_message(OWNER, &id("INBOX", 1))
            .await
            .unwrap()
            .flagged
    );
}
