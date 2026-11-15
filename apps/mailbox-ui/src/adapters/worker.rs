//! Background sync workers for real accounts on Desktop (MB-140). One long-lived IMAP session
//! per account (`mailbox_imap::live`); queued actions and body fetches borrow it.
//!
//! Workers run on the tokio runtime of the Desktop app, independent of any component. The UI
//! learns about state changes through [`changed`] and reads them with [`snapshot`]. Nothing here
//! exists in the Web build.
use super::account::{OWNER, WINDOW, open_store};
use mailbox_core::live::{AccountLive, Attention, LinkState};
use mailbox_core::secrets::SecretStore;
use mailbox_core::store::MailStore;
use mailbox_core::{MailServerEndpoint, MailboxError, TlsMode};
use mailbox_imap::Session;
use mailbox_imap::live::{Command, Connector, LiveConfig, LiveHandle, Tls, worker};
use mailbox_secrets::KeychainStore;
use mailbox_store::SqliteStore;
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Mutex;
use tokio::sync::Notify;

pub type Handle = LiveHandle<Tls>;

static WORKERS: Mutex<BTreeMap<String, Handle>> = Mutex::new(BTreeMap::new());
/// The stored "Đồng bộ tự động" setting. Off: no worker runs; manual sync uses its own connection.
static AUTO_SYNC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Apply the auto-sync setting. Returns true when it changed (the caller then calls
/// [`ensure_running`], which starts or stops the workers).
pub fn set_auto_sync(on: bool) -> bool {
    AUTO_SYNC.swap(on, std::sync::atomic::Ordering::SeqCst) != on
}

pub fn auto_sync() -> bool {
    AUTO_SYNC.load(std::sync::atomic::Ordering::SeqCst)
}
static CHANGED: Notify = Notify::const_new();

/// Logs in with the account's current keychain secret at every attempt, so a password the
/// user re-entered through the connect form is used by the next "retry now".
struct KeychainConnector {
    store: SqliteStore,
    account_id: String,
}

impl Connector for KeychainConnector {
    type Stream = Tls;
    fn connect(&mut self) -> impl Future<Output = Result<Session<Tls>, MailboxError>> + Send {
        let (store, account_id) = (self.store.clone(), self.account_id.clone());
        async move {
            let account = store.get_account(OWNER, &account_id).await?;
            let password = KeychainStore::default()
                .get(&account.credential_ref)
                .map_err(|e| match e {
                    // The user removed the keychain entry: only a new password helps.
                    MailboxError::NotFound => MailboxError::ReauthenticationRequired,
                    _ => MailboxError::Storage("keychain"),
                })?;
            let endpoint = MailServerEndpoint {
                hostname: account.imap_host.clone(),
                port: account.imap_port,
                tls_mode: TlsMode::ImplicitTls,
            };
            Ok(Session::connect(&endpoint, &account.email_address, &password).await?)
        }
    }
}

fn workers() -> std::sync::MutexGuard<'static, BTreeMap<String, Handle>> {
    WORKERS.lock().unwrap_or_else(|e| e.into_inner())
}

/// The running worker of an account, if any.
pub fn handle(account_id: &str) -> Option<Handle> {
    workers().get(account_id).cloned()
}

/// Wait until some worker's state changed (or a worker started or stopped).
pub async fn changed() {
    CHANGED.notified().await;
}

/// Current state of every running worker.
pub fn snapshot() -> BTreeMap<String, AccountLive> {
    workers()
        .iter()
        .map(|(id, h)| (id.clone(), h.state()))
        .collect()
}

/// Start a worker for every saved account that has none, stop workers of removed accounts.
/// With auto-sync off, stop every worker instead.
pub async fn ensure_running() {
    if !auto_sync() {
        stop_all();
        return;
    }
    let Ok(store) = open_store().await else {
        return;
    };
    let Ok(accounts) = store.list_accounts(OWNER).await else {
        return;
    };
    let ids: Vec<String> = accounts.into_iter().map(|a| a.id).collect();
    let mut map = workers();
    map.retain(|id, h| {
        let keep = ids.contains(id);
        if !keep {
            h.send(Command::Stop);
        }
        keep
    });
    for id in ids {
        if map.contains_key(&id) {
            continue;
        }
        let config = LiveConfig {
            window: WINDOW,
            on_replay: super::actions::remember,
            ..LiveConfig::default()
        };
        let connector = KeychainConnector {
            store: store.clone(),
            account_id: id.clone(),
        };
        let (handle, run) = worker(store.clone(), OWNER.into(), id.clone(), connector, config);
        let mut status = handle.status();
        tokio::spawn(run);
        // Forward state changes to the UI.
        tokio::spawn(async move {
            while status.changed().await.is_ok() {
                CHANGED.notify_one();
            }
            CHANGED.notify_one();
        });
        map.insert(id, handle);
    }
    CHANGED.notify_one();
}

/// Stop every worker (leaving Real mode).
pub fn stop_all() {
    for (_, h) in std::mem::take(&mut *workers()) {
        h.send(Command::Stop);
    }
    CHANGED.notify_one();
}

/// "Retry now" for one account.
pub fn retry_now(account_id: &str) {
    if let Some(h) = handle(account_id) {
        h.send(Command::RetryNow);
    }
}

/// Ask every worker for a full sync. Returns false when no worker runs.
pub fn sync_all() -> bool {
    let map = workers();
    for h in map.values() {
        h.send(Command::SyncNow);
    }
    !map.is_empty()
}

/// Why an account's server cannot be used right now, from its worker's state.
pub fn offline_reason(state: LinkState) -> super::connect::CheckFailure {
    use super::connect::CheckFailure;
    match state {
        LinkState::NeedsAttention(Attention::AuthFailed) => CheckFailure::AuthFailed,
        LinkState::NeedsAttention(Attention::Certificate) => CheckFailure::CertificateInvalid,
        LinkState::NeedsAttention(Attention::Storage) => CheckFailure::Storage,
        _ => CheckFailure::Unreachable,
    }
}

/// What a full manual sync does after its IMAP work, for changes a live worker made: recompute
/// conversations, index the new mail for search, and announce new mail (MB-200/201/211).
/// `known` holds the message ids seen per account; an account seen for the first time only gets
/// its baseline, so mail already in the cache is never announced. Best effort throughout.
pub async fn after_change(
    account_ids: &[String],
    known: &mut BTreeMap<String, std::collections::BTreeSet<String>>,
) {
    let Ok(store) = super::account::open_store().await else {
        return;
    };
    for id in account_ids {
        let Ok(account) = store.get_account(OWNER, id).await else {
            continue;
        };
        let _ = mailbox_core::threading::rethread_account(
            &store,
            OWNER,
            id,
            std::slice::from_ref(&account.email_address),
        )
        .await;
        if let Some(before) = known.get(id) {
            super::notify::after_sync(&store, id, before).await;
        }
        known.insert(id.clone(), super::notify::snapshot(&store, id).await);
    }
    super::history::index_after_sync(&store).await;
}
