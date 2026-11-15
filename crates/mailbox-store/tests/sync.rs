//! Sync use case against the real SQLite store and a scripted remote mailbox.
use mailbox_core::MailboxError;
use mailbox_core::store::{AccountRecord, MailStore};
use mailbox_core::sync::{
    RemoteEnvelope, RemoteFolder, RemoteMailbox, RemoteStatus, SyncKind, discover_folders,
    sync_folder,
};
use mailbox_store::SqliteStore;

/// A fake server holding one INBOX. Records which fetches were asked for.
struct Fake {
    uid_validity: Option<u64>,
    uids: Vec<u32>,
    fail_fetch: bool,
    calls: Vec<String>,
}

impl Fake {
    fn new(uid_validity: u64, uids: &[u32]) -> Self {
        Self {
            uid_validity: Some(uid_validity),
            uids: uids.to_vec(),
            fail_fetch: false,
            calls: Vec::new(),
        }
    }
    fn envelope(uid: u32) -> RemoteEnvelope {
        RemoteEnvelope {
            uid,
            subject: format!("Thư {uid}"),
            from_display: "A".into(),
            from_address: "a@example.com".into(),
            internal_date: i64::from(uid) * 10,
            size_bytes: 100,
            ..Default::default()
        }
    }
}

impl RemoteMailbox for Fake {
    async fn list_folders(&mut self) -> Result<Vec<RemoteFolder>, MailboxError> {
        let f = |name: &str, role: Option<&str>, selectable| RemoteFolder {
            name: name.into(),
            delimiter: Some(".".into()),
            special_use: role.map(str::to_string),
            selectable,
        };
        Ok(vec![
            f("INBOX", Some("inbox"), true),
            f("INBOX.Sent", Some("sent"), true),
            f("INBOX.Parent", None, false),
        ])
    }
    async fn examine(&mut self, folder: &str) -> Result<RemoteStatus, MailboxError> {
        self.calls.push(format!("examine {folder}"));
        Ok(RemoteStatus {
            exists: self.uids.len() as u32,
            uid_validity: self.uid_validity,
            uid_next: self.uids.last().map(|u| u64::from(*u) + 1),
        })
    }
    async fn fetch_latest(
        &mut self,
        exists: u32,
        count: u32,
    ) -> Result<Vec<RemoteEnvelope>, MailboxError> {
        self.calls.push(format!("latest {exists} {count}"));
        if self.fail_fetch {
            return Err(MailboxError::TransientConnectionFailure);
        }
        let skip = self.uids.len().saturating_sub(count as usize);
        Ok(self
            .uids
            .iter()
            .skip(skip)
            .map(|u| Self::envelope(*u))
            .collect())
    }
    async fn fetch_from_uid(&mut self, from_uid: u32) -> Result<Vec<RemoteEnvelope>, MailboxError> {
        self.calls.push(format!("from {from_uid}"));
        if self.fail_fetch {
            return Err(MailboxError::TransientConnectionFailure);
        }
        // Like a real server, `n:*` echoes the last message even when nothing is new.
        let mut out: Vec<_> = self
            .uids
            .iter()
            .filter(|u| **u >= from_uid)
            .map(|u| Self::envelope(*u))
            .collect();
        if out.is_empty()
            && let Some(last) = self.uids.last()
        {
            out.push(Self::envelope(*last));
        }
        Ok(out)
    }
}

fn account() -> AccountRecord {
    AccountRecord {
        id: "acc".into(),
        owner_id: "local".into(),
        display_name: "Test".into(),
        email_address: "t@example.com".into(),
        imap_host: "imap.example.com".into(),
        imap_port: 993,
        smtp_host: "smtp.example.com".into(),
        smtp_port: 465,
        auth_type: "password".into(),
        credential_ref: "imap:acc".into(),
        status: "active".into(),
    }
}

fn temp_db(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mailbox-sync-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("mailbox.sqlite3")
}

async fn uids(store: &SqliteStore) -> Vec<u32> {
    let mut v: Vec<u32> = store
        .list_messages("local", "acc", "acc/INBOX", 1000)
        .await
        .unwrap()
        .iter()
        .map(|m| m.imap_uid)
        .collect();
    v.sort_unstable();
    v
}

#[tokio::test]
async fn discovery_stores_selectable_folders_with_roles() {
    let store = SqliteStore::in_memory().await.unwrap();
    store.upsert_account(&account()).await.unwrap();
    let mut remote = Fake::new(1, &[]);
    let found = discover_folders(&store, &mut remote, "local", "acc")
        .await
        .unwrap();
    assert_eq!(found.len(), 2, "the non-selectable container is skipped");
    let stored = store.list_folders("local", "acc").await.unwrap();
    assert_eq!(
        stored
            .iter()
            .map(|f| (f.remote_name.as_str(), f.special_use.as_deref()))
            .collect::<Vec<_>>(),
        vec![("INBOX", Some("inbox")), ("INBOX.Sent", Some("sent"))]
    );
}

