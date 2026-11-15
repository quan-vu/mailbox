//! Owner-only end-to-end check of flags and moves against a real IMAP/SMTP provider
//! (MB-132, MB-144, MB-122, MB-143).
//!
//! What it does, and nothing else: sends ONE message from the test mailbox to itself with a
//! unique marker subject, waits for it, flags it, moves it to the trash folder and permanently
//! deletes exactly that message. Every mutation is preceded by a check that the cached row
//! carries the marker subject. No pre-existing message is flagged, moved or deleted, and a
//! plain EXPUNGE is never sent. Prints counts and capabilities only, never addresses.
//!
//!   MAILBOX_SMTP_ENV_FILE=/abs/path/.env.smtp MAILBOX_SMTP_SEND_TO=self \
//!   MAILBOX_TEST_IMAP_HOST=<imap host> \
//!     cargo test -p mailbox-store --test real_actions -- --ignored --nocapture
use mailbox_core::actions::{
    MessageFlag, MoveTarget, OpResult, PendingOpStore, queue_expunge, queue_flag, queue_move,
    replay,
};
use mailbox_core::folders::{FolderRole, role_of, sync_all_folders};
use mailbox_core::secrets::Secret;
use mailbox_core::store::{AccountRecord, MailStore, MessageRecord};
use mailbox_core::{MailServerEndpoint, TlsMode};
use mailbox_imap::Session;
use mailbox_imap::mutate::MutableSession;
use mailbox_smtp::{SendOutcome, TextMessage, send};
use mailbox_store::SqliteStore;
use std::time::Duration;

const OWNER: &str = "local";
const ACC: &str = "acc";

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

fn bare_address(from: &str) -> String {
    match (from.find('<'), from.rfind('>')) {
        (Some(a), Some(b)) if b > a => from[a + 1..b].trim().to_string(),
        _ => from.trim().to_string(),
    }
}

/// The one cached row with the marker subject, wherever it is.
async fn find_marked(store: &SqliteStore, marker: &str) -> Option<MessageRecord> {
    let mut found = Vec::new();
    for folder in store.list_folders(OWNER, ACC).await.unwrap() {
        for m in store.folder_messages(OWNER, ACC, &folder.id).await.unwrap() {
            if m.subject == marker {
                found.push(m);
            }
        }
    }
    assert!(
        found.len() <= 1,
        "the marker must identify exactly one message"
    );
    found.pop()
}

async fn assert_is_ours(store: &SqliteStore, message_id: &str, marker: &str) {
    let row = store.get_message(OWNER, message_id).await.unwrap();
    assert_eq!(
        row.subject, marker,
        "refusing to touch a message this test did not create"
    );
}

