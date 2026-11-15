//! Behavioural contract every `MailStore` must satisfy. The same checks run against SQLite
//! (always) and PostgreSQL (when `MAILBOX_TEST_PG_URL` points at a throwaway database).
use mailbox_core::store::{AccountRecord, FolderRecord, MailStore, MessageRecord};
use mailbox_core::{ImapCursor, MailboxError};
use mailbox_store::{PgStore, SqliteStore};

fn account(id: &str, owner: &str) -> AccountRecord {
    AccountRecord {
        id: id.into(),
        owner_id: owner.into(),
        display_name: "Work".into(),
        email_address: "minh@company.vn".into(),
        imap_host: "imap.company.vn".into(),
        imap_port: 993,
        smtp_host: "smtp.company.vn".into(),
        smtp_port: 465,
        auth_type: "password".into(),
        credential_ref: format!("keychain:{id}"),
        status: "active".into(),
    }
}

fn folder(id: &str, account: &str, name: &str) -> FolderRecord {
    FolderRecord {
        id: id.into(),
        account_id: account.into(),
        remote_name: name.into(),
        delimiter: Some("/".into()),
        special_use: None,
        uid_validity: Some(10),
        uid_next: Some(1),
    }
}

fn message(
    id: &str,
    account: &str,
    folder: &str,
    epoch: u64,
    uid: u32,
    date: i64,
) -> MessageRecord {
    MessageRecord {
        id: id.into(),
        account_id: account.into(),
        folder_id: folder.into(),
        uid_validity: epoch,
        imap_uid: uid,
        message_id_header: Some(format!("<{id}@example.com>")),
        subject: format!("Chủ đề {id}"),
        from_display: "Trần Linh".into(),
        from_address: "linh.tran@company.vn".into(),
        preview: "Xin chào".into(),
        internal_date: date,
        size_bytes: 2048,
        seen: false,
        flagged: false,
    }
}