#[tokio::test]
async fn first_sync_takes_a_window_then_only_new_mail_even_after_restart() {
    let path = temp_db("restart");
    let mut remote = Fake::new(7, &[1, 2, 3, 4, 5]);
    {
        let store = SqliteStore::open(&path).await.unwrap();
        store.upsert_account(&account()).await.unwrap();
        let inbox = discover_folders(&store, &mut remote, "local", "acc")
            .await
            .unwrap()
            .remove(0);
        let report = sync_folder(&store, &mut remote, "local", &inbox, 3)
            .await
            .unwrap();
        assert_eq!((report.kind, report.stored), (SyncKind::Initial, 3));
        assert_eq!(uids(&store).await, vec![3, 4, 5], "only the newest window");
    }
    // The app restarts; two new messages arrived meanwhile.
    remote.uids.extend([6, 7]);
    remote.calls.clear();
    let store = SqliteStore::open(&path).await.unwrap();
    let inbox = store
        .list_folders("local", "acc")
        .await
        .unwrap()
        .into_iter()
        .find(|f| f.remote_name == "INBOX")
        .unwrap();
    let report = sync_folder(&store, &mut remote, "local", &inbox, 3)
        .await
        .unwrap();
    assert_eq!((report.kind, report.stored), (SyncKind::Incremental, 2));
    assert_eq!(
        remote.calls,
        vec!["examine INBOX", "from 6"],
        "resumes from the persisted cursor"
    );
    assert_eq!(uids(&store).await, vec![3, 4, 5, 6, 7]);

    // Nothing new: the echoed last message is not stored twice and the cursor stays.
    let report = sync_folder(&store, &mut remote, "local", &inbox, 3)
        .await
        .unwrap();
    assert_eq!(report.stored, 0);
    assert_eq!(
        store
            .load_cursor("local", "acc", "acc/INBOX")
            .await
            .unwrap()
            .unwrap()
            .last_uid,
        7
    );
    drop(store);
    // Best effort: Windows refuses to delete a database file that is still open.
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[tokio::test]
async fn uidvalidity_change_discards_the_old_epoch_and_rebuilds() {
    let store = SqliteStore::in_memory().await.unwrap();
    store.upsert_account(&account()).await.unwrap();
    let mut remote = Fake::new(10, &[1, 2, 3]);
    let inbox = discover_folders(&store, &mut remote, "local", "acc")
        .await
        .unwrap()
        .remove(0);
    sync_folder(&store, &mut remote, "local", &inbox, 50)
        .await
        .unwrap();

    // The server recreated the mailbox: same UID numbers now mean different messages.
    let mut remote = Fake::new(11, &[1, 2]);
    let report = sync_folder(&store, &mut remote, "local", &inbox, 50)
        .await
        .unwrap();
    assert_eq!((report.kind, report.stored), (SyncKind::Rebuilt, 2));
    let rows = store
        .list_messages("local", "acc", "acc/INBOX", 100)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2, "no rows of the old epoch remain");
    assert!(rows.iter().all(|m| m.uid_validity == 11));
    let cursor = store
        .load_cursor("local", "acc", "acc/INBOX")
        .await
        .unwrap()
        .unwrap();
    assert_eq!((cursor.uid_validity, cursor.last_uid), (11, 2));
}

#[tokio::test]
async fn failed_fetch_leaves_cursor_and_data_untouched() {
    let store = SqliteStore::in_memory().await.unwrap();
    store.upsert_account(&account()).await.unwrap();
    let mut remote = Fake::new(3, &[1, 2, 3]);
    let inbox = discover_folders(&store, &mut remote, "local", "acc")
        .await
        .unwrap()
        .remove(0);
    remote.fail_fetch = true;
    assert_eq!(
        sync_folder(&store, &mut remote, "local", &inbox, 50).await,
        Err(MailboxError::TransientConnectionFailure)
    );
    assert_eq!(
        store
            .load_cursor("local", "acc", "acc/INBOX")
            .await
            .unwrap(),
        None
    );
    assert!(uids(&store).await.is_empty());

    remote.fail_fetch = false;
    sync_folder(&store, &mut remote, "local", &inbox, 50)
        .await
        .unwrap();
    remote.uids.push(4);
    remote.fail_fetch = true;
    assert!(
        sync_folder(&store, &mut remote, "local", &inbox, 50)
            .await
            .is_err()
    );
    assert_eq!(
        store
            .load_cursor("local", "acc", "acc/INBOX")
            .await
            .unwrap()
            .unwrap()
            .last_uid,
        3,
        "cursor did not move"
    );
}

#[tokio::test]
async fn server_without_uidvalidity_is_refused() {
    let store = SqliteStore::in_memory().await.unwrap();
    store.upsert_account(&account()).await.unwrap();
    let mut remote = Fake::new(1, &[1]);
    let inbox = discover_folders(&store, &mut remote, "local", "acc")
        .await
        .unwrap()
        .remove(0);
    remote.uid_validity = None;
    assert!(matches!(
        sync_folder(&store, &mut remote, "local", &inbox, 50).await,
        Err(MailboxError::InconsistentData(_))
    ));
}
