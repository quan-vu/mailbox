//! Behavioural contract of `BodyStore` (MB-130). SQLite always; PostgreSQL when
//! `MAILBOX_TEST_PG_URL` points at a throwaway database (see `contract.rs`).
use mailbox_core::MailboxError;
use mailbox_core::body::{AttachmentMeta, BodyRecord, BodyStore, MessageLocator};
use mailbox_core::store::{AccountRecord, FolderRecord, MailStore, MessageRecord};
use mailbox_store::{PgStore, SqliteStore};

fn body(message_id: &str) -> BodyRecord {
    BodyRecord {
        message_id: message_id.into(),
        text: "Xin chào,\n\tdòng hai 😀".into(),
        html: Some("<p>Xin <b>chào</b></p><img alt=\"x\">".into()),
        html_remote: Some(
            "<p>Xin <b>chào</b></p><img src=\"https://img.example/a.png\" alt=\"x\">".into(),
        ),
        remote_blocked: true,
        truncated: false,
        attachments: (0..12)
            .map(|part| AttachmentMeta {
                part,
                filename: format!("Báo cáo {part}.pdf"),
                media_type: "application/pdf".into(),
                size_bytes: 3_000_000_000 + u64::from(part),
                content_id: (part == 1).then(|| "logo@x".to_string()),
                inline: part == 1,
            })
            .collect(),
    }
}

