//! Drafts and the outbox state machine (MB-133, MB-151), with scheduled send (MB-218).
//!
//! ```text
//! Draft -> Queued -> Sending -> Sent
//!   ^  \      ^                \-> Failed { reason, retryable }
//!   |   v     | Due              \-> Unknown
//!   +- Scheduled -- Missed --> Failed { MissedSchedule, retryable }
//! ```
//!
//! Scheduled send: the message waits in the outbox with a UTC send time and is sent by the
//! running app when due ([`due_action`]). Content cannot change while scheduled: editing first
//! takes it back to `Draft` ([`unschedule`]). A message whose time passed more than
//! [`MISSED_GRACE_SECS`] ago (app closed, computer asleep) is not sent on its own: it becomes a
//! retryable failure and waits for the user.
//!
//! Safe-sending rules, enforced here and covered by the tests below:
//! - `Sending` is written to the store *before* the transport is called. If the process dies
//!   in between, the next start finds `Sending` and [`recover`] turns it into `Unknown`: the
//!   message may or may not have left, and only the user can decide what happens next.
//! - Nothing in this module sends on its own. [`recover`] has no transport at all, and
//!   [`submit`] refuses an `Unknown` message unless the caller passes
//!   [`SendIntent::ResendAfterUnknown`], which the UI only does after an explicit confirmation.
//! - State changes are compare-and-set in the store, so two clicks cannot start two sends.
//! - The `Message-ID` belongs to the draft and never changes, so a duplicate is detectable.
use crate::MailboxError;

/// Why a message was not sent. Short stable keys; never server text or addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailReason {
    /// The server refused the message itself.
    Rejected,
    /// The server refused one or more recipients; nothing was sent.
    RecipientsRejected,
    AuthFailed,
    /// Could not reach the server, or the connection broke before the message was handed over.
    Network,
    Certificate,
    TooLarge,
    /// The server lacks an extension the message needs (e.g. international addresses).
    Unsupported,
    /// The draft cannot be turned into a valid message (address, header, recipients).
    InvalidMessage,
    AttachmentMissing,
    AttachmentsTooLarge,
    /// The From address is not the identity of the sending account.
    IdentityMismatch,
    /// The app closed while the message was still waiting; it was never handed over.
    Interrupted,
    Storage,
    /// The scheduled time passed while the app could not send (closed, asleep) by more than
    /// the grace period; nothing was handed over.
    MissedSchedule,
}

impl FailReason {
    pub const ALL: [FailReason; 14] = [
        FailReason::Rejected,
        FailReason::RecipientsRejected,
        FailReason::AuthFailed,
        FailReason::Network,
        FailReason::Certificate,
        FailReason::TooLarge,
        FailReason::Unsupported,
        FailReason::InvalidMessage,
        FailReason::AttachmentMissing,
        FailReason::AttachmentsTooLarge,
        FailReason::IdentityMismatch,
        FailReason::Interrupted,
        FailReason::Storage,
        FailReason::MissedSchedule,
    ];

