//! MB-200 "load older" and MB-201 header storage/backfill against a fake server on SQLite.
use mailbox_core::MailboxError;
use mailbox_core::history::{RemoteHistory, backfill_headers, fetch_older};
use mailbox_core::search::{Narrowing, PageStore, Scope};
use mailbox_core::store::{AccountRecord, FolderRecord, MailStore};
use mailbox_core::sync::{
    RemoteEnvelope, RemoteFolder, RemoteMailbox, RemoteStatus, discover_folders, sync_folder,
};
use mailbox_core::threading::{EnvelopeHeaders, ThreadStore, rethread_account};
use mailbox_store::SqliteStore;

const OWNER: &str = "local";

/// A folder whose UIDs are `uids`; message n replies to message n-1 when `chain` is set.
struct Fake {
    uids: Vec<u32>,
    uid_validity: u64,
    with_headers: bool,
    commands: Vec<String>,
}

impl Fake {
    fn new(uids: Vec<u32>) -> Self {
        Self {
            uids,
            uid_validity: 7,
            with_headers: true,
            commands: Vec::new(),
        }
    }
    fn envelope(&self, uid: u32) -> RemoteEnvelope {
        let headers = if self.with_headers && uid > 1 {
            EnvelopeHeaders {
                in_reply_to: Some(format!("<m{}@x.org>", uid - 1)),
                references: vec!["<m1@x.org>".into(), format!("<m{}@x.org>", uid - 1)],
                to: "me@here.org".into(),
                cc: String::new(),
            }
        } else {
            EnvelopeHeaders::default()
        };
        RemoteEnvelope {
            uid,
            subject: format!("Thư {uid}"),
            from_address: "a@x.org".into(),
            internal_date: i64::from(uid) * 60,
            message_id: Some(format!("<m{uid}@x.org>")),
            headers,
            ..Default::default()
        }
    }
}

impl RemoteMailbox for Fake {
    async fn list_folders(&mut self) -> Result<Vec<RemoteFolder>, MailboxError> {
        Ok(vec![RemoteFolder {
            name: "INBOX".into(),
            delimiter: None,
            special_use: None,
            selectable: true,
        }])
    }
    async fn examine(&mut self, _folder: &str) -> Result<RemoteStatus, MailboxError> {
        self.commands.push("EXAMINE".into());
        Ok(RemoteStatus {
            exists: self.uids.len() as u32,
            uid_validity: Some(self.uid_validity),
            uid_next: self.uids.last().map(|u| u64::from(*u) + 1),
        })
    }
    async fn fetch_latest(
        &mut self,
        exists: u32,
        count: u32,
    ) -> Result<Vec<RemoteEnvelope>, MailboxError> {
        let skip = exists.saturating_sub(count) as usize;
        Ok(self.uids[skip..]
            .iter()
            .map(|u| self.envelope(*u))
            .collect())
    }
    async fn fetch_from_uid(&mut self, from_uid: u32) -> Result<Vec<RemoteEnvelope>, MailboxError> {
        Ok(self
            .uids
            .iter()
            .filter(|u| **u >= from_uid)
            .map(|u| self.envelope(*u))
            .collect())
    }
}

impl RemoteHistory for Fake {
    async fn fetch_uid_range(
        &mut self,
        lo: u32,
        hi: u32,
    ) -> Result<Vec<RemoteEnvelope>, MailboxError> {
        self.commands.push(format!("RANGE {lo}:{hi}"));
        Ok(self
            .uids
            .iter()
            .filter(|u| (lo..=hi).contains(*u))
            .map(|u| self.envelope(*u))
            .collect())
    }
    async fn fetch_uid_set(&mut self, uids: &[u32]) -> Result<Vec<RemoteEnvelope>, MailboxError> {
        self.commands.push(format!("SET {}", uids.len()));
        Ok(self
            .uids
            .iter()
            .filter(|u| uids.contains(u))
            .map(|u| self.envelope(*u))
            .collect())
    }
}

