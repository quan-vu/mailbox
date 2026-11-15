//! Live sync worker (MB-140): one long-lived IMAP session per account.
//!
//! The worker owns the connection and keeps the local cache current:
//! - on connect: replay the pending-operation queue once, then sync every folder;
//! - then `IDLE` on INBOX when the server has it (re-issued every [`LiveConfig::idle_reissue`]),
//!   otherwise poll every [`LiveConfig::poll`]; a change runs the incremental sync and flag pull
//!   of INBOX, other folders follow on a slower rotation ([`LiveConfig::other_folders_every`]);
//! - a lost connection, timeout or `BYE` becomes `Offline` with a capped, jittered backoff;
//!   a rejected password, bad certificate or broken store becomes `NeedsAttention` and nothing
//!   is retried until the user asks (no retry storm).
//!
//! Other use cases (queued flag/move actions, body fetch) borrow the same session through
//! [`LiveHandle::session`] instead of paying a new TLS handshake each. A borrower that arrives
//! while the worker idles makes it send `DONE` first and hands over the connection.
//!
//! Sync never fights the user: the flag pull keeps local values that have a pending operation
//! (`mailbox_core::actions::pull_flags`), and a changed UIDVALIDITY rebuilds only that folder
//! through `mailbox_core::sync::sync_folder`.
use crate::idle::IdleEnd;
use crate::mutate::MutableSession;
use crate::{ConnectError, Session};
use mailbox_core::MailboxError;
use mailbox_core::actions::{PendingOpStore, ReplayReport, pull_flags, replay};
use mailbox_core::folders::{FolderRole, FolderRoleStore, role_of, sync_all_folders};
use mailbox_core::live::{
    AccountLive, BACKOFF_BASE, BACKOFF_CAP, Backoff, Fault, IDLE_REISSUE, LinkState, LiveMode,
    OTHER_FOLDERS_EVERY, POLL_INTERVAL, classify,
};
use mailbox_core::store::{FolderRecord, MailStore};
use mailbox_core::sync::{SyncKind, sync_folder};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{Mutex, MutexGuard, Notify, mpsc, watch};

/// The verified TLS stream real sessions run on.
pub type Tls = mailbox_net::Tls;

/// Opens a logged-in session. The UI's connector reads the password from the OS keychain at
/// every attempt, so a password the user re-entered is picked up by "retry now".
pub trait Connector: Send {
    type Stream: AsyncRead + AsyncWrite + Unpin + Send;
    fn connect(
        &mut self,
    ) -> impl Future<Output = Result<Session<Self::Stream>, MailboxError>> + Send;
}

/// Requests from the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Connect now instead of waiting for the backoff (or after the user fixed a problem).
    RetryNow,
    /// Replay queued operations and sync every folder now.
    SyncNow,
    /// Log out and end the worker.
    Stop,
}

#[derive(Debug, Clone)]
pub struct LiveConfig {
    pub idle_reissue: Duration,
    pub poll: Duration,
    pub other_folders_every: Duration,
    pub backoff_base: Duration,
    pub backoff_cap: Duration,
    /// Envelopes fetched on first sync / rebuild of a folder.
    pub window: u32,
    pub max_folders: usize,
    /// Uniform sample in `[0, 1)` for backoff jitter.
    pub jitter: fn() -> f64,
    /// Called with every replay report (the UI keeps undo traces of moves).
    pub on_replay: fn(&ReplayReport),
    /// Current UNIX time in seconds (for `Offline::retry_at` and `last_synced`).
    pub now: fn() -> i64,
}

fn system_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Cheap, non-cryptographic jitter source: it only spreads reconnects in time.
fn clock_jitter() -> f64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    f64::from(nanos % 1_000_000) / 1_000_000.0
}

impl Default for LiveConfig {
    fn default() -> Self {
        Self {
            idle_reissue: IDLE_REISSUE,
            poll: POLL_INTERVAL,
            other_folders_every: OTHER_FOLDERS_EVERY,
            backoff_base: BACKOFF_BASE,
            backoff_cap: BACKOFF_CAP,
            window: 100,
            max_folders: 64,
            jitter: clock_jitter,
            on_replay: |_| {},
            now: system_now,
        }
    }
}

struct Shared<S> {
    slot: Mutex<Option<MutableSession<S>>>,
    /// Borrowers waiting for the slot; the worker gives way before it starts to idle.
    waiting: AtomicUsize,
    wants: Notify,
    /// The worker owns the sender: once it ends, every receiver sees the channel closed.
    status: watch::Receiver<AccountLive>,
    commands: mpsc::UnboundedSender<Command>,
}

