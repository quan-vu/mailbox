//! Live sync state (MB-140 / MB-141): what one account's background connection is doing, and
//! the retry policy that keeps it from hammering a server. Pure logic, no I/O: the IMAP driver
//! (`mailbox-imap::live`) feeds it outcomes and the UI renders the result.
//!
//! Rules:
//! - a lost connection, a timeout or a server `BYE` is a state (`Offline`), never an error
//!   dialog; the driver retries with exponential backoff and jitter, capped;
//! - a rejected password, an invalid certificate or a broken local store is `NeedsAttention`:
//!   retrying cannot fix it, so nothing is retried until the user acts (no retry storm);
//! - "online" is only claimed after a login and a sync actually succeeded.
use crate::MailboxError;
use std::time::Duration;

/// Re-issue `IDLE` well before the 29-minute limit of RFC 2177 (servers may drop at 30).
pub const IDLE_REISSUE: Duration = Duration::from_secs(25 * 60);
/// Without `IDLE`, check the inbox this often.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2 * 60);
/// Folders other than the inbox are re-synced at most this often.
pub const OTHER_FOLDERS_EVERY: Duration = Duration::from_secs(10 * 60);
/// First retry delay after a connection loss.
pub const BACKOFF_BASE: Duration = Duration::from_secs(2);
/// Longest wait between two connection attempts.
pub const BACKOFF_CAP: Duration = Duration::from_secs(5 * 60);

/// How an online account learns about new mail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveMode {
    /// The server pushes changes (`IDLE`).
    Idle,
    /// The server has no `IDLE`: the inbox is checked every [`POLL_INTERVAL`].
    Polling,
}

/// Problems that retrying cannot solve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attention {
    /// The server rejected the stored password (or it is missing from the keychain).
    AuthFailed,
    /// The server certificate is not valid for this host. Never bypassed.
    Certificate,
    /// The local database or keychain could not be used.
    Storage,
}

/// Per-account connection state shown in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Connecting,
    Online(LiveMode),
    Syncing,
    /// Not connected; the next attempt starts at `retry_at` (UNIX seconds).
    Offline {
        retry_at: i64,
    },
    NeedsAttention(Attention),
}

impl LinkState {
    /// Stable key for `data-*` attributes and tests.
    pub fn key(self) -> &'static str {
        match self {
            LinkState::Connecting => "connecting",
            LinkState::Online(LiveMode::Idle) => "online-idle",
            LinkState::Online(LiveMode::Polling) => "online-polling",
            LinkState::Syncing => "syncing",
            LinkState::Offline { .. } => "offline",
            LinkState::NeedsAttention(Attention::AuthFailed) => "auth-failed",
            LinkState::NeedsAttention(Attention::Certificate) => "certificate",
            LinkState::NeedsAttention(Attention::Storage) => "storage",
        }
    }

    /// Whether a manual "retry now" makes sense.
    pub fn can_retry(self) -> bool {
        matches!(
            self,
            LinkState::Offline { .. } | LinkState::NeedsAttention(_)
        )
    }
}

/// Everything the UI shows about one account's live sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountLive {
    pub state: LinkState,
    /// UNIX seconds of the last sync that completed.
    pub last_synced: Option<i64>,
    /// Folders that failed in the last full sync while the account itself worked.
    pub failed_folders: usize,
    /// Bumped whenever a sync changed the local cache, so the UI knows to reload.
    pub generation: u64,
}

impl Default for AccountLive {
    fn default() -> Self {
        Self {
            state: LinkState::Connecting,
            last_synced: None,
            failed_folders: 0,
            generation: 0,
        }
    }
}

/// What a failed step means for the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// Try again later (network, timeout, server hiccup, protocol trouble).
    Transient,
    /// Wait for the user.
    Attention(Attention),
}

/// Map a core error to what the worker should do about it.
pub fn classify(e: &MailboxError) -> Fault {
    match e {
        MailboxError::ReauthenticationRequired => Fault::Attention(Attention::AuthFailed),
        MailboxError::Storage("certificate_invalid") | MailboxError::Storage("tls_failed") => {
            Fault::Attention(Attention::Certificate)
        }
        // Includes a stored host that no longer validates: waiting does not fix that either.
        MailboxError::Storage(_) => Fault::Attention(Attention::Storage),
        MailboxError::TransientConnectionFailure
        | MailboxError::InconsistentData(_)
        | MailboxError::UidValidityChanged
        | MailboxError::NotFound
        | MailboxError::NotImplemented => Fault::Transient,
    }
}