async fn setup() -> (SqliteStore, FolderRecord) {
    let store = SqliteStore::in_memory().await.unwrap();
    store
        .upsert_account(&AccountRecord {
            id: "acc".into(),
            owner_id: OWNER.into(),
            display_name: "x".into(),
            email_address: "me@here.org".into(),
            imap_host: "imap.x.org".into(),
            imap_port: 993,
            smtp_host: "smtp.x.org".into(),
            smtp_port: 465,
            auth_type: "password".into(),
            credential_ref: "imap.acc".into(),
            status: "active".into(),
        })
        .await
        .unwrap();
    let folders = discover_folders(&store, &mut Fake::new(vec![]), OWNER, "acc")
        .await
        .unwrap();
    (store, folders[0].clone())
}

fn inbox(folder: &FolderRecord) -> Scope {
    Scope::Folder {
        account_id: folder.account_id.clone(),
        folder_id: folder.id.clone(),
    }
}

#[tokio::test]
async fn load_older_walks_down_in_bounded_batches_until_the_folder_start() {
    let (store, folder) = setup().await;
    // 250 messages with a gap of deleted UIDs between 40 and 160.
    let uids: Vec<u32> = (1..=40).chain(161..=370).collect();
    let mut remote = Fake::new(uids);
    sync_folder(&store, &mut remote, OWNER, &folder, 100)
        .await
        .unwrap();
    let count = |s: &SqliteStore| {
        let s = s.clone();
        let scope = inbox(&folder);
        async move {
            s.page(OWNER, &scope, &Narrowing::default(), None, 500)
                .await
                .unwrap()
                .len()
        }
    };
    assert_eq!(count(&store).await, 100);

    let first = fetch_older(&store, &mut remote, OWNER, &folder, 50)
        .await
        .unwrap();
    assert_eq!((first.stored, first.reached_start), (50, false));
    let second = fetch_older(&store, &mut remote, OWNER, &folder, 50)
        .await
        .unwrap();
    assert_eq!(second.stored, 50);
    // Now at UID 171; below is 161..=170 then the gap, then 1..=40.
    let mut rounds = 0;
    loop {
        let r = fetch_older(&store, &mut remote, OWNER, &folder, 50)
            .await
            .unwrap();
        rounds += 1;
        assert!(r.stored <= 400, "bounded per call");
        if r.reached_start {
            break;
        }
        assert!(rounds < 10, "reaches the start");
    }
    assert_eq!(
        count(&store).await,
        250,
        "every message cached exactly once"
    );
    let done = fetch_older(&store, &mut remote, OWNER, &folder, 50)
        .await
        .unwrap();
    assert_eq!((done.stored, done.reached_start), (0, true));
    // Older messages carry headers; the cursor (highest UID) did not move.
    assert!(
        store
            .uids_missing_headers(OWNER, "acc", &folder.id, 7, 1000)
            .await
            .unwrap()
            .len()
            <= 1,
        "only UID 1 (no reply headers) may lack them"
    );
    let cursor = store
        .load_cursor(OWNER, "acc", &folder.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cursor.last_uid, 370);
    // Read-only: only EXAMINE and FETCH-style calls.
    assert!(
        remote
            .commands
            .iter()
            .all(|c| c == "EXAMINE" || c.starts_with("RANGE") || c.starts_with("SET"))
    );

    // All 250 are one conversation (each replies to the previous one).
    rethread_account(&store, OWNER, "acc", &["me@here.org".into()])
        .await
        .unwrap();
    let sizes = store.thread_sizes(OWNER, "acc").await.unwrap();
    assert_eq!(sizes.len(), 1);
    assert_eq!(sizes[0].1, 250);
}