/// The UI's side of one account's worker. Cheap to clone.
pub struct LiveHandle<S> {
    shared: Arc<Shared<S>>,
}

impl<S> Clone for LiveHandle<S> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}

/// A borrowed connection. Dropping it hands the connection back to the worker.
pub struct Borrowed<'a, S> {
    guard: MutexGuard<'a, Option<MutableSession<S>>>,
    commands: &'a mpsc::UnboundedSender<Command>,
}

impl<S> std::ops::Deref for Borrowed<'_, S> {
    type Target = MutableSession<S>;
    fn deref(&self) -> &Self::Target {
        // Only constructed around a present session.
        self.guard.as_ref().expect("borrowed session present")
    }
}

impl<S> std::ops::DerefMut for Borrowed<'_, S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.as_mut().expect("borrowed session present")
    }
}

impl<S> Borrowed<'_, S> {
    /// The connection broke or may be out of step with the server: drop it instead of giving it
    /// back, and make the worker reconnect.
    pub fn discard(mut self) {
        self.guard.take();
        let _ = self.commands.send(Command::RetryNow);
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> LiveHandle<S> {
    /// State updates; `changed()` fails once the worker has ended.
    pub fn status(&self) -> watch::Receiver<AccountLive> {
        self.shared.status.clone()
    }

    pub fn state(&self) -> AccountLive {
        self.shared.status.borrow().clone()
    }

    /// Returns false when the worker has ended.
    pub fn send(&self, command: Command) -> bool {
        self.shared.commands.send(command).is_ok()
    }

    /// Borrow the account's connection; `None` while it is not connected. If the worker is
    /// idling it ends the idle (`DONE`) first. Hold the guard only for the commands you need.
    pub async fn session(&self) -> Option<Borrowed<'_, S>> {
        self.shared.waiting.fetch_add(1, Ordering::SeqCst);
        self.shared.wants.notify_waiters();
        let guard = self.shared.slot.lock().await;
        self.shared.waiting.fetch_sub(1, Ordering::SeqCst);
        guard.is_some().then(|| Borrowed {
            guard,
            commands: &self.shared.commands,
        })
    }
}

/// Create a worker for one account. Run the returned future (e.g. `tokio::spawn`) and keep the
/// handle to borrow the session, read the status and send commands.
pub fn worker<St, C>(
    store: St,
    owner_id: String,
    account_id: String,
    connector: C,
    config: LiveConfig,
) -> (LiveHandle<C::Stream>, impl Future<Output = ()> + use<St, C>)
where
    St: MailStore + FolderRoleStore + PendingOpStore,
    C: Connector,
{
    let (commands, rx) = mpsc::unbounded_channel();
    let (status, status_rx) = watch::channel(AccountLive::default());
    let shared = Arc::new(Shared {
        slot: Mutex::new(None),
        waiting: AtomicUsize::new(0),
        wants: Notify::new(),
        status: status_rx,
        commands,
    });
    let handle = LiveHandle {
        shared: shared.clone(),
    };
    let run = async move {
        let mut w = Worker {
            shared,
            status,
            store,
            owner_id,
            account_id,
            connector,
            config,
            rx,
            backoff: Backoff::new(BACKOFF_BASE, BACKOFF_CAP),
        };
        w.backoff = Backoff::new(w.config.backoff_base, w.config.backoff_cap);
        w.run().await;
    };
    (handle, run)
}

struct Worker<St, C: Connector> {
    shared: Arc<Shared<C::Stream>>,
    status: watch::Sender<AccountLive>,
    store: St,
    owner_id: String,
    account_id: String,
    connector: C,
    config: LiveConfig,
    rx: mpsc::UnboundedReceiver<Command>,
    backoff: Backoff,
}

/// What woke the online loop.
enum Wake {
    Changed,
    Timer,
    Command(Option<Command>),
    /// A borrower wanted the connection.
    GiveWay,
    /// The server refused IDLE on this connection: poll from now on.
    NoIdle,
}

/// Result of one sync step.
struct Synced {
    changed: bool,
    failed_folders: usize,
}

