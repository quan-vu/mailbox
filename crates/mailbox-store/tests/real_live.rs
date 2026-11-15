//! Owner-only end-to-end check of the live sync worker (MB-140) against a real IMAP/SMTP
//! provider.
//!
//! What it does, and nothing else: starts the worker (one IMAP connection, IDLE when the server
//! has it, otherwise the 2-minute poll), sends ONE message from the test mailbox to itself with a
//! unique marker subject, and waits until the worker has put it into the local cache WITHOUT any
//! manual sync call. Then it permanently deletes exactly that message over the worker's borrowed
//! connection (`UID STORE +FLAGS \Deleted` + `UID EXPUNGE <uid>`; never a plain EXPUNGE), after
//! checking that the cached row carries the marker subject. Prints counts and timings only,
//! never addresses.
//!
//!   MAILBOX_SMTP_ENV_FILE=/abs/path/.env.smtp MAILBOX_SMTP_SEND_TO=self \
//!   MAILBOX_TEST_IMAP_HOST=<imap host> \
//!     cargo test -p mailbox-store --test real_live -- --ignored --nocapture
use mailbox_core::actions::{Expunged, PendingOpStore};
use mailbox_core::live::{AccountLive, LinkState};
use mailbox_core::secrets::Secret;
use mailbox_core::store::{AccountRecord, MailStore, MessageRecord};
use mailbox_core::{MailServerEndpoint, MailboxError, TlsMode};
use mailbox_imap::Session;
use mailbox_imap::live::{Command, Connector, LiveConfig, Tls, worker};
use mailbox_smtp::{SendOutcome, TextMessage, send};
use mailbox_store::SqliteStore;
use std::future::Future;
use std::time::{Duration, Instant};

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

struct EnvConnector {
    imap: MailServerEndpoint,
    user: String,
    pass: std::sync::Arc<Secret>,
}

impl Connector for EnvConnector {
    type Stream = Tls;
    fn connect(&mut self) -> impl Future<Output = Result<Session<Tls>, MailboxError>> + Send {
        let (imap, user, pass) = (self.imap.clone(), self.user.clone(), self.pass.clone());
        async move { Ok(Session::connect(&imap, &user, &pass).await?) }
    }
}

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

async fn until(
    rx: &mut tokio::sync::watch::Receiver<AccountLive>,
    secs: u64,
    f: impl Fn(&AccountLive) -> bool,
) -> AccountLive {
    tokio::time::timeout(Duration::from_secs(secs), rx.wait_for(|s| f(s)))
        .await
        .expect("state reached in time")
        .expect("worker alive")
        .clone()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "owner only: real mailbox; sends one message to the test mailbox itself and removes it"]
async fn real_new_mail_arrives_through_the_worker_without_a_manual_sync() {
    assert_eq!(
        env("MAILBOX_SMTP_SEND_TO").trim(),
        "self",
        "this test only ever sends to the test mailbox itself"
    );
    let user = env("SMTP_USER_1").trim().to_string();
    let pass = std::sync::Arc::new(Secret::new(env("SMTP_PASS_1")));
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

    let started = Instant::now();
    let (h, run) = worker(
        store.clone(),
        OWNER.into(),
        ACC.into(),
        EnvConnector {
            imap: imap.clone(),
            user: user.clone(),
            pass: pass.clone(),
        },
        LiveConfig {
            window: 50,
            ..LiveConfig::default()
        },
    );
    let task = tokio::spawn(run);
    let mut rx = h.status();
    let online = until(&mut rx, 180, |s| matches!(s.state, LinkState::Online(_))).await;
    println!(
        "worker online after {:.1}s in mode {} ({} folder(s) failed)",
        started.elapsed().as_secs_f64(),
        online.state.key(),
        online.failed_folders
    );

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let marker = format!("MailBox MB-140 live test {nanos:x}");
    assert!(find_marked(&store, &marker).await.is_none());
    let outcome = send(
        &smtp,
        &user,
        &pass,
        &TextMessage {
            from: bare_address(&env("SMTP_FROM_1")),
            to: user.clone(),
            subject: marker.clone(),
            body: "Automated MailBox live-sync test message. It is deleted by the same test run.\n"
                .into(),
        },
    )
    .await
    .expect("SMTP session over verified TLS");
    assert_eq!(outcome, SendOutcome::Accepted);
    let sent_at = Instant::now();
    println!("sent 1 marked message to the test mailbox itself");

    // No sync call from here on: only the worker writes to the store.
    let mut arrived = None;
    while sent_at.elapsed() < Duration::from_secs(200) {
        arrived = find_marked(&store, &marker).await;
        if arrived.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let message = arrived.expect("the worker did not deliver the self-sent message within 200 s");
    println!(
        "worker stored it {:.1}s after the send, state {}",
        sent_at.elapsed().as_secs_f64(),
        h.state().state.key()
    );

    // Clean up exactly that message over the worker's own connection.
    let row = store.get_message(OWNER, &message.id).await.unwrap();
    assert_eq!(
        row.subject, marker,
        "refusing to touch a message this test did not create"
    );
    let folder = store
        .list_folders(OWNER, ACC)
        .await
        .unwrap()
        .into_iter()
        .find(|f| f.id == message.folder_id)
        .unwrap();
    {
        let mut rw = h.session().await.expect("worker connection available");
        let selected = rw.select(&folder.remote_name).await.expect("SELECT");
        assert_eq!(
            selected.uid_validity,
            Some(message.uid_validity),
            "UID belongs to this epoch"
        );
        assert!(rw.uid_exists(message.imap_uid).await.unwrap());
        let removed = rw.delete_forever(message.imap_uid).await.expect("delete");
        println!(
            "cleanup: {}",
            if removed == Expunged::Removed {
                "removed with UID EXPUNGE"
            } else {
                "flagged \\Deleted only (server has no UIDPLUS)"
            }
        );
    }
    let logins_before_stop = h.state();
    assert!(store.list_ops(OWNER, ACC).await.unwrap().is_empty());
    h.send(Command::Stop);
    let _ = tokio::time::timeout(Duration::from_secs(60), task).await;
    println!(
        "done; last state {}, generation {}",
        logins_before_stop.state.key(),
        logins_before_stop.generation
    );
}