/// Exponential backoff with "equal jitter": attempt `n` waits a random time in
/// `[d/2, d]` where `d = min(cap, base * 2^n)`. Jitter spreads reconnects of many clients;
/// the lower bound guarantees there is never a tight loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    pub base: Duration,
    pub cap: Duration,
    attempt: u32,
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new(BACKOFF_BASE, BACKOFF_CAP)
    }
}

impl Backoff {
    pub fn new(base: Duration, cap: Duration) -> Self {
        Self {
            base,
            cap,
            attempt: 0,
        }
    }

    /// Attempts since the last success.
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// Delay before the next attempt. `jitter` is a uniform sample in `[0, 1)`.
    pub fn next_delay(&mut self, jitter: f64) -> Duration {
        let factor = 1u32.checked_shl(self.attempt.min(20)).unwrap_or(u32::MAX);
        let full = self.base.saturating_mul(factor).min(self.cap);
        self.attempt = self.attempt.saturating_add(1);
        let jitter = if jitter.is_finite() {
            jitter.clamp(0.0, 1.0)
        } else {
            0.0
        };
        full / 2 + (full / 2).mul_f64(jitter)
    }

    /// A connection and sync succeeded: start again from `base`.
    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_within_jitter_bounds_and_is_capped() {
        let mut b = Backoff::new(Duration::from_secs(2), Duration::from_secs(300));
        let lows: Vec<u64> = (0..10).map(|_| b.next_delay(0.0).as_secs()).collect();
        assert_eq!(lows, vec![1, 2, 4, 8, 16, 32, 64, 128, 150, 150]);
        let mut b = Backoff::new(Duration::from_secs(2), Duration::from_secs(300));
        let highs: Vec<u64> = (0..10)
            .map(|_| b.next_delay(0.999_999).as_secs_f64().round() as u64)
            .collect();
        assert_eq!(highs, vec![2, 4, 8, 16, 32, 64, 128, 256, 300, 300]);
    }

    #[test]
    fn backoff_never_returns_zero_and_survives_bad_jitter_and_many_attempts() {
        let mut b = Backoff::default();
        for _ in 0..1_000 {
            let d = b.next_delay(f64::NAN);
            assert!(d >= BACKOFF_BASE / 2 && d <= BACKOFF_CAP);
        }
        b.reset();
        assert_eq!(b.attempt(), 0);
        assert_eq!(b.next_delay(-5.0), Duration::from_secs(1));
    }

    #[test]
    fn faults_split_into_retry_and_wait_for_user() {
        assert_eq!(
            classify(&MailboxError::TransientConnectionFailure),
            Fault::Transient
        );
        assert_eq!(
            classify(&MailboxError::InconsistentData("imap protocol")),
            Fault::Transient
        );
        assert_eq!(
            classify(&MailboxError::ReauthenticationRequired),
            Fault::Attention(Attention::AuthFailed)
        );
        assert_eq!(
            classify(&MailboxError::Storage("certificate_invalid")),
            Fault::Attention(Attention::Certificate)
        );
        assert_eq!(
            classify(&MailboxError::Storage("sqlite")),
            Fault::Attention(Attention::Storage)
        );
    }

    #[test]
    fn state_keys_are_distinct_and_retry_is_offered_only_when_useful() {
        let all = [
            LinkState::Connecting,
            LinkState::Online(LiveMode::Idle),
            LinkState::Online(LiveMode::Polling),
            LinkState::Syncing,
            LinkState::Offline { retry_at: 5 },
            LinkState::NeedsAttention(Attention::AuthFailed),
            LinkState::NeedsAttention(Attention::Certificate),
            LinkState::NeedsAttention(Attention::Storage),
        ];
        let keys: std::collections::BTreeSet<_> = all.iter().map(|s| s.key()).collect();
        assert_eq!(keys.len(), all.len());
        let retry: Vec<_> = all
            .iter()
            .filter(|s| s.can_retry())
            .map(|s| s.key())
            .collect();
        assert_eq!(
            retry,
            vec!["offline", "auth-failed", "certificate", "storage"]
        );
        assert_eq!(AccountLive::default().state, LinkState::Connecting);
        assert!(IDLE_REISSUE < Duration::from_secs(29 * 60));
    }
}