    pub fn key(self) -> &'static str {
        match self {
            FailReason::Rejected => "rejected",
            FailReason::RecipientsRejected => "recipients_rejected",
            FailReason::AuthFailed => "auth_failed",
            FailReason::Network => "network",
            FailReason::Certificate => "certificate",
            FailReason::TooLarge => "too_large",
            FailReason::Unsupported => "unsupported",
            FailReason::InvalidMessage => "invalid_message",
            FailReason::AttachmentMissing => "attachment_missing",
            FailReason::AttachmentsTooLarge => "attachments_too_large",
            FailReason::IdentityMismatch => "identity_mismatch",
            FailReason::Interrupted => "interrupted",
            FailReason::Storage => "storage",
            FailReason::MissedSchedule => "missed_schedule",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.key() == key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutboxState {
    /// Being written. The only state in which content can change.
    #[default]
    Draft,
    /// Waiting for its send time ([`DraftRecord::scheduled_at`]). Content is frozen.
    Scheduled,
    /// The user asked to send; not yet handed to the transport.
    Queued,
    /// Handed to the transport; no result recorded yet.
    Sending,
    /// The server accepted responsibility for the message.
    Sent,
    /// Definitely not sent.
    Failed { reason: FailReason, retryable: bool },
    /// May or may not have been sent. Never retried automatically.
    Unknown,
}

impl OutboxState {
    pub fn key(self) -> &'static str {
        match self {
            OutboxState::Draft => "draft",
            OutboxState::Scheduled => "scheduled",
            OutboxState::Queued => "queued",
            OutboxState::Sending => "sending",
            OutboxState::Sent => "sent",
            OutboxState::Failed { .. } => "failed",
            OutboxState::Unknown => "unknown",
        }
    }

    /// Rebuild from stored columns. Unknown text is an error, never a silent default.
    pub fn from_parts(key: &str, reason: Option<&str>, retryable: bool) -> Option<Self> {
        Some(match key {
            "draft" => OutboxState::Draft,
            "scheduled" => OutboxState::Scheduled,
            "queued" => OutboxState::Queued,
            "sending" => OutboxState::Sending,
            "sent" => OutboxState::Sent,
            "unknown" => OutboxState::Unknown,
            "failed" => OutboxState::Failed {
                reason: FailReason::from_key(reason?)?,
                retryable,
            },
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxEvent {
    /// The user pressed Send.
    Queue,
    /// The transport is about to be called.
    Start,
    Accepted,
    Refused {
        reason: FailReason,
        retryable: bool,
    },
    /// The connection was lost after the message content was written.
    Lost,
    /// The app started and found this message mid-flight.
    Recovered,
    /// The user pressed Retry on a failed message.
    Retry,
    /// The user confirmed sending again although the first attempt may have gone through.
    ConfirmResend,
    /// The user checked and the message did arrive.
    MarkSent,
    /// The user wants to change a failed or uncertain message.
    Edit,
    /// The user picked a send time.
    Schedule,
    /// The user cancelled the schedule or wants to edit: back to a draft.
    Unschedule,
    /// The send time came while the app was running (or within the grace period).
    Due,
    /// The send time passed longer ago than the grace period.
    Missed,
}

/// The whole state machine. `None`: the event is not allowed in that state.
pub fn transition(state: OutboxState, event: OutboxEvent) -> Option<OutboxState> {
    use OutboxEvent as E;
    use OutboxState as S;
    Some(match (state, event) {
        (S::Draft, E::Queue) => S::Queued,
        (S::Draft, E::Schedule) => S::Scheduled,
        (S::Scheduled, E::Unschedule) => S::Draft,
        (S::Scheduled, E::Due) => S::Queued,
        (S::Scheduled, E::Missed) => S::Failed {
            reason: FailReason::MissedSchedule,
            retryable: true,
        },
        (S::Queued, E::Start) => S::Sending,
        (S::Queued | S::Sending, E::Refused { reason, retryable }) => {
            S::Failed { reason, retryable }
        }
        (S::Sending, E::Accepted) => S::Sent,
        (S::Sending, E::Lost) => S::Unknown,
        // Start-up: mid-flight is uncertain; queued never left; everything else is settled.
        (S::Sending, E::Recovered) => S::Unknown,
        (S::Queued, E::Recovered) => S::Failed {
            reason: FailReason::Interrupted,
            retryable: true,
        },
        (settled, E::Recovered) => settled,
        (
            S::Failed {
                retryable: true, ..
            },
            E::Retry,
        ) => S::Queued,
        (S::Unknown, E::ConfirmResend) => S::Queued,
        (S::Unknown, E::MarkSent) => S::Sent,
        (S::Failed { .. } | S::Unknown, E::Edit) => S::Draft,
        _ => return None,
    })
}

/// What happened to the copy in the server's Sent folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SentCopyState {
    /// Not sent yet, or nothing to reconcile.
    #[default]
    None,
    /// Sent; the Sent-folder copy has not been checked yet.
    Pending,
    /// We appended a copy.
    Saved,
    /// The provider had already saved it; no second copy was made.
    AlreadyThere,
    /// Sent, but the copy could not be saved (no Sent folder, server refused, connection).
    Failed,
}

impl SentCopyState {
    pub fn key(self) -> &'static str {
        match self {
            SentCopyState::None => "none",
            SentCopyState::Pending => "pending",
            SentCopyState::Saved => "saved",
            SentCopyState::AlreadyThere => "already_there",
            SentCopyState::Failed => "failed",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        [
            SentCopyState::None,
            SentCopyState::Pending,
            SentCopyState::Saved,
            SentCopyState::AlreadyThere,
            SentCopyState::Failed,
        ]
        .into_iter()
        .find(|s| s.key() == key)
    }
}

/// An attachment is a reference to a local file, or to an attachment of a received message
/// (forward). Either is read (and size-checked) when sending.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AttachmentRef {
    /// Local file; empty for a forwarded part.
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    /// Forward: the attachment is fetched again from the original message at send time.
    pub forwarded: Option<ForwardedPart>,
}

/// One attachment of a message in the local cache (`MessageRecord::id` and its attachment
/// position, as listed by the cached body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedPart {
    pub message_id: String,
    pub part: u32,
}

/// A draft or outbox message. Contains mail content, never credentials.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DraftRecord {
    pub id: String,
    pub owner_id: String,
    pub account_id: String,
    /// Bumped by the store on every content save; a save with a stale version is refused.
    pub version: u32,
    /// `<id@domain>`, generated once when the draft is created and kept for its whole life.
    pub message_id: String,
    pub from_identity: String,
    /// Recipients as entered: `addr` or `Name <addr>`.
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub attachments: Vec<AttachmentRef>,
    pub state: OutboxState,
    pub sent_copy: SentCopyState,
    /// Seconds since the Unix epoch, UTC. Set by the store.
    pub updated_at: i64,
    /// Send time (seconds since the Unix epoch, UTC) while `Scheduled`; kept afterwards so the
    /// outbox can say which time was missed. Cleared when the message goes back to `Draft`.
    pub scheduled_at: Option<i64>,
}

/// Storage port for drafts and the outbox. Every call is scoped by `owner_id`; a draft of
/// another owner behaves exactly like one that does not exist.
#[allow(async_fn_in_trait)]
pub trait DraftStore {
    /// Create the draft, or replace its content when `draft.version` matches the stored version.
    /// Content can only change while the stored state is `Draft`; the stored `message_id` and
    /// state are never changed by this call. Returns the new version.
    async fn save_draft(&self, draft: &DraftRecord) -> Result<u32, MailboxError>;
    async fn get_draft(&self, owner_id: &str, draft_id: &str) -> Result<DraftRecord, MailboxError>;
    /// Most recently updated first.
    async fn list_drafts(&self, owner_id: &str) -> Result<Vec<DraftRecord>, MailboxError>;
    /// Local only. Refused while the message is `Queued` or `Sending`.
    async fn delete_draft(&self, owner_id: &str, draft_id: &str) -> Result<(), MailboxError>;
    /// Compare-and-set: succeeds only if the stored state is still `expected`.
    async fn set_state(
        &self,
        owner_id: &str,
        draft_id: &str,
        expected: OutboxState,
        next: OutboxState,
    ) -> Result<(), MailboxError>;
    async fn set_sent_copy(
        &self,
        owner_id: &str,
        draft_id: &str,
        sent_copy: SentCopyState,
    ) -> Result<(), MailboxError>;
}

/// Result of one delivery attempt, as the transport saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Accepted,
    /// Definitely not sent.
    Refused {
        reason: FailReason,
        retryable: bool,
    },
    /// Handed over, no final answer.
    Unknown,
}