#[tokio::test]
#[ignore = "owner only: real mailbox; sends one message to the test mailbox itself and removes it"]
async fn real_message_is_flagged_moved_to_trash_and_cleaned_up() {
    assert_eq!(
        env("MAILBOX_SMTP_SEND_TO").trim(),
        "self",
        "this test only ever sends to the test mailbox itself"
    );
    let user = env("SMTP_USER_1").trim().to_string();
    let pass = Secret::new(env("SMTP_PASS_1"));
    let smtp_port: u16 = env("SMTP_PORT_1").trim().parse().expect("SMTP_PORT_1");
    let smtp = MailServerEndpoint {
        hostname: env("SMTP_HOST_1").trim().to_string(),
        port: smtp_port,
        tls_mode: if smtp_port == 465 {
            TlsMode::ImplicitTls
        } else {
            TlsMode::StartTls
        },
    };
    let imap = MailServerEndpoint {
        hostname: std::env::var("MAILBOX_TEST_IMAP_HOST")
            .unwrap_or_else(|_| smtp.hostname.clone())
            .trim()
            .to_string(),
        port: 993,
        tls_mode: TlsMode::ImplicitTls,
    };

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let marker = format!("MailBox MB-132 action test {nanos:x}");
    let outcome = send(
        &smtp,
        &user,
        &pass,
        &TextMessage {
            from: bare_address(&env("SMTP_FROM_1")),
            to: user.clone(),
            subject: marker.clone(),
            body: "Automated MailBox test message. It is flagged, moved to the trash folder and deleted by the same test run.\n".into(),
        },
    )
    .await
    .expect("SMTP session over verified TLS");
    assert_eq!(outcome, SendOutcome::Accepted);
    println!("sent 1 marked message to the test mailbox itself");

    let store = SqliteStore::in_memory().await.unwrap();
    store
        .upsert_account(&AccountRecord {
            id: ACC.into(),
            owner_id: OWNER.into(),
            display_name: "Test".into(),
            email_address: user.clone(),
            imap_host: imap.hostname.clone(),
            imap_port: 993,
            smtp_host: smtp.hostname.clone(),
            smtp_port,
            auth_type: "password".into(),
            credential_ref: "imap.acc".into(),
            status: "active".into(),
        })
        .await
        .unwrap();

    // MB-143: sync every folder until the marked message has arrived.
    let mut arrived = None;
    for attempt in 1..=18 {
        let mut session = Session::connect(&imap, &user, &pass)
            .await
            .expect("IMAP login over verified TLS");
        let report = sync_all_folders(&store, &mut session, OWNER, ACC, 50, 64)
            .await
            .unwrap();
        session.logout().await;
        if attempt == 1 {
            let roles: Vec<&str> = report
                .folders
                .iter()
                .filter_map(|f| f.role.map(|r| r.key()))
                .collect();
            println!(
                "folders synced: {} (failed {}, skipped {}); roles: {}",
                report.folders.len(),
                report.failed(),
                report.skipped,
                roles.join(", ")
            );
            assert_eq!(report.failed(), 0);
        }
        arrived = find_marked(&store, &marker).await;
        if arrived.is_some() {
            println!("marked message arrived after {attempt} sync(s)");
            break;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    let message = arrived.expect("the self-sent message did not arrive within 90 seconds");
    let folders = store.list_folders(OWNER, ACC).await.unwrap();
    let source = folders
        .iter()
        .find(|f| f.id == message.folder_id)
        .unwrap()
        .clone();
    println!(
        "it arrived in the folder with role {:?}",
        role_of(&source).map(|r| r.key())
    );
    let trash = folders
        .iter()
        .find(|f| role_of(f) == Some(FolderRole::Trash))
        .expect("this account has a trash folder")
        .clone();

    // MB-132: flag it, replay, then read the flags back from the server.
    assert_is_ours(&store, &message.id, &marker).await;
    queue_flag(&store, OWNER, &message.id, MessageFlag::Flagged, true)
        .await
        .unwrap();
    queue_flag(&store, OWNER, &message.id, MessageFlag::Seen, true)
        .await
        .unwrap();
    let mut rw = MutableSession::connect(&imap, &user, &pass)
        .await
        .expect("read-write session");
    println!(
        "server capabilities after login: MOVE={} UIDPLUS={}",
        rw.can_move(),
        rw.has_uidplus()
    );
    let flagged = replay(&store, &mut rw, OWNER, ACC).await.unwrap();
    assert!(!flagged.interrupted);
    assert_eq!(
        flagged.done.iter().map(|d| &d.result).collect::<Vec<_>>(),
        vec![&OpResult::Confirmed, &OpResult::Confirmed]
    );
    let mut reader = rw.into_reader();
    let status = reader.examine(&source.remote_name).await.unwrap();
    assert_eq!(status.uid_validity, Some(message.uid_validity));
    let flags = reader
        .fetch_flags(message.imap_uid, message.imap_uid)
        .await
        .unwrap();
    assert_eq!(flags.len(), 1);
    assert!(
        flags[0].seen && flags[0].flagged,
        "the server stored both flags"
    );
    println!("server confirms \\Seen and \\Flagged on the marked message");

    // MB-144: move it to the trash folder.
    let mut rw = MutableSession::upgrade(reader).await.unwrap();
    let mut in_trash = message.id.clone();
    if source.id != trash.id {
        assert_is_ours(&store, &message.id, &marker).await;
        queue_move(
            &store,
            OWNER,
            &message.id,
            MoveTarget::Role(FolderRole::Trash),
        )
        .await
        .unwrap();
        let moved = replay(&store, &mut rw, OWNER, ACC).await.unwrap();
        let OpResult::Moved {
            new_message_id,
            source_left_deleted,
        } = &moved.done[0].result
        else {
            panic!("move was not performed: {:?}", moved.done[0].result)
        };
        println!(
            "moved to trash; new UID reported by server: {}; original left flagged deleted: {source_left_deleted}",
            new_message_id.is_some()
        );
        if !source_left_deleted {
            rw.select(&source.remote_name).await.unwrap();
            assert!(
                !rw.uid_exists(message.imap_uid).await.unwrap(),
                "the message left its original folder"
            );
        }
        in_trash = match new_message_id {
            Some(id) => id.clone(),
            None => {
                // No COPYUID: read the trash folder again and find our marker.
                let mut reader = rw.into_reader();
                mailbox_core::sync::sync_folder(&store, &mut reader, OWNER, &trash, 50)
                    .await
                    .unwrap();
                rw = MutableSession::upgrade(reader).await.unwrap();
                find_marked(&store, &marker)
                    .await
                    .expect("the moved message is in the trash folder")
                    .id
            }
        };
    }
    let row = store.get_message(OWNER, &in_trash).await.unwrap();
    assert_eq!(
        (row.folder_id.as_str(), row.subject.as_str()),
        (trash.id.as_str(), marker.as_str())
    );

    // Clean up: permanently delete exactly that message (UID EXPUNGE of one UID, or flagged
    // deleted when the server has no UIDPLUS).
    assert_is_ours(&store, &in_trash, &marker).await;
    queue_expunge(&store, OWNER, &in_trash).await.unwrap();
    let cleaned = replay(&store, &mut rw, OWNER, ACC).await.unwrap();
    println!("cleanup: {:?}", cleaned.done[0].result);
    assert!(matches!(
        cleaned.done[0].result,
        OpResult::Expunged | OpResult::LeftDeleted
    ));
    if cleaned.done[0].result == OpResult::Expunged {
        rw.select(&trash.remote_name).await.unwrap();
        assert!(
            !rw.uid_exists(row.imap_uid).await.unwrap(),
            "gone from the trash folder"
        );
    }
    rw.into_reader().logout().await;
    assert!(store.list_ops(OWNER, ACC).await.unwrap().is_empty());
    assert!(find_marked(&store, &marker).await.is_none());
}