/// `p` makes ids unique per run so the PostgreSQL database can be reused between runs.
async fn contract(store: &impl MailStore, p: &str) {
    let (alice, bob) = (format!("{p}-alice"), format!("{p}-bob"));
    let (acc, fid) = (format!("{p}-acc"), format!("{p}-inbox"));

    // Accounts: round trip, update, owner isolation.
    let mut a = account(&acc, &alice);
    store.upsert_account(&a).await.unwrap();
    assert_eq!(store.get_account(&alice, &acc).await.unwrap(), a);
    a.display_name = "Công việc".into();
    a.status = "needs_attention".into();
    store.upsert_account(&a).await.unwrap();
    assert_eq!(store.list_accounts(&alice).await.unwrap(), vec![a.clone()]);
    assert!(store.list_accounts(&bob).await.unwrap().is_empty());
    assert_eq!(
        store.get_account(&bob, &acc).await,
        Err(MailboxError::NotFound)
    );

    // Another owner cannot take over, read, write into or delete the account.
    assert_eq!(
        store.upsert_account(&account(&acc, &bob)).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        store.get_account(&alice, &acc).await.unwrap().owner_id,
        alice
    );
    assert_eq!(
        store
            .upsert_folder(&bob, &folder(&fid, &acc, "INBOX"))
            .await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        store.list_folders(&bob, &acc).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        store.delete_account(&bob, &acc).await,
        Err(MailboxError::NotFound)
    );

    // Folders: unique per (account, remote_name); upsert updates in place.
    store
        .upsert_folder(&alice, &folder(&fid, &acc, "INBOX"))
        .await
        .unwrap();
    let mut f = folder(&fid, &acc, "INBOX");
    f.uid_validity = Some(11);
    f.special_use = Some("\\Inbox".into());
    store.upsert_folder(&alice, &f).await.unwrap();
    assert_eq!(store.list_folders(&alice, &acc).await.unwrap(), vec![f]);

    // Messages: identity is (account, folder, uidvalidity, uid).
    let m1 = message(&format!("{p}-m1"), &acc, &fid, 11, 1, 100);
    let m2 = message(&format!("{p}-m2"), &acc, &fid, 11, 2, 300);
    let m3 = message(&format!("{p}-m3"), &acc, &fid, 11, 3, 200);
    store
        .upsert_messages(&alice, &[m1.clone(), m2.clone(), m3.clone()])
        .await
        .unwrap();
    let listed = store.list_messages(&alice, &acc, &fid, 10).await.unwrap();
    assert_eq!(
        listed,
        vec![m2.clone(), m3.clone(), m1.clone()],
        "newest first"
    );
    assert_eq!(
        store
            .list_messages(&alice, &acc, &fid, 2)
            .await
            .unwrap()
            .len(),
        2,
        "limit applies"
    );

    // Same key again updates flags instead of duplicating.
    let mut seen = m1.clone();
    seen.id = format!("{p}-other-id");
    seen.seen = true;
    seen.flagged = true;
    store.upsert_messages(&alice, &[seen]).await.unwrap();
    let listed = store.list_messages(&alice, &acc, &fid, 10).await.unwrap();
    assert_eq!(listed.len(), 3);
    let updated = listed.iter().find(|m| m.imap_uid == 1).unwrap();
    assert!(updated.seen && updated.flagged);
    assert_eq!(updated.id, m1.id, "the original row is kept");

    // The same UID under a new UIDVALIDITY is a different message.
    store
        .upsert_messages(
            &alice,
            &[message(&format!("{p}-m1b"), &acc, &fid, 12, 1, 400)],
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .list_messages(&alice, &acc, &fid, 10)
            .await
            .unwrap()
            .len(),
        4
    );

    // A batch is atomic and scoped: one foreign message rejects the whole batch.
    let foreign = message(
        &format!("{p}-x"),
        &acc,
        &format!("{p}-missing-folder"),
        11,
        9,
        1,
    );
    let ok = message(&format!("{p}-m9"), &acc, &fid, 11, 99, 1);
    assert_eq!(
        store.upsert_messages(&alice, &[ok, foreign]).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        store
            .list_messages(&alice, &acc, &fid, 10)
            .await
            .unwrap()
            .len(),
        4,
        "nothing was written"
    );
    assert_eq!(
        store.upsert_messages(&bob, std::slice::from_ref(&m2)).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        store.list_messages(&bob, &acc, &fid, 10).await,
        Err(MailboxError::NotFound)
    );

    // Cursor: absent, saved, replaced.
    assert_eq!(store.load_cursor(&alice, &acc, &fid).await.unwrap(), None);
    let mut cursor = ImapCursor {
        account_id: acc.clone(),
        folder_id: fid.clone(),
        uid_validity: 11,
        last_uid: 3,
    };
    store.save_cursor(&alice, &cursor).await.unwrap();
    cursor.uid_validity = 12;
    cursor.last_uid = 1;
    store.save_cursor(&alice, &cursor).await.unwrap();
    assert_eq!(
        store.load_cursor(&alice, &acc, &fid).await.unwrap(),
        Some(cursor.clone())
    );
    assert_eq!(
        store.save_cursor(&bob, &cursor).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        store.load_cursor(&bob, &acc, &fid).await,
        Err(MailboxError::NotFound)
    );

    // Resetting a folder drops its cached messages and cursor, and is owner-scoped.
    assert_eq!(
        store.reset_folder(&bob, &acc, &fid).await,
        Err(MailboxError::NotFound)
    );
    store.reset_folder(&alice, &acc, &fid).await.unwrap();
    assert!(
        store
            .list_messages(&alice, &acc, &fid, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.load_cursor(&alice, &acc, &fid).await.unwrap(), None);
    assert_eq!(
        store.list_folders(&alice, &acc).await.unwrap().len(),
        1,
        "the folder itself stays"
    );

    // Deleting the account removes its folders, messages and cursor (local metadata only).
    store.delete_account(&alice, &acc).await.unwrap();
    assert!(store.list_accounts(&alice).await.unwrap().is_empty());
    store.upsert_account(&a).await.unwrap();
    assert!(
        store.list_folders(&alice, &acc).await.unwrap().is_empty(),
        "folders were cascaded"
    );
    store
        .upsert_folder(&alice, &folder(&fid, &acc, "INBOX"))
        .await
        .unwrap();
    assert!(
        store
            .list_messages(&alice, &acc, &fid, 10)
            .await
            .unwrap()
            .is_empty(),
        "messages were cascaded"
    );
    assert_eq!(
        store.load_cursor(&alice, &acc, &fid).await.unwrap(),
        None,
        "cursor was cascaded"
    );
    store.delete_account(&alice, &acc).await.unwrap();
}

#[tokio::test]
async fn sqlite_meets_the_contract() {
    let store = SqliteStore::in_memory().await.unwrap();
    contract(&store, "t").await;
}

#[tokio::test]
async fn sqlite_data_and_cursor_survive_restart_and_migrations_are_idempotent() {
    let dir = std::env::temp_dir().join(format!("mailbox-store-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mailbox.sqlite3");
    let _ = std::fs::remove_file(&path);
    {
        let store = SqliteStore::open(&path).await.unwrap();
        store
            .upsert_account(&account("acc", "alice"))
            .await
            .unwrap();
        store
            .upsert_folder("alice", &folder("inbox", "acc", "INBOX"))
            .await
            .unwrap();
        store
            .upsert_messages("alice", &[message("m1", "acc", "inbox", 10, 7, 50)])
            .await
            .unwrap();
        let cursor = ImapCursor {
            account_id: "acc".into(),
            folder_id: "inbox".into(),
            uid_validity: 10,
            last_uid: 7,
        };
        store.save_cursor("alice", &cursor).await.unwrap();
    }
    // Reopen: migrations run again on an existing database and data is still there.
    let store = SqliteStore::open(&path).await.unwrap();
    assert_eq!(
        store
            .list_messages("alice", "acc", "inbox", 10)
            .await
            .unwrap()
            .len(),
        1
    );
    let cursor = store
        .load_cursor("alice", "acc", "inbox")
        .await
        .unwrap()
        .unwrap();
    assert_eq!((cursor.uid_validity, cursor.last_uid), (10, 7));
    drop(store);
    // Best effort: Windows refuses to delete a database file that is still open.
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn schemas_have_no_plaintext_secret_columns() {
    for file in [
        "../../migrations/sqlite/0001_init.sql",
        "../../migrations/postgres/0001_init.sql",
    ] {
        let sql = std::fs::read_to_string(file).unwrap().to_lowercase();
        let ddl: String = sql
            .lines()
            .filter(|l| !l.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in ["password", "secret", "token", "refresh"] {
            assert!(
                !ddl.contains(forbidden),
                "{file} has a `{forbidden}` column"
            );
        }
        assert!(ddl.contains("credential_ref"));
    }
}

/// Needs a throwaway PostgreSQL, e.g.
/// `docker run --rm -d --name mailbox-test-pg -e POSTGRES_PASSWORD=test -p 55432:5432 postgres:16-alpine`
/// then `MAILBOX_TEST_PG_URL=postgres://postgres:test@127.0.0.1:55432/postgres cargo test -p mailbox-store -- --ignored`.
#[tokio::test]
#[ignore = "needs MAILBOX_TEST_PG_URL"]
async fn postgres_meets_the_same_contract() {
    let url =
        std::env::var("MAILBOX_TEST_PG_URL").expect("set MAILBOX_TEST_PG_URL to run this test");
    let store = PgStore::connect(&url).await.unwrap();
    let run = format!(
        "r{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    contract(&store, &run).await;
    // Second connect re-runs migrations on an existing schema.
    PgStore::connect(&url).await.unwrap();
}