#[tokio::test]
async fn load_older_refuses_after_a_uidvalidity_change() {
    let (store, folder) = setup().await;
    let mut remote = Fake::new((1..=200).collect());
    sync_folder(&store, &mut remote, OWNER, &folder, 100)
        .await
        .unwrap();
    remote.uid_validity = 8;
    assert_eq!(
        fetch_older(&store, &mut remote, OWNER, &folder, 50).await,
        Err(MailboxError::UidValidityChanged)
    );
    assert!(!remote.commands.iter().any(|c| c.starts_with("RANGE")));
}

#[tokio::test]
async fn messages_cached_before_headers_were_kept_are_backfilled_lazily() {
    let (store, folder) = setup().await;
    let mut remote = Fake::new((1..=30).collect());
    // A cache written before migration 0005: envelopes and cursor, no header rows.
    let envelopes: Vec<RemoteEnvelope> = (1..=30).map(|u| remote.envelope(u)).collect();
    let (records, _) = mailbox_core::history::records_for(&folder, 7, &envelopes);
    store.upsert_messages(OWNER, &records).await.unwrap();
    store
        .save_cursor(
            OWNER,
            &mailbox_core::ImapCursor {
                account_id: "acc".into(),
                folder_id: folder.id.clone(),
                uid_validity: 7,
                last_uid: 30,
            },
        )
        .await
        .unwrap();
    rethread_account(&store, OWNER, "acc", &[]).await.unwrap();
    assert_eq!(
        store.thread_sizes(OWNER, "acc").await.unwrap().len(),
        30,
        "without headers every message is its own conversation"
    );

    let updated = backfill_headers(&store, &mut remote, OWNER, &folder, 20)
        .await
        .unwrap();
    assert_eq!(updated, 20, "bounded per call");
    let updated = backfill_headers(&store, &mut remote, OWNER, &folder, 20)
        .await
        .unwrap();
    assert_eq!(updated, 10);
    let before = remote.commands.len();
    assert_eq!(
        backfill_headers(&store, &mut remote, OWNER, &folder, 20)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        remote.commands.len(),
        before,
        "nothing left: no server round trip"
    );
    rethread_account(&store, OWNER, "acc", &[]).await.unwrap();
    assert_eq!(store.thread_sizes(OWNER, "acc").await.unwrap().len(), 1);
}

#[tokio::test]
async fn backfill_marks_messages_gone_from_the_server_so_they_are_not_asked_again() {
    let (store, folder) = setup().await;
    let mut remote = Fake::new((1..=5).collect());
    let envelopes: Vec<RemoteEnvelope> = (1..=5).map(|u| remote.envelope(u)).collect();
    let (records, _) = mailbox_core::history::records_for(&folder, 7, &envelopes);
    store.upsert_messages(OWNER, &records).await.unwrap();
    store
        .save_cursor(
            OWNER,
            &mailbox_core::ImapCursor {
                account_id: "acc".into(),
                folder_id: folder.id.clone(),
                uid_validity: 7,
                last_uid: 5,
            },
        )
        .await
        .unwrap();
    remote.uids = vec![1, 2]; // 3..5 were expunged elsewhere
    assert_eq!(
        backfill_headers(&store, &mut remote, OWNER, &folder, 20)
            .await
            .unwrap(),
        2
    );
    assert!(
        store
            .uids_missing_headers(OWNER, "acc", &folder.id, 7, 100)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn sync_stores_reply_headers_with_new_envelopes() {
    let (store, folder) = setup().await;
    let mut remote = Fake::new((1..=3).collect());
    sync_folder(&store, &mut remote, OWNER, &folder, 100)
        .await
        .unwrap();
    let id = format!("{}#7#3", folder.id);
    let h = store.headers_of(OWNER, &id).await.unwrap().unwrap();
    assert_eq!(h.in_reply_to.as_deref(), Some("<m2@x.org>"));
    assert_eq!(h.references, vec!["<m1@x.org>", "<m2@x.org>"]);
    assert!(
        store
            .uids_missing_headers(OWNER, "acc", &folder.id, 7, 100)
            .await
            .unwrap()
            .is_empty()
    );
}
