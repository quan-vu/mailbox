//! Owner-only end-to-end check (MB-130): real IMAP server → `fetch_body` → MIME parser →
//! SQLite body cache. Strictly read-only on the server (LIST, EXAMINE, FETCH with BODY.PEEK).
//! Asserts and prints structure only (counts, sizes, booleans) — never subjects, addresses,
//! file names or body text.
//!
//!   MAILBOX_SMTP_ENV_FILE=<path to .env.smtp> MAILBOX_TEST_IMAP_HOST=<host> \
//!     cargo test -p mailbox-store --test real_body -- --ignored --nocapture
use mailbox_core::body::BodyStore;
use mailbox_core::secrets::Secret;
use mailbox_core::store::{AccountRecord, MailStore};
use mailbox_core::sync::{discover_folders, sync_folder};
use mailbox_core::{MailServerEndpoint, TlsMode};
use mailbox_imap::Session;
use mailbox_imap::body::BodyError;
use mailbox_mime::{Limits, parse};
use mailbox_store::SqliteStore;

/// Value from the environment, else from the git-ignored env file. Values are never printed.
fn env(key: &str) -> String {
    if let Ok(v) = std::env::var(key) {
        return v;
    }
    let path = std::env::var("MAILBOX_SMTP_ENV_FILE").unwrap_or_else(|_| "../../.env.smtp".into());
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{key} is not set and the env file is not readable"));
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
        .unwrap_or_else(|| panic!("{key} is missing from the env file"))
}

#[tokio::test]
#[ignore = "owner only: real IMAP account"]
async fn real_message_body_is_fetched_parsed_and_cached_read_only() {
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
    let inbox = folders
        .iter()
        .find(|f| f.special_use.as_deref() == Some("inbox"))
        .expect("INBOX exists");
    sync_folder(&store, &mut session, "local", inbox, 20)
        .await
        .unwrap();
    let rows = store
        .list_messages("local", "acc", &inbox.id, 20)
        .await
        .unwrap();
    assert!(
        !rows.is_empty(),
        "the test mailbox needs at least one message in INBOX"
    );

    let limits = Limits::default();
    let (mut fetched, mut with_html, mut with_attachments, mut blocked) = (0, 0, 0, 0);
    // The newest few messages; the session stays on the folder `sync_folder` examined.
    for row in rows.iter().take(5) {
        let locator = store.locate_message("local", &row.id).await.unwrap();
        assert_eq!(locator.imap_uid, row.imap_uid);
        let raw = match session
            .fetch_body(locator.imap_uid, limits.max_message_bytes as u64)
            .await
        {
            Ok(raw) => raw,
            Err(BodyError::TooLarge { size }) => {
                println!("message skipped: {size} bytes is over the limit");
                continue;
            }
            Err(e) => panic!("fetch failed: {e:?}"),
        };
        assert!(!raw.is_empty());
        // The server's size and the literal agree (same message, nothing truncated).
        assert_eq!(
            raw.len() as u64,
            locator.size_bytes,
            "RFC822.SIZE matches the body length"
        );
        let parsed = parse(&raw, &limits).expect("a real message parses within the limits");
        assert!(!parsed.headers.from.is_empty(), "From header decoded");
        assert!(
            parsed.headers.date.is_some_and(|d| d > 1_000_000_000),
            "Date header decoded"
        );
        assert!(
            !parsed.text.is_empty() || parsed.html.is_some() || !parsed.attachments.is_empty(),
            "some content was extracted"
        );
        for html in parsed
            .html
            .iter()
            .chain(parsed.html_with_remote_images.iter())
        {
            let lower = html.to_lowercase();
            assert!(!lower.contains("<script") && !lower.contains("javascript:"));
            assert!(
                !lower.contains("<style") && !lower.contains("<link") && !lower.contains("<iframe")
            );
        }
        if let Some(html) = &parsed.html {
            assert!(
                !html.contains("src=\"http"),
                "no remote source in the default HTML"
            );
        }
        println!(
            "message: {} bytes raw; text {} bytes (plain part: {}); html: {}; remote blocked: {}; attachments: {}; truncated: {}",
            raw.len(),
            parsed.text.len(),
            parsed.text_is_plain,
            parsed.html.as_ref().map(|h| h.len()).unwrap_or(0),
            parsed.remote_blocked,
            parsed.attachments.len(),
            parsed.truncated
        );
        fetched += 1;
        with_html += usize::from(parsed.html.is_some());
        with_attachments += usize::from(!parsed.attachments.is_empty());
        blocked += usize::from(parsed.remote_blocked);
        let record = parsed.into_record(&row.id);
        store.save_body("local", &record).await.unwrap();
        assert_eq!(
            store.load_body("local", &row.id).await.unwrap(),
            Some(record)
        );
    }
    session.logout().await;
    println!(
        "bodies fetched: {fetched}; with html: {with_html}; with attachments: {with_attachments}; with blocked remote content: {blocked}"
    );
    assert!(fetched > 0);

    // Read-only check: flags are exactly as the first sync saw them (BODY.PEEK set no \Seen).
    let mut session = Session::connect(&endpoint, user.trim(), &pass)
        .await
        .unwrap();
    let status = session.examine(&inbox.remote_name).await.unwrap();
    let again = session.fetch_latest(status.exists, 20).await.unwrap();
    session.logout().await;
    for row in rows.iter().take(5) {
        if let Some(now) = again.iter().find(|e| e.uid == row.imap_uid) {
            assert_eq!(now.seen, row.seen, "fetching a body must not mark it read");
        }
    }
}