/// Hands one message to a mail server, once. Implementations must not retry.
#[allow(async_fn_in_trait)]
pub trait Transport {
    async fn deliver(&mut self, draft: &DraftRecord) -> Delivery;
}

/// What the user asked for. Each value comes from a distinct, explicit action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendIntent {
    /// Send a draft.
    Send,
    /// Try again after a failure that may be temporary.
    Retry,
    /// Send again although the previous attempt may have been delivered.
    ResendAfterUnknown,
    /// The running app found a scheduled message due (see [`due_action`]).
    Scheduled,
}

const NOT_ALLOWED: MailboxError = MailboxError::InconsistentData("outbox transition");

async fn step<S: DraftStore>(
    store: &S,
    owner_id: &str,
    draft_id: &str,
    from: OutboxState,
    event: OutboxEvent,
) -> Result<OutboxState, MailboxError> {
    let next = transition(from, event).ok_or(NOT_ALLOWED)?;
    store.set_state(owner_id, draft_id, from, next).await?;
    Ok(next)
}

/// Run one explicit send attempt and record every state on the way. Returns the final state.
pub async fn submit<S: DraftStore, T: Transport>(
    store: &S,
    transport: &mut T,
    owner_id: &str,
    draft_id: &str,
    intent: SendIntent,
) -> Result<OutboxState, MailboxError> {
    let draft = store.get_draft(owner_id, draft_id).await?;
    let first = match (intent, draft.state) {
        (SendIntent::Send, OutboxState::Draft) => OutboxEvent::Queue,
        (SendIntent::Retry, OutboxState::Failed { .. }) => OutboxEvent::Retry,
        (SendIntent::ResendAfterUnknown, OutboxState::Unknown) => OutboxEvent::ConfirmResend,
        (SendIntent::Scheduled, OutboxState::Scheduled) => OutboxEvent::Due,
        _ => return Err(NOT_ALLOWED),
    };
    let queued = step(store, owner_id, draft_id, draft.state, first).await?;
    // Recorded before any network activity: a crash from here on is visible as `Sending`.
    let sending = step(store, owner_id, draft_id, queued, OutboxEvent::Start).await?;
    let event = match transport.deliver(&draft).await {
        Delivery::Accepted => OutboxEvent::Accepted,
        Delivery::Refused { reason, retryable } => OutboxEvent::Refused { reason, retryable },
        Delivery::Unknown => OutboxEvent::Lost,
    };
    step(store, owner_id, draft_id, sending, event).await
}

/// Call once at start-up, before the outbox is shown. Messages that were mid-flight when the app
/// stopped become `Unknown`; queued ones become a retryable failure. Sends nothing.
/// Returns how many messages changed.
pub async fn recover<S: DraftStore>(store: &S, owner_id: &str) -> Result<u32, MailboxError> {
    let mut changed = 0;
    for draft in store.list_drafts(owner_id).await? {
        if let Some(next) = transition(draft.state, OutboxEvent::Recovered)
            && next != draft.state
        {
            store
                .set_state(owner_id, &draft.id, draft.state, next)
                .await?;
            changed += 1;
        }
    }
    Ok(changed)
}

/// Apply a user decision that needs no network: mark an uncertain message as sent, or take a
/// failed/uncertain message back to editing.
pub async fn decide<S: DraftStore>(
    store: &S,
    owner_id: &str,
    draft_id: &str,
    event: OutboxEvent,
) -> Result<OutboxState, MailboxError> {
    if !matches!(event, OutboxEvent::MarkSent | OutboxEvent::Edit) {
        return Err(NOT_ALLOWED);
    }
    let draft = store.get_draft(owner_id, draft_id).await?;
    step(store, owner_id, draft_id, draft.state, event).await
}

/// A scheduled time must be at least this far ahead when it is picked.
pub const MIN_LEAD_SECS: i64 = 60;
/// A scheduled time can be at most this far ahead (one year).
pub const MAX_LEAD_SECS: i64 = 366 * 24 * 3600;
/// A due message is still sent automatically when found up to this late (app was closed or the
/// computer slept at the send time). Later than that it waits for the user. Stated in the UI.
pub const MISSED_GRACE_SECS: i64 = 12 * 3600;

