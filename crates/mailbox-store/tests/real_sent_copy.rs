//! Owner-only check (track G) that the Sent-folder copy goes through the live worker's held
//! IMAP connection (`mailbox_imap::borrow::live_or_fresh`) on a real provider.
//!
//! What it does, and nothing else: starts the worker, waits until it is online, APPENDs ONE
//! message created by this test (unique `Message-ID` and marker subject; no mail is sent) to the
//! account's Sent folder over the borrowed connection, runs the same job again to show that no
//! second copy is made, then permanently deletes exactly that UID (checked by `Message-ID` and
//! UIDVALIDITY). Prints counts and routes only, never addresses.
//!
//!   MAILBOX_SMTP_ENV_FILE=/abs/path/.env.smtp MAILBOX_SMTP_SEND_TO=self \
//!   MAILBOX_TEST_IMAP_HOST=<imap host> \
//!     cargo test -p mailbox-store --test real_sent_copy -- --ignored --nocapture
use mailbox_core::live::{AccountLive, LinkState};
use mailbox_core::secrets::Secret;
use mailbox_core::store::{AccountRecord, MailStore};
use mailbox_core::{MailServerEndpoint, MailboxError, TlsMode};
use mailbox_imap::Session;
use mailbox_imap::append::{AppendError, FolderRole, SentCopy, find_folder, save_sent_copy};
use mailbox_imap::borrow::{RepeatableJob, Route, live_or_fresh};
use mailbox_imap::live::{Command, Connector, LiveConfig, Tls, worker};
use mailbox_store::SqliteStore;
use std::future::Future;
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

#[allow(dead_code)]
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

struct CopyJob {
    folder: String,
    message_id: String,
    bytes: Vec<u8>,
}

impl RepeatableJob for CopyJob {
    type Output = SentCopy;
    async fn run<'a, S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'a>(
        &'a self,
        session: &'a mut Session<S>,
    ) -> Result<SentCopy, AppendError> {
        save_sent_copy(session, &self.folder, &self.message_id, &self.bytes).await
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "owner only: real mailbox; appends one test-made message to Sent and removes it"]
async fn real_sent_copy_uses_the_worker_connection_and_makes_one_copy() {
    assert_eq!(
        env("MAILBOX_SMTP_SEND_TO").trim(),
        "self",
        "test mailbox only"
    );
    let user = env("SMTP_USER_1").trim().to_string();
    let pass = std::sync::Arc::new(Secret::new(env("SMTP_PASS_1")));
    let imap = MailServerEndpoint {
        hostname: std::env::var("MAILBOX_TEST_IMAP_HOST")
            .unwrap_or_else(|_| env("SMTP_HOST_1"))
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
            smtp_host: imap.hostname.clone(),
            smtp_port: 465,
            auth_type: "password".into(),
            credential_ref: "imap.acc".into(),
            status: "active".into(),
        })
        .await
        .unwrap();
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
            window: 20,
            ..LiveConfig::default()
        },
    );
    let task = tokio::spawn(run);
    let mut rx = h.status();
    until(&mut rx, 180, |s| matches!(s.state, LinkState::Online(_))).await;
    let generation_online = h.state().generation;

    let sent = {
        let mut rw = h.session().await.expect("worker connection");
        let folders = rw.reader_mut().list_folders().await.expect("LIST");
        find_folder(&folders, FolderRole::Sent)
            .map(|f| f.name.clone())
            .expect("the test account has a Sent folder")
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let message_id = format!("<mb-trackg-{nanos:x}@mailbox.invalid>");
    let marker = format!("MailBox track G sent-copy test {nanos:x}");
    let bytes = format!(
        "Message-ID: {message_id}\r\nFrom: test@mailbox.invalid\r\nTo: test@mailbox.invalid\r\nSubject: {marker}\r\nDate: Sun, 15 Nov 2026 00:00:00 +0000\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nAutomated MailBox test copy. Deleted by the same run.\r\n"
    )
    .into_bytes();
    let job = CopyJob {
        folder: sent.clone(),
        message_id: message_id.clone(),
        bytes,
    };
    let never = || async { Err::<Session<Tls>, _>(AppendError::Refused) };
    let (first, route) = live_or_fresh(Some(&h), &job, never).await;
    println!(
        "first run: {:?} via {route:?}",
        first.map(|c| matches!(c, SentCopy::Appended { .. }))
    );
    assert_eq!(route, Route::Live, "the held connection did the work");
    assert!(matches!(first, Ok(SentCopy::Appended { .. })));
    let (second, route) = live_or_fresh(Some(&h), &job, never).await;
    println!(
        "second run: already there = {}",
        second == Ok(SentCopy::AlreadyThere)
    );
    assert_eq!((second, route), (Ok(SentCopy::AlreadyThere), Route::Live));

    // Clean up exactly the message this test made.
    {
        let mut rw = h.session().await.expect("worker connection");
        let uids = rw
            .reader_mut()
            .find_by_message_id(&sent, &message_id)
            .await
            .expect("search");
        assert_eq!(uids.len(), 1, "exactly one copy, made by this test");
        let selected = rw.select(&sent).await.expect("SELECT");
        assert!(selected.uid_validity.is_some());
        assert!(rw.uid_exists(uids[0]).await.unwrap());
        let removed = rw.delete_forever(uids[0]).await.expect("delete");
        println!("cleanup: {removed:?}");
    }
    println!(
        "worker state {}, generation {} -> {}",
        h.state().state.key(),
        generation_online,
        h.state().generation
    );
    h.send(Command::Stop);
    let _ = tokio::time::timeout(Duration::from_secs(60), task).await;
}