async fn contract<S: MailStore + BodyStore>(store: &S, p: &str) {
    let (alice, bob) = (format!("{p}-alice"), format!("{p}-bob"));
    let (acc, fid) = (format!("{p}-acc"), format!("{p}-inbox"));
    let (m1, m2) = (format!("{p}-m1"), format!("{p}-m2"));
    store
        .upsert_account(&AccountRecord {
            id: acc.clone(),
            owner_id: alice.clone(),
            display_name: "Work".into(),
            email_address: "minh@example.vn".into(),
            imap_host: "imap.example.vn".into(),
            imap_port: 993,
            smtp_host: "smtp.example.vn".into(),
            smtp_port: 465,
            auth_type: "password".into(),
            credential_ref: format!("keychain:{acc}"),
            status: "active".into(),
        })
        .await
        .unwrap();
    let folder = FolderRecord {
        id: fid.clone(),
        account_id: acc.clone(),
        remote_name: "INBOX.Dự án".into(),
        delimiter: Some(".".into()),
        special_use: None,
        uid_validity: Some(10),
        uid_next: None,
    };
    store.upsert_folder(&alice, &folder).await.unwrap();
    let message = |id: &str, uid: u32| MessageRecord {
        id: id.into(),
        account_id: acc.clone(),
        folder_id: fid.clone(),
        uid_validity: 10,
        imap_uid: uid,
        message_id_header: None,
        subject: "s".into(),
        from_display: String::new(),
        from_address: "a@example.org".into(),
        preview: String::new(),
        internal_date: 1,
        size_bytes: 4096,
        seen: false,
        flagged: false,
    };
    store
        .upsert_messages(&alice, &[message(&m1, 7), message(&m2, 8)])
        .await
        .unwrap();

    // Locator: everything needed to fetch from the server, owner-scoped.
    assert_eq!(
        store.locate_message(&alice, &m1).await.unwrap(),
        MessageLocator {
            account_id: acc.clone(),
            folder_remote_name: "INBOX.Dự án".into(),
            uid_validity: 10,
            imap_uid: 7,
            size_bytes: 4096,
        }
    );
    assert_eq!(
        store.locate_message(&bob, &m1).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        store.locate_message(&alice, "nope").await,
        Err(MailboxError::NotFound)
    );

    // Nothing cached yet.
    assert_eq!(store.load_body(&alice, &m1).await.unwrap(), None);

    // Round trip, attachments in part order (12 parts: "10" must not sort before "2").
    let b = body(&m1);
    store.save_body(&alice, &b).await.unwrap();
    assert_eq!(store.load_body(&alice, &m1).await.unwrap(), Some(b.clone()));
    assert_eq!(store.load_body(&alice, &m2).await.unwrap(), None);

    // Replace: fewer attachments, no HTML.
    let mut b2 = body(&m1);
    b2.text = "khác".into();
    b2.html = None;
    b2.html_remote = None;
    b2.remote_blocked = false;
    b2.truncated = true;
    b2.attachments.truncate(1);
    store.save_body(&alice, &b2).await.unwrap();
    assert_eq!(
        store.load_body(&alice, &m1).await.unwrap(),
        Some(b2.clone())
    );

    // Another owner can neither read, write nor delete; a missing message cannot get a body.
    assert_eq!(
        store.load_body(&bob, &m1).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(store.save_body(&bob, &b).await, Err(MailboxError::NotFound));
    assert_eq!(
        store.delete_body(&bob, &m1).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        store.save_body(&alice, &body("nope")).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(store.load_body(&alice, &m1).await.unwrap(), Some(b2));

    // A re-synced envelope keeps its cached body.
    store
        .upsert_messages(&alice, &[message(&m1, 7)])
        .await
        .unwrap();
    assert!(store.load_body(&alice, &m1).await.unwrap().is_some());

    // Delete one body.
    store.save_body(&alice, &body(&m2)).await.unwrap();
    store.delete_body(&alice, &m2).await.unwrap();
    assert_eq!(store.load_body(&alice, &m2).await.unwrap(), None);
    assert!(store.load_body(&alice, &m1).await.unwrap().is_some());

    // A folder reset (UIDVALIDITY change) takes the bodies of the old epoch with it.
    store.save_body(&alice, &body(&m2)).await.unwrap();
    store.reset_folder(&alice, &acc, &fid).await.unwrap();
    assert_eq!(
        store.load_body(&alice, &m1).await,
        Err(MailboxError::NotFound)
    );
    store
        .upsert_messages(&alice, &[message(&m1, 7)])
        .await
        .unwrap();
    assert_eq!(
        store.load_body(&alice, &m1).await.unwrap(),
        None,
        "no stale body or attachments"
    );
    store.save_body(&alice, &body(&m1)).await.unwrap();

    // Removing the account removes everything.
    store.delete_account(&alice, &acc).await.unwrap();
    assert_eq!(
        store.load_body(&alice, &m1).await,
        Err(MailboxError::NotFound)
    );
}

#[tokio::test]
async fn sqlite_meets_the_body_contract() {
    contract(&SqliteStore::in_memory().await.unwrap(), "t").await;
}

#[tokio::test]
async fn bodies_survive_reopening_an_existing_database() {
    let dir = std::env::temp_dir().join(format!("mailbox-body-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.sqlite3");
    {
        let store = SqliteStore::open(&path).await.unwrap();
        // Leaves an account-less database behind after the contract; then store one more body.
        contract(&store, "a").await;
    }
    // Second open re-runs migrations (0001 + 0002) on the existing file.
    let store = SqliteStore::open(&path).await.unwrap();
    contract(&store, "b").await;
    drop(store);
    // Best effort: Windows refuses to delete a database file that is still open.
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn body_schema_stores_no_secrets_and_no_attachment_bytes() {
    for file in [
        "../../migrations/sqlite/0002_message_bodies.sql",
        "../../migrations/postgres/0002_message_bodies.sql",
    ] {
        let ddl: String = std::fs::read_to_string(file)
            .unwrap()
            .to_lowercase()
            .lines()
            .filter(|l| !l.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in ["password", "secret", "token", "blob", "bytea"] {
            assert!(!ddl.contains(forbidden), "{file} has `{forbidden}`");
        }
    }
}

#[tokio::test]
#[ignore = "needs MAILBOX_TEST_PG_URL"]
async fn postgres_meets_the_body_contract() {
    let url =
        std::env::var("MAILBOX_TEST_PG_URL").expect("set MAILBOX_TEST_PG_URL to run this test");
    let store = PgStore::connect(&url).await.unwrap();
    let run = format!(
        "b{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    contract(&store, &run).await;
    PgStore::connect(&url).await.unwrap();
}