/// Why a send time cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleError {
    /// In the past or less than [`MIN_LEAD_SECS`] ahead.
    TooSoon,
    /// More than [`MAX_LEAD_SECS`] ahead.
    TooFar,
}

/// Check a send time picked by the user (UTC epoch seconds) against the current time.
pub fn check_schedule(at: i64, now: i64) -> Result<(), ScheduleError> {
    let lead = at.saturating_sub(now);
    if lead < MIN_LEAD_SECS {
        Err(ScheduleError::TooSoon)
    } else if lead > MAX_LEAD_SECS {
        Err(ScheduleError::TooFar)
    } else {
        Ok(())
    }
}

/// What the running app does with a scheduled message at `now`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DueAction {
    /// Not yet; check again in this many seconds.
    Wait(i64),
    /// Send now.
    Send,
    /// Too late to send without asking: becomes a retryable `MissedSchedule` failure.
    Missed,
}

pub fn due_action(at: i64, now: i64) -> DueAction {
    if now < at {
        DueAction::Wait(at - now)
    } else if now - at <= MISSED_GRACE_SECS {
        DueAction::Send
    } else {
        DueAction::Missed
    }
}

/// Storage for the send time. Every call is owner-scoped and compare-and-set like
/// [`DraftStore::set_state`].
#[allow(async_fn_in_trait)]
pub trait ScheduleStore {
    /// `Draft` (or `Scheduled`, to change the time) -> `Scheduled` at `at`. Refused in any other
    /// state, so a message already queued or sending cannot be rescheduled.
    async fn schedule_draft(
        &self,
        owner_id: &str,
        draft_id: &str,
        at: i64,
    ) -> Result<(), MailboxError>;
    /// `Scheduled` -> `Draft`, clearing the send time. Refused in any other state.
    async fn unschedule_draft(&self, owner_id: &str, draft_id: &str) -> Result<(), MailboxError>;
}

/// The user picked a send time. Validated against `now`; sends nothing.
pub async fn schedule<S: ScheduleStore>(
    store: &S,
    owner_id: &str,
    draft_id: &str,
    at: i64,
    now: i64,
) -> Result<(), MailboxError> {
    check_schedule(at, now).map_err(|_| MailboxError::InconsistentData("schedule time"))?;
    store.schedule_draft(owner_id, draft_id, at).await
}

/// Cancel a schedule (or take it back to edit). Fails once the send has started.
pub async fn unschedule<S: ScheduleStore>(
    store: &S,
    owner_id: &str,
    draft_id: &str,
) -> Result<(), MailboxError> {
    store.unschedule_draft(owner_id, draft_id).await
}

