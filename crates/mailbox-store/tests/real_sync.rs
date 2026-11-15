//! Owner-only end-to-end check: real IMAP server → sync use case → SQLite. Read-only on the
//! server (LIST, EXAMINE, FETCH BODY.PEEK). Prints counts only, never subjects or addresses.
//!
//!   MAILBOX_TEST_IMAP_HOST=<host> cargo test -p mailbox-store --test real_sync -- --ignored --nocapture
//!
//! Login name and password are read from the git-ignored `.env.smtp` (`SMTP_USER_1`,
//! `SMTP_PASS_1`: the same mailbox is used for IMAP and SMTP) or from the environment.
use mailbox_core::secrets::Secret;
use mailbox_core::store::{AccountRecord, MailStore};
use mailbox_core::sync::{SyncKind, discover_folders, sync_folder};
use mailbox_core::{MailServerEndpoint, TlsMode};
use mailbox_imap::Session;
use mailbox_store::SqliteStore;

fn env(key: &str) -> String {
    if let Ok(v) = std::env::var(key) {
        return v;
    }
    let text = std::fs::read_to_string("../../.env.smtp")
        .unwrap_or_else(|_| panic!("{key} is not set and .env.smtp is not readable"));
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
        .unwrap_or_else(|| panic!("{key} is missing from .env.smtp"))
}

#[tokio::test]
#[ignore = "owner only: real IMAP account"]
async fn real_inbox_syncs_into_sqlite_and_resumes_incrementally() {
    let endpoint = MailServerEndpoint {
        hostname: env("MAILBOX_TEST_IMAP_HOST"),
        port: 993,
        tls_mode: TlsMode::ImplicitTls,
    };
    let (user, pass) = (env("SMTP_USER_1"), Secret::new(env("SMTP_PASS_1")));
    let store = SqliteStore::in_memory().await.unwrap();
    store
        .upsert_account(&AccountRecord {
            id: "acc".into(),
            owner_id: "local".into(),
            display_name: "Test".into(),
            email_address: user.trim().to_string(),
            imap_host: endpoint.hostname.clone(),
            imap_port: 993,
            smtp_host: endpoint.hostname.clone(),
            smtp_port: 465,
            auth_type: "password".into(),
            credential_ref: "imap:acc".into(),
            status: "active".into(),
        })
        .await
        .unwrap();

    let mut session = Session::connect(&endpoint, user.trim(), &pass)
        .await
        .expect("login over verified TLS");
    let folders = discover_folders(&store, &mut session, "local", "acc")
        .await
        .unwrap();
    let roles: Vec<_> = folders
        .iter()
        .filter_map(|f| f.special_use.clone())
        .collect();
    println!("folders: {} (roles: {})", folders.len(), roles.join(", "));
    let inbox = folders
        .iter()
        .find(|f| f.special_use.as_deref() == Some("inbox"))
        .expect("INBOX exists");

    let first = sync_folder(&store, &mut session, "local", inbox, 50)
        .await
        .unwrap();
    assert_eq!(first.kind, SyncKind::Initial);
    let second = sync_folder(&store, &mut session, "local", inbox, 50)
        .await
        .unwrap();
    assert_eq!(second.kind, SyncKind::Incremental);
    // Other folders: counts only, to exercise header parsing on whatever real mail exists.
    for f in folders.iter().filter(|f| f.id != inbox.id) {
        let report = sync_folder(&store, &mut session, "local", f, 50)
            .await
            .unwrap();
        let rows = store
            .list_messages("local", "acc", &f.id, 100)
            .await
            .unwrap();
        println!(
            "folder role {:?}: stored {}; with subject {}; with sender {}; with date {}",
            f.special_use.as_deref().unwrap_or("-"),
            report.stored,
            rows.iter().filter(|m| !m.subject.is_empty()).count(),
            rows.iter().filter(|m| m.from_address.contains('@')).count(),
            rows.iter()
                .filter(|m| m.internal_date > 1_500_000_000)
                .count()
        );
    }
    session.logout().await;

    let rows = store
        .list_messages("local", "acc", &inbox.id, 100)
        .await
        .unwrap();
    let cursor = store
        .load_cursor("local", "acc", &inbox.id)
        .await
        .unwrap()
        .expect("cursor saved");
    println!(
        "first sync stored {}; second stored {} (new mail may arrive in between)",
        first.stored, second.stored
    );
    println!(
        "rows: {}; with subject: {}; with sender: {}; with date: {}",
        rows.len(),
        rows.iter().filter(|m| !m.subject.is_empty()).count(),
        rows.iter().filter(|m| m.from_address.contains('@')).count(),
        rows.iter().filter(|m| m.internal_date > 0).count()
    );
    assert_eq!(rows.len(), first.stored + second.stored);
    assert!(cursor.uid_validity > 0);
    assert!(
        rows.iter()
            .all(|m| m.uid_validity == cursor.uid_validity && m.imap_uid <= cursor.last_uid)
    );
}