impl<St, C> Worker<St, C>
where
    St: MailStore + FolderRoleStore + PendingOpStore,
    C: Connector,
{
    fn set(&self, state: LinkState) {
        self.status.send_modify(|s| s.state = state);
    }

    fn synced(&self, synced: &Synced, full: bool) {
        let now = (self.config.now)();
        self.status.send_modify(|s| {
            s.last_synced = Some(now);
            if full {
                s.failed_folders = synced.failed_folders;
            }
            if synced.changed {
                s.generation += 1;
            }
        });
    }

    async fn run(&mut self) {
        loop {
            self.set(LinkState::Connecting);
            let connected = match self.connector.connect().await {
                Ok(session) => MutableSession::upgrade(session)
                    .await
                    .map_err(MailboxError::from),
                Err(e) => Err(e),
            };
            let session = match connected {
                Ok(s) => s,
                Err(e) => {
                    if self.wait_after(e).await {
                        continue;
                    }
                    return;
                }
            };
            let mut mode = if session.reader().supports_idle() {
                LiveMode::Idle
            } else {
                LiveMode::Polling
            };
            *self.shared.slot.lock().await = Some(session);
            match self.online(&mut mode).await {
                Ok(()) => {
                    // Stop requested: leave politely.
                    if let Some(s) = self.shared.slot.lock().await.take() {
                        s.into_reader().logout().await;
                    }
                    return;
                }
                Err(e) => {
                    // The connection is no longer usable; never reuse it.
                    self.shared.slot.lock().await.take();
                    if !self.wait_after(e).await {
                        return;
                    }
                }
            }
        }
    }

    /// Back off after a failure. Returns false when the worker should end.
    async fn wait_after(&mut self, e: MailboxError) -> bool {
        match classify(&e) {
            Fault::Transient => {
                let delay = self.backoff.next_delay((self.config.jitter)());
                let retry_at = (self.config.now)() + delay.as_secs() as i64;
                self.set(LinkState::Offline { retry_at });
                tokio::select! {
                    () = tokio::time::sleep(delay) => true,
                    cmd = self.rx.recv() => !matches!(cmd, None | Some(Command::Stop)),
                }
            }
            Fault::Attention(why) => {
                self.set(LinkState::NeedsAttention(why));
                // Only the user can fix this: wait for them, however long it takes.
                match self.rx.recv().await {
                    Some(Command::RetryNow | Command::SyncNow) => {
                        self.backoff.reset();
                        true
                    }
                    Some(Command::Stop) | None => false,
                }
            }
        }
    }

    /// The connected part. `Ok` means stop was requested; `Err` means the connection failed.
    async fn online(&mut self, mode: &mut LiveMode) -> Result<(), MailboxError> {
        self.set(LinkState::Syncing);
        let synced = self.full_sync().await?;
        self.backoff.reset();
        self.synced(&synced, true);
        let mut last_full = tokio::time::Instant::now();
        loop {
            self.set(LinkState::Online(*mode));
            let wake = match mode {
                LiveMode::Idle => {
                    // Wake for the other-folder rotation too, and never idle past the re-issue.
                    let until_full = self
                        .config
                        .other_folders_every
                        .saturating_sub(last_full.elapsed());
                    self.idle_wait(self.config.idle_reissue.min(until_full))
                        .await?
                }
                LiveMode::Polling => self.poll_wait().await,
            };
            let full_due = last_full.elapsed() >= self.config.other_folders_every;
            let full = match wake {
                Wake::Command(None | Some(Command::Stop)) => return Ok(()),
                Wake::Command(Some(Command::SyncNow | Command::RetryNow)) => true,
                Wake::GiveWay => {
                    // Let the borrower in (the mutex is fair), then carry on.
                    tokio::task::yield_now().await;
                    continue;
                }
                Wake::NoIdle => {
                    *mode = LiveMode::Polling;
                    continue;
                }
                Wake::Changed | Wake::Timer => full_due,
            };
            self.set(LinkState::Syncing);
            let synced = if full {
                last_full = tokio::time::Instant::now();
                self.full_sync().await?
            } else {
                self.inbox_sync().await?
            };
            self.synced(&synced, full);
        }
    }

    /// Replay queued operations, then sync every folder with its flags.
    async fn full_sync(&mut self) -> Result<Synced, MailboxError> {
        let mut slot = self.shared.slot.lock().await;
        let session = slot
            .as_mut()
            .ok_or(MailboxError::TransientConnectionFailure)?;
        let replayed = replay(&self.store, session, &self.owner_id, &self.account_id).await?;
        (self.config.on_replay)(&replayed);
        if replayed.interrupted {
            return Err(MailboxError::TransientConnectionFailure);
        }
        let report = sync_all_folders(
            &self.store,
            session.reader_mut(),
            &self.owner_id,
            &self.account_id,
            self.config.window,
            self.config.max_folders,
        )
        .await?;
        let mut changed = !replayed.done.is_empty() || report.stored() > 0;
        for folder in &report.folders {
            match &folder.result {
                // Once the connection is gone every folder after it failed the same way.
                Err(MailboxError::TransientConnectionFailure) => {
                    return Err(MailboxError::TransientConnectionFailure);
                }
                // The account cannot work without its inbox.
                Err(e) if folder.role == Some(FolderRole::Inbox) => return Err(clone_err(e)),
                Ok(r) => changed |= r.kind == SyncKind::Rebuilt,
                Err(_) => {}
            }
            if let Some(f) = &folder.flags {
                changed |= f.changed > 0 || f.vanished > 0;
            }
        }
        Ok(Synced {
            changed,
            failed_folders: report.failed(),
        })
    }

    async fn inbox(&self) -> Result<FolderRecord, MailboxError> {
        self.store
            .list_folders(&self.owner_id, &self.account_id)
            .await?
            .into_iter()
            .find(|f| role_of(f) == Some(FolderRole::Inbox))
            .ok_or(MailboxError::NotFound)
    }

    /// Incremental sync and flag pull of INBOX only.
    async fn inbox_sync(&mut self) -> Result<Synced, MailboxError> {
        let folder = self.inbox().await?;
        let mut slot = self.shared.slot.lock().await;
        let session = slot
            .as_mut()
            .ok_or(MailboxError::TransientConnectionFailure)?;
        let reader = session.reader_mut();
        let report = sync_folder(
            &self.store,
            reader,
            &self.owner_id,
            &folder,
            self.config.window,
        )
        .await?;
        let mut changed = report.stored > 0 || report.kind == SyncKind::Rebuilt;
        if let Some(cursor) = self
            .store
            .load_cursor(&self.owner_id, &self.account_id, &folder.id)
            .await?
        {
            let pull = pull_flags(
                &self.store,
                reader,
                &self.owner_id,
                &folder,
                cursor.uid_validity,
            )
            .await?;
            changed |= pull.changed > 0 || pull.vanished > 0;
        }
        Ok(Synced {
            changed,
            failed_folders: 0,
        })
    }

    /// Hold the session in IDLE on INBOX until something happens.
    async fn idle_wait(&mut self, max: Duration) -> Result<Wake, MailboxError> {
        let folder = self.inbox().await?;
        let shared = self.shared.clone();
        let wants = shared.wants.notified();
        let mut wants = std::pin::pin!(wants);
        // Register before looking at `waiting`, so a borrower arriving now is never missed.
        wants.as_mut().enable();
        if shared.waiting.load(Ordering::SeqCst) > 0 {
            return Ok(Wake::GiveWay);
        }
        let mut slot = shared.slot.lock().await;
        let session = slot
            .as_mut()
            .ok_or(MailboxError::TransientConnectionFailure)?;
        let reader = session.reader_mut();
        // Mail that arrived between the last sync and now would not be announced by IDLE.
        let status = reader.examine(&folder.remote_name).await?;
        if status.uid_validity != folder.uid_validity || status.uid_next != folder.uid_next {
            return Ok(Wake::Changed);
        }
        let rx = &mut self.rx;
        let outcome = reader
            .idle(max, async {
                tokio::select! {
                    () = wants => None,
                    cmd = rx.recv() => Some(cmd),
                }
            })
            .await?;
        Ok(match outcome.end {
            IdleEnd::Changed => Wake::Changed,
            IdleEnd::TimedOut => Wake::Timer,
            IdleEnd::Refused => Wake::NoIdle,
            IdleEnd::Interrupted(None) => Wake::GiveWay,
            IdleEnd::Interrupted(Some(cmd)) => Wake::Command(cmd),
        })
    }

    /// Without IDLE: wait for the poll interval or a command. The session is free meanwhile.
    async fn poll_wait(&mut self) -> Wake {
        tokio::select! {
            () = tokio::time::sleep(self.config.poll) => Wake::Timer,
            cmd = self.rx.recv() => Wake::Command(cmd),
        }
    }
}

fn clone_err(e: &MailboxError) -> MailboxError {
    match e {
        MailboxError::ReauthenticationRequired => MailboxError::ReauthenticationRequired,
        MailboxError::TransientConnectionFailure => MailboxError::TransientConnectionFailure,
        MailboxError::UidValidityChanged => MailboxError::UidValidityChanged,
        MailboxError::NotImplemented => MailboxError::NotImplemented,
        MailboxError::InconsistentData(s) => MailboxError::InconsistentData(s),
        MailboxError::NotFound => MailboxError::NotFound,
        MailboxError::Storage(s) => MailboxError::Storage(s),
    }
}

impl From<ConnectError> for Fault {
    fn from(e: ConnectError) -> Self {
        classify(&MailboxError::from(e))
    }
}