/// Look at every scheduled message: those past the grace period become a retryable
/// `MissedSchedule` failure (nothing is sent), those due are returned for the caller to submit
/// with [`SendIntent::Scheduled`] (the compare-and-set in [`submit`] stops a second send).
/// Also returns the earliest future send time, so the caller knows when to look again.
pub async fn take_due<S: DraftStore>(
    store: &S,
    owner_id: &str,
    now: i64,
) -> Result<(Vec<DraftRecord>, Option<i64>), MailboxError> {
    let mut due = Vec::new();
    let mut next: Option<i64> = None;
    for draft in store.list_drafts(owner_id).await? {
        if draft.state != OutboxState::Scheduled {
            continue;
        }
        // A scheduled row without a time is broken data: never sent on a guess.
        let Some(at) = draft.scheduled_at else {
            step(store, owner_id, &draft.id, draft.state, OutboxEvent::Missed).await?;
            continue;
        };
        match due_action(at, now) {
            DueAction::Wait(_) => next = Some(next.map_or(at, |n| n.min(at))),
            DueAction::Send => due.push(draft),
            DueAction::Missed => {
                step(store, owner_id, &draft.id, draft.state, OutboxEvent::Missed).await?;
            }
        }
    }
    due.sort_by_key(|d| d.scheduled_at);
    Ok((due, next))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    /// In-memory store that also records every state it was asked to write.
    #[derive(Default)]
    struct MemStore {
        drafts: RefCell<BTreeMap<String, DraftRecord>>,
        history: RefCell<Vec<OutboxState>>,
    }

    impl MemStore {
        fn with(state: OutboxState) -> Self {
            let store = MemStore::default();
            let draft = DraftRecord {
                id: "d1".into(),
                owner_id: "me".into(),
                account_id: "acc".into(),
                message_id: "<d1.x@example.com>".into(),
                state,
                ..DraftRecord::default()
            };
            store.drafts.borrow_mut().insert("d1".into(), draft);
            store
        }

        fn state(&self) -> OutboxState {
            self.drafts.borrow()["d1"].state
        }
    }

    impl DraftStore for MemStore {
        async fn save_draft(&self, draft: &DraftRecord) -> Result<u32, MailboxError> {
            self.drafts
                .borrow_mut()
                .insert(draft.id.clone(), draft.clone());
            Ok(draft.version + 1)
        }
        async fn get_draft(&self, owner: &str, id: &str) -> Result<DraftRecord, MailboxError> {
            self.drafts
                .borrow()
                .get(id)
                .filter(|d| d.owner_id == owner)
                .cloned()
                .ok_or(MailboxError::NotFound)
        }
        async fn list_drafts(&self, owner: &str) -> Result<Vec<DraftRecord>, MailboxError> {
            Ok(self
                .drafts
                .borrow()
                .values()
                .filter(|d| d.owner_id == owner)
                .cloned()
                .collect())
        }
        async fn delete_draft(&self, _: &str, id: &str) -> Result<(), MailboxError> {
            self.drafts.borrow_mut().remove(id);
            Ok(())
        }
        async fn set_state(
            &self,
            owner: &str,
            id: &str,
            expected: OutboxState,
            next: OutboxState,
        ) -> Result<(), MailboxError> {
            let mut drafts = self.drafts.borrow_mut();
            let draft = drafts
                .get_mut(id)
                .filter(|d| d.owner_id == owner)
                .ok_or(MailboxError::NotFound)?;
            if draft.state != expected {
                return Err(MailboxError::InconsistentData("outbox state changed"));
            }
            draft.state = next;
            self.history.borrow_mut().push(next);
            Ok(())
        }
        async fn set_sent_copy(
            &self,
            _: &str,
            id: &str,
            s: SentCopyState,
        ) -> Result<(), MailboxError> {
            self.drafts
                .borrow_mut()
                .get_mut(id)
                .ok_or(MailboxError::NotFound)?
                .sent_copy = s;
            Ok(())
        }
    }

    /// Transport double: counts calls and records what the store said while it was called.
    struct Fake<'a> {
        result: Option<Delivery>,
        calls: u32,
        seen_ids: Vec<String>,
        store: &'a MemStore,
        state_during_call: Vec<OutboxState>,
    }

    impl<'a> Fake<'a> {
        fn new(store: &'a MemStore, result: Option<Delivery>) -> Self {
            Self {
                result,
                calls: 0,
                seen_ids: Vec::new(),
                store,
                state_during_call: Vec::new(),
            }
        }
    }

    impl Transport for Fake<'_> {
        async fn deliver(&mut self, draft: &DraftRecord) -> Delivery {
            self.calls += 1;
            self.seen_ids.push(draft.message_id.clone());
            self.state_during_call.push(self.store.state());
            match self.result {
                Some(result) => result,
                // The process "dies" here: the call never returns.
                None => std::future::pending().await,
            }
        }
    }

    fn poll_once<F: Future>(future: F) -> Poll<F::Output> {
        std::pin::pin!(future)
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
    }

    fn run<F: Future>(future: F) -> F::Output {
        match poll_once(future) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("test future did not finish"),
        }
    }

    const TEMPORARY: Delivery = Delivery::Refused {
        reason: FailReason::Network,
        retryable: true,
    };
    const PERMANENT: Delivery = Delivery::Refused {
        reason: FailReason::Rejected,
        retryable: false,
    };

    #[test]
    fn transition_table_is_exactly_the_documented_one() {
        use OutboxEvent as E;
        use OutboxState as S;
        let failed = |retryable| S::Failed {
            reason: FailReason::Network,
            retryable,
        };
        let refused = E::Refused {
            reason: FailReason::Network,
            retryable: true,
        };
        let allowed = [
            (S::Draft, E::Queue, S::Queued),
            (S::Queued, E::Start, S::Sending),
            (S::Queued, refused, failed(true)),
            (
                S::Queued,
                E::Recovered,
                S::Failed {
                    reason: FailReason::Interrupted,
                    retryable: true,
                },
            ),
            (S::Sending, E::Accepted, S::Sent),
            (S::Sending, refused, failed(true)),
            (S::Sending, E::Lost, S::Unknown),
            (S::Sending, E::Recovered, S::Unknown),
            (failed(true), E::Retry, S::Queued),
            (failed(true), E::Edit, S::Draft),
            (failed(false), E::Edit, S::Draft),
            (S::Unknown, E::ConfirmResend, S::Queued),
            (S::Unknown, E::MarkSent, S::Sent),
            (S::Unknown, E::Edit, S::Draft),
            // Start-up recovery leaves settled messages alone.
            (S::Draft, E::Recovered, S::Draft),
            (S::Sent, E::Recovered, S::Sent),
            (S::Unknown, E::Recovered, S::Unknown),
            (failed(true), E::Recovered, failed(true)),
            (failed(false), E::Recovered, failed(false)),
            // Scheduled send (MB-218).
            (S::Draft, E::Schedule, S::Scheduled),
            (S::Scheduled, E::Unschedule, S::Draft),
            (S::Scheduled, E::Due, S::Queued),
            (
                S::Scheduled,
                E::Missed,
                S::Failed {
                    reason: FailReason::MissedSchedule,
                    retryable: true,
                },
            ),
            (S::Scheduled, E::Recovered, S::Scheduled),
        ];
        let states = [
            S::Draft,
            S::Scheduled,
            S::Queued,
            S::Sending,
            S::Sent,
            failed(true),
            failed(false),
            S::Unknown,
        ];
        let events = [
            E::Queue,
            E::Start,
            E::Accepted,
            refused,
            E::Lost,
            E::Recovered,
            E::Retry,
            E::ConfirmResend,
            E::MarkSent,
            E::Edit,
            E::Schedule,
            E::Unschedule,
            E::Due,
            E::Missed,
        ];
        for s in states {
            for e in events {
                let expected = allowed
                    .iter()
                    .find(|(a, b, _)| *a == s && *b == e)
                    .map(|(_, _, n)| *n);
                assert_eq!(transition(s, e), expected, "{s:?} + {e:?}");
            }
        }
        // The two rules that matter most, spelled out.
        assert_eq!(
            transition(S::Unknown, E::Retry),
            None,
            "uncertain is never a plain retry"
        );
        assert_eq!(transition(S::Unknown, E::Queue), None);
        assert_eq!(transition(S::Sent, E::Queue), None, "sent is final");
        assert_eq!(
            transition(failed(false), E::Retry),
            None,
            "permanent failures need an edit"
        );
    }

    #[test]
    fn sending_is_recorded_before_the_transport_is_called() {
        let store = MemStore::with(OutboxState::Draft);
        let mut t = Fake::new(&store, Some(Delivery::Accepted));
        assert_eq!(
            run(submit(&store, &mut t, "me", "d1", SendIntent::Send)),
            Ok(OutboxState::Sent)
        );
        assert_eq!(t.calls, 1);
        assert_eq!(t.state_during_call, vec![OutboxState::Sending]);
        assert_eq!(
            *store.history.borrow(),
            vec![OutboxState::Queued, OutboxState::Sending, OutboxState::Sent]
        );
    }

    #[test]
    fn temporary_failure_can_be_retried_only_by_an_explicit_call() {
        let store = MemStore::with(OutboxState::Draft);
        let mut t = Fake::new(&store, Some(TEMPORARY));
        let failed = OutboxState::Failed {
            reason: FailReason::Network,
            retryable: true,
        };
        assert_eq!(
            run(submit(&store, &mut t, "me", "d1", SendIntent::Send)),
            Ok(failed)
        );
        assert_eq!(t.calls, 1, "one attempt per call, no loop");
        // A second plain Send is not a retry.
        assert!(run(submit(&store, &mut t, "me", "d1", SendIntent::Send)).is_err());
        assert_eq!(t.calls, 1);
        t.result = Some(Delivery::Accepted);
        assert_eq!(
            run(submit(&store, &mut t, "me", "d1", SendIntent::Retry)),
            Ok(OutboxState::Sent)
        );
        assert_eq!(t.calls, 2);
        assert_eq!(t.seen_ids[0], t.seen_ids[1], "same Message-ID on the retry");
    }

    #[test]
    fn permanent_failure_cannot_be_retried_unchanged() {
        let store = MemStore::with(OutboxState::Draft);
        let mut t = Fake::new(&store, Some(PERMANENT));
        run(submit(&store, &mut t, "me", "d1", SendIntent::Send)).unwrap();
        assert!(run(submit(&store, &mut t, "me", "d1", SendIntent::Retry)).is_err());
        assert_eq!(t.calls, 1);
        assert_eq!(
            run(decide(&store, "me", "d1", OutboxEvent::Edit)),
            Ok(OutboxState::Draft)
        );
    }

    #[test]
    fn unknown_outcome_is_never_resent_without_explicit_confirmation() {
        let store = MemStore::with(OutboxState::Draft);
        let mut t = Fake::new(&store, Some(Delivery::Unknown));
        assert_eq!(
            run(submit(&store, &mut t, "me", "d1", SendIntent::Send)),
            Ok(OutboxState::Unknown)
        );
        for intent in [SendIntent::Send, SendIntent::Retry] {
            assert!(
                run(submit(&store, &mut t, "me", "d1", intent)).is_err(),
                "{intent:?}"
            );
        }
        assert_eq!(t.calls, 1, "the transport was not touched again");
        assert_eq!(store.state(), OutboxState::Unknown);

        t.result = Some(Delivery::Accepted);
        assert_eq!(
            run(submit(
                &store,
                &mut t,
                "me",
                "d1",
                SendIntent::ResendAfterUnknown
            )),
            Ok(OutboxState::Sent)
        );
        assert_eq!(t.calls, 2);
        assert_eq!(
            t.seen_ids[0], t.seen_ids[1],
            "the resend is detectable as a duplicate"
        );
    }

    #[test]
    fn user_can_settle_an_unknown_message_as_sent_without_sending() {
        let store = MemStore::with(OutboxState::Unknown);
        assert_eq!(
            run(decide(&store, "me", "d1", OutboxEvent::MarkSent)),
            Ok(OutboxState::Sent)
        );
        let store = MemStore::with(OutboxState::Unknown);
        // Only the two no-network decisions go through `decide`.
        assert!(run(decide(&store, "me", "d1", OutboxEvent::ConfirmResend)).is_err());
        assert!(run(decide(&store, "me", "d1", OutboxEvent::Accepted)).is_err());
        assert_eq!(store.state(), OutboxState::Unknown);
    }

    #[test]
    fn crash_mid_send_comes_back_as_unknown_and_is_not_resent() {
        let store = MemStore::with(OutboxState::Draft);
        {
            let mut t = Fake::new(&store, None);
            // The send starts and the process stops before any result is recorded.
            assert!(poll_once(submit(&store, &mut t, "me", "d1", SendIntent::Send)).is_pending());
            assert_eq!(t.calls, 1);
        }
        assert_eq!(
            store.state(),
            OutboxState::Sending,
            "what the next start finds on disk"
        );

        // Next start: recovery has no transport, so it cannot send anything.
        assert_eq!(run(recover(&store, "me")), Ok(1));
        assert_eq!(store.state(), OutboxState::Unknown);
        assert_eq!(run(recover(&store, "me")), Ok(0), "idempotent");

        let mut t = Fake::new(&store, Some(Delivery::Accepted));
        assert!(run(submit(&store, &mut t, "me", "d1", SendIntent::Send)).is_err());
        assert!(run(submit(&store, &mut t, "me", "d1", SendIntent::Retry)).is_err());
        assert_eq!(
            t.calls, 0,
            "no automatic or accidental resend after a crash"
        );
    }

    #[test]
    fn crash_while_queued_is_a_retryable_failure_because_nothing_left() {
        let store = MemStore::with(OutboxState::Queued);
        assert_eq!(run(recover(&store, "me")), Ok(1));
        assert_eq!(
            store.state(),
            OutboxState::Failed {
                reason: FailReason::Interrupted,
                retryable: true
            }
        );
    }

    #[test]
    fn a_message_already_in_flight_or_sent_cannot_be_submitted_again() {
        for state in [OutboxState::Queued, OutboxState::Sending, OutboxState::Sent] {
            let store = MemStore::with(state);
            let mut t = Fake::new(&store, Some(Delivery::Accepted));
            for intent in [
                SendIntent::Send,
                SendIntent::Retry,
                SendIntent::ResendAfterUnknown,
            ] {
                assert!(
                    run(submit(&store, &mut t, "me", "d1", intent)).is_err(),
                    "{state:?} {intent:?}"
                );
            }
            assert_eq!(t.calls, 0, "{state:?}");
            assert_eq!(store.state(), state);
        }
    }

    #[test]
    fn other_owners_cannot_send_or_recover_a_draft() {
        let store = MemStore::with(OutboxState::Draft);
        let mut t = Fake::new(&store, Some(Delivery::Accepted));
        assert_eq!(
            run(submit(
                &store,
                &mut t,
                "someone-else",
                "d1",
                SendIntent::Send
            )),
            Err(MailboxError::NotFound)
        );
        assert_eq!(t.calls, 0);
        assert_eq!(run(recover(&store, "someone-else")), Ok(0));
    }

    #[test]
    fn stored_keys_round_trip_and_unknown_text_is_rejected() {
        for reason in FailReason::ALL {
            assert_eq!(FailReason::from_key(reason.key()), Some(reason));
            let state = OutboxState::Failed {
                reason,
                retryable: true,
            };
            assert_eq!(
                OutboxState::from_parts("failed", Some(reason.key()), true),
                Some(state)
            );
        }
        for state in [
            OutboxState::Draft,
            OutboxState::Scheduled,
            OutboxState::Queued,
            OutboxState::Sending,
            OutboxState::Sent,
            OutboxState::Unknown,
        ] {
            assert_eq!(
                OutboxState::from_parts(state.key(), None, false),
                Some(state)
            );
        }
        assert_eq!(OutboxState::from_parts("failed", None, false), None);
        assert_eq!(OutboxState::from_parts("failed", Some("nope"), false), None);
        assert_eq!(OutboxState::from_parts("delivered", None, false), None);
        assert_eq!(
            SentCopyState::from_key("already_there"),
            Some(SentCopyState::AlreadyThere)
        );
        assert_eq!(SentCopyState::from_key("x"), None);
    }

    impl ScheduleStore for MemStore {
        async fn schedule_draft(&self, owner: &str, id: &str, at: i64) -> Result<(), MailboxError> {
            let mut drafts = self.drafts.borrow_mut();
            let d = drafts
                .get_mut(id)
                .filter(|d| d.owner_id == owner)
                .ok_or(MailboxError::NotFound)?;
            if !matches!(d.state, OutboxState::Draft | OutboxState::Scheduled) {
                return Err(MailboxError::InconsistentData("outbox state changed"));
            }
            d.state = OutboxState::Scheduled;
            d.scheduled_at = Some(at);
            Ok(())
        }
        async fn unschedule_draft(&self, owner: &str, id: &str) -> Result<(), MailboxError> {
            let mut drafts = self.drafts.borrow_mut();
            let d = drafts
                .get_mut(id)
                .filter(|d| d.owner_id == owner)
                .ok_or(MailboxError::NotFound)?;
            if d.state != OutboxState::Scheduled {
                return Err(MailboxError::InconsistentData("outbox state changed"));
            }
            d.state = OutboxState::Draft;
            d.scheduled_at = None;
            Ok(())
        }
    }

    const NOW: i64 = 1_800_000_000;

    #[test]
    fn a_send_time_must_be_ahead_but_not_absurdly_far() {
        assert_eq!(check_schedule(NOW - 1, NOW), Err(ScheduleError::TooSoon));
        assert_eq!(
            check_schedule(NOW + MIN_LEAD_SECS - 1, NOW),
            Err(ScheduleError::TooSoon)
        );
        assert_eq!(check_schedule(NOW + MIN_LEAD_SECS, NOW), Ok(()));
        assert_eq!(check_schedule(NOW + MAX_LEAD_SECS, NOW), Ok(()));
        assert_eq!(
            check_schedule(NOW + MAX_LEAD_SECS + 1, NOW),
            Err(ScheduleError::TooFar)
        );
        assert_eq!(
            check_schedule(i64::MAX, i64::MAX - 5),
            Err(ScheduleError::TooSoon)
        );
    }

    #[test]
    fn due_rule_sends_within_the_grace_period_and_holds_after_it() {
        assert_eq!(due_action(NOW + 90, NOW), DueAction::Wait(90));
        assert_eq!(due_action(NOW, NOW), DueAction::Send);
        assert_eq!(due_action(NOW - MISSED_GRACE_SECS, NOW), DueAction::Send);
        assert_eq!(
            due_action(NOW - MISSED_GRACE_SECS - 1, NOW),
            DueAction::Missed
        );
    }

    #[test]
    fn scheduling_freezes_the_draft_and_cancel_brings_it_back() {
        let store = MemStore::with(OutboxState::Draft);
        assert!(
            run(schedule(&store, "me", "d1", NOW + 10, NOW)).is_err(),
            "too soon"
        );
        assert_eq!(store.state(), OutboxState::Draft);
        run(schedule(&store, "me", "d1", NOW + 3600, NOW)).unwrap();
        assert_eq!(store.state(), OutboxState::Scheduled);
        // Changing the time while scheduled is allowed.
        run(schedule(&store, "me", "d1", NOW + 7200, NOW)).unwrap();
        assert_eq!(store.drafts.borrow()["d1"].scheduled_at, Some(NOW + 7200));
        // A plain "send" or a "retry" cannot bypass the schedule.
        let mut t = Fake::new(&store, Some(Delivery::Accepted));
        for intent in [
            SendIntent::Send,
            SendIntent::Retry,
            SendIntent::ResendAfterUnknown,
        ] {
            assert!(run(submit(&store, &mut t, "me", "d1", intent)).is_err());
        }
        assert_eq!(t.calls, 0);
        // Restart leaves it scheduled.
        assert_eq!(run(recover(&store, "me")), Ok(0));
        assert_eq!(store.state(), OutboxState::Scheduled);
        run(unschedule(&store, "me", "d1")).unwrap();
        assert_eq!(store.state(), OutboxState::Draft);
        assert_eq!(store.drafts.borrow()["d1"].scheduled_at, None);
        assert!(
            run(unschedule(&store, "me", "d1")).is_err(),
            "nothing to cancel"
        );
        // Another owner cannot schedule or cancel.
        assert_eq!(
            run(schedule(&store, "x", "d1", NOW + 3600, NOW)),
            Err(MailboxError::NotFound)
        );
    }

    #[test]
    fn due_messages_are_sent_once_and_missed_ones_wait_for_the_user() {
        let store = MemStore::default();
        for (id, at) in [
            ("d1", Some(NOW - 30)),
            ("late", Some(NOW - MISSED_GRACE_SECS - 60)),
            ("later", Some(NOW + 500)),
            ("soon", Some(NOW + 100)),
            ("broken", None),
        ] {
            store.drafts.borrow_mut().insert(
                id.into(),
                DraftRecord {
                    id: id.into(),
                    owner_id: "me".into(),
                    message_id: format!("<{id}@x>"),
                    state: OutboxState::Scheduled,
                    scheduled_at: at,
                    ..DraftRecord::default()
                },
            );
        }
        let (due, next) = run(take_due(&store, "me", NOW)).unwrap();
        assert_eq!(
            due.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
            ["d1"]
        );
        assert_eq!(next, Some(NOW + 100));
        let missed = OutboxState::Failed {
            reason: FailReason::MissedSchedule,
            retryable: true,
        };
        for id in ["late", "broken"] {
            assert_eq!(store.drafts.borrow()[id].state, missed, "{id} is not sent");
        }
        let mut t = Fake::new(&store, Some(Delivery::Accepted));
        assert_eq!(
            run(submit(&store, &mut t, "me", "d1", SendIntent::Scheduled)),
            Ok(OutboxState::Sent)
        );
        // A second tick (or a second window) finds nothing and cannot send again.
        assert!(run(submit(&store, &mut t, "me", "d1", SendIntent::Scheduled)).is_err());
        assert_eq!(t.calls, 1);
        assert!(run(take_due(&store, "me", NOW)).unwrap().0.is_empty());
        // The missed message is sent only by an explicit retry.
        assert_eq!(
            run(submit(&store, &mut t, "me", "late", SendIntent::Retry)),
            Ok(OutboxState::Sent)
        );
    }

    #[test]
    fn a_scheduled_send_that_breaks_off_is_unknown_like_any_other() {
        let store = MemStore::with(OutboxState::Scheduled);
        let mut t = Fake::new(&store, Some(Delivery::Unknown));
        assert_eq!(
            run(submit(&store, &mut t, "me", "d1", SendIntent::Scheduled)),
            Ok(OutboxState::Unknown)
        );
        assert!(run(submit(&store, &mut t, "me", "d1", SendIntent::Scheduled)).is_err());
        assert_eq!(t.calls, 1);
    }
}
