//! Drafts and sending on Desktop (MB-133, MB-150, MB-151). Nothing here exists in the Web build.
//!
//! - Drafts and outbox state live in the local SQLite store; passwords only in the OS keychain
//!   (`imap.<account>` from account setup, `smtp.<account>` when the SMTP password differs).
//! - A send is one explicit call that goes through `mailbox_core::outbox::submit`: the state is
//!   written before the network is touched, an uncertain outcome is kept as `Unknown`, and
//!   nothing in this file retries on its own.
//! - Attachments are read from disk at send time, with size checks, off the UI thread.
//! - Errors are short codes. Addresses, subjects, bodies and passwords are never logged.
use super::account::{OWNER, data_dir};
use super::connect::CheckFailure;
use crate::state::compose::{ReplyKind, threading};
use mailbox_compose::address::{Mailbox, parse_mailbox};
use mailbox_compose::message::{Attachment, BuildError, Limits, Message, Transfer, new_message_id};
use mailbox_core::outbox::{
    Delivery, DraftRecord, DraftStore, FailReason, ForwardedPart, OutboxEvent, OutboxState,
    SendIntent, SentCopyState, Transport, decide, recover, submit,
};
use mailbox_core::secrets::{Secret, SecretStore};
use mailbox_core::store::{AccountRecord, MailStore};
use mailbox_core::{MailServerEndpoint, MailboxError, TlsMode};
use mailbox_imap::append::{AppendError, FolderRole, SentCopy, find_folder, save_sent_copy};
use mailbox_imap::borrow::{RepeatableJob, live_or_fresh};
use mailbox_secrets::KeychainStore;
use mailbox_smtp::submit::{PartialPolicy, SendReport, Stage, Submission, SubmitError};
use mailbox_smtp::{SendOutcome, SmtpError, identity_matches_login};
use mailbox_store::SqliteStore;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Keychain reference for an SMTP password that differs from the IMAP one.
pub fn smtp_ref(account_id: &str) -> String {
    format!("smtp.{account_id}")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Unique, id-safe token: time, a per-process random value and a counter.
pub fn unique_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    // `RandomState` is seeded from the OS; hashing the time and counter mixes it in.
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(nanos);
    hasher.write_u64(count);
    format!("{nanos:x}.{:016x}.{count:x}", hasher.finish())
}

static RECOVERED: AtomicBool = AtomicBool::new(false);

/// Open the local store. The first call in a process also settles what an earlier run left
/// mid-flight (`Sending` becomes `Unknown`), before any new send can start. Sends nothing.
async fn store() -> Result<SqliteStore, MailboxError> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|_| MailboxError::Storage("data_dir"))?;
    let store = SqliteStore::open(&dir.join("mailbox.sqlite3")).await?;
    if !RECOVERED.swap(true, Ordering::SeqCst)
        && let Err(e) = recover(&store, OWNER).await
    {
        RECOVERED.store(false, Ordering::SeqCst);
        return Err(e);
    }
    Ok(store)
}

/// What a save gave the draft: its identity is fixed from the first save on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub id: String,
    pub version: u32,
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
}

/// `Message-ID` header of a synced message, looked up by the id the UI shows it under.
async fn original_message_id(
    store: &impl MailStore,
    account_id: &str,
    summary_id: &str,
) -> Option<String> {
    for folder in store.list_folders(OWNER, account_id).await.ok()? {
        let messages = store
            .list_messages(OWNER, account_id, &folder.id, 500)
            .await
            .ok()?;
        if let Some(m) = messages.into_iter().find(|m| m.id == summary_id) {
            return m.message_id_header;
        }
    }
    None
}

/// Save a draft. The first save allocates the draft id, the stable `Message-ID` and, for a
/// reply or forward, the threading headers.
pub async fn save_in<S: DraftStore + MailStore + mailbox_core::threading::ThreadStore>(
    store: &S,
    mut record: DraftRecord,
    reply: Option<(String, ReplyKind)>,
) -> Result<Saved, MailboxError> {
    if record.id.is_empty() {
        let token = unique_token();
        record.message_id = new_message_id(&token, &record.from_identity)
            .ok_or(MailboxError::InconsistentData("draft sender"))?;
        record.id = format!("d-{token}");
        record.version = 0;
        if let Some((summary_id, kind)) = reply {
            let original = original_message_id(store, &record.account_id, &summary_id).await;
            // The original's own References (stored since MB-201) continue the chain.
            let chain = store
                .headers_of(OWNER, &summary_id)
                .await
                .ok()
                .flatten()
                .map(|h| h.references)
                .unwrap_or_default();
            (record.in_reply_to, record.references) = threading(kind, original.as_deref(), &chain);
        }
    }
    let version = store.save_draft(&record).await?;
    Ok(Saved {
        id: record.id,
        version,
        message_id: record.message_id,
        in_reply_to: record.in_reply_to,
        references: record.references,
    })
}

pub async fn save(
    record: DraftRecord,
    reply: Option<(String, ReplyKind)>,
) -> Result<Saved, MailboxError> {
    save_in(&store().await?, record, reply).await
}

/// Drafts and outbox messages, newest first.
pub async fn list() -> Result<Vec<DraftRecord>, MailboxError> {
    store().await?.list_drafts(OWNER).await
}

/// Delete a local draft or outbox entry. Never touches the server.
pub async fn discard(draft_id: &str) -> Result<(), MailboxError> {
    store().await?.delete_draft(OWNER, draft_id).await
}

/// A message rendered and ready for SMTP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub submission: Submission,
}

fn mailboxes(list: &[String]) -> Result<Vec<Mailbox>, FailReason> {
    list.iter()
        .map(|entry| parse_mailbox(entry).map_err(|_| FailReason::InvalidMessage))
        .collect()
}

/// Build the message from a stored draft without forwarded parts. Reads the attachment files
/// (blocking). Tests only: the app always goes through [`prepare_with`].
#[cfg(test)]
pub fn prepare(
    record: &DraftRecord,
    account: &AccountRecord,
    now_epoch: u64,
    limits: &Limits,
) -> Result<Prepared, FailReason> {
    prepare_with(record, account, now_epoch, limits, &[])
}

/// Bytes of the forwarded attachments of a draft, fetched again from the original messages (one
/// fetch per original). A network problem is a retryable failure; a message that is gone or a
/// part that no longer matches is `AttachmentMissing`.
pub async fn forwarded_bytes(
    record: &DraftRecord,
) -> Result<Vec<(ForwardedPart, Vec<u8>)>, FailReason> {
    use super::body::BodyFailure;
    let mut by_message: std::collections::BTreeMap<&str, Vec<u32>> = Default::default();
    for p in record
        .attachments
        .iter()
        .filter_map(|a| a.forwarded.as_ref())
    {
        by_message.entry(&p.message_id).or_default().push(p.part);
    }
    let mut out = Vec::new();
    for (message_id, parts) in by_message {
        let bytes = super::body::attachment_data(message_id, &parts)
            .await
            .map_err(|e| match e {
                BodyFailure::TooLarge => FailReason::AttachmentsTooLarge,
                BodyFailure::Disk => FailReason::Storage,
                BodyFailure::Unreachable | BodyFailure::Auth | BodyFailure::Security => {
                    FailReason::Network
                }
                _ => FailReason::AttachmentMissing,
            })?;
        for (part, data) in parts.into_iter().zip(bytes) {
            out.push((
                ForwardedPart {
                    message_id: message_id.to_string(),
                    part,
                },
                data,
            ));
        }
    }
    Ok(out)
}

/// [`prepare`] with the bytes of forwarded attachments already fetched.
pub fn prepare_with(
    record: &DraftRecord,
    account: &AccountRecord,
    now_epoch: u64,
    limits: &Limits,
    forwarded: &[(ForwardedPart, Vec<u8>)],
) -> Result<Prepared, FailReason> {
    // The From address must be the identity the account logs in with (ACC-06).
    if !identity_matches_login(&account.email_address, &record.from_identity) {
        return Err(FailReason::IdentityMismatch);
    }
    let mut attachments = Vec::new();
    let mut total = 0u64;
    for a in &record.attachments {
        if let Some(part) = &a.forwarded {
            let data = forwarded
                .iter()
                .find(|(p, _)| p == part)
                .map(|(_, d)| d.clone())
                .ok_or(FailReason::AttachmentMissing)?;
            total += data.len() as u64;
            if total > limits.max_attachment_bytes {
                return Err(FailReason::AttachmentsTooLarge);
            }
            attachments.push(Attachment {
                filename: a.name.clone(),
                content_type: None,
                data,
            });
            continue;
        }
        let meta = std::fs::metadata(&a.path).map_err(|_| FailReason::AttachmentMissing)?;
        if !meta.is_file() {
            return Err(FailReason::AttachmentMissing);
        }
        // Checked before reading, so an oversized file is never loaded into memory.
        total += meta.len();
        if total > limits.max_attachment_bytes {
            return Err(FailReason::AttachmentsTooLarge);
        }
        attachments.push(Attachment {
            filename: a.name.clone(),
            content_type: None,
            data: std::fs::read(&a.path).map_err(|_| FailReason::AttachmentMissing)?,
        });
    }
    let message = Message {
        from: Mailbox {
            name: String::new(),
            address: record.from_identity.trim().to_string(),
        },
        to: mailboxes(&record.to)?,
        cc: mailboxes(&record.cc)?,
        bcc: mailboxes(&record.bcc)?,
        subject: record.subject.clone(),
        text: record.body_text.clone(),
        html: record.body_html.clone(),
        message_id: record.message_id.clone(),
        in_reply_to: record.in_reply_to.clone(),
        references: record.references.clone(),
        date_epoch: now_epoch,
        attachments,
    };
    let build = |transfer| {
        message.build(transfer, limits).map_err(|e| match e {
            BuildError::AttachmentsTooLarge { .. } => FailReason::AttachmentsTooLarge,
            BuildError::MessageTooLarge { .. } => FailReason::TooLarge,
            _ => FailReason::InvalidMessage,
        })
    };
    let built = build(Transfer::EightBit)?;
    // Servers without 8BITMIME get the encoded rendering of the same message.
    let fallback_7bit = if built.has_8bit && !built.needs_smtputf8 {
        Some(build(Transfer::SevenBit)?.bytes)
    } else {
        None
    };
    Ok(Prepared {
        submission: Submission {
            from: built.envelope_from,
            recipients: built.recipients,
            data: built.bytes,
            fallback_7bit,
            smtputf8: built.needs_smtputf8,
        },
    })
}

/// A finished SMTP dialogue as an outbox result, plus the recipients the server refused.
pub fn map_report(report: &SendReport) -> (Delivery, Vec<String>) {
    let rejected: Vec<String> = report.rejected().into_iter().map(str::to_string).collect();
    let delivery = match report.outcome {
        SendOutcome::Accepted => Delivery::Accepted,
        SendOutcome::Unknown => Delivery::Unknown,
        SendOutcome::Rejected { permanent } => Delivery::Refused {
            reason: match report.stage {
                Some(Stage::Rcpt) => FailReason::RecipientsRejected,
                _ if report.code == Some(552) => FailReason::TooLarge,
                _ => FailReason::Rejected,
            },
            retryable: !permanent,
        },
    };
    (delivery, rejected)
}

/// A submission that never reached DATA: always "not sent", with a reason.
pub fn map_error(error: SubmitError) -> Delivery {
    let (reason, retryable) = match error {
        SubmitError::Session(SmtpError::AuthFailed | SmtpError::AuthUnsupported) => {
            (FailReason::AuthFailed, true)
        }
        SubmitError::Session(SmtpError::CertificateInvalid) => (FailReason::Certificate, true),
        SubmitError::Session(SmtpError::InvalidEndpoint) => (FailReason::Network, false),
        SubmitError::Session(SmtpError::InvalidAddress) => (FailReason::InvalidMessage, false),
        SubmitError::Session(
            SmtpError::Unreachable
            | SmtpError::Timeout
            | SmtpError::TlsFailed
            | SmtpError::NotSmtp
            | SmtpError::StartTlsUnavailable,
        ) => (FailReason::Network, true),
        SubmitError::TooLarge { .. } => (FailReason::TooLarge, false),
        SubmitError::SmtpUtf8Unavailable | SubmitError::EightBitUnavailable => {
            (FailReason::Unsupported, false)
        }
        SubmitError::InvalidEnvelope | SubmitError::InvalidMessage => {
            (FailReason::InvalidMessage, false)
        }
    };
    Delivery::Refused { reason, retryable }
}

fn smtp_endpoint(account: &AccountRecord) -> MailServerEndpoint {
    MailServerEndpoint {
        hostname: account.smtp_host.clone(),
        port: account.smtp_port,
        tls_mode: if account.smtp_port == 465 {
            TlsMode::ImplicitTls
        } else {
            TlsMode::StartTls
        },
    }
}

/// The bytes of the last accepted message, kept in memory only, for the Sent-folder copy.
static LAST_SENT: Mutex<Option<(String, Vec<u8>)>> = Mutex::new(None);

struct SmtpTransport {
    account: AccountRecord,
    rejected: Vec<String>,
}

impl Transport for SmtpTransport {
    async fn deliver(&mut self, draft: &DraftRecord) -> Delivery {
        let refuse = |reason, retryable| Delivery::Refused { reason, retryable };
        let keychain = KeychainStore::default();
        // A separate SMTP password wins; otherwise SMTP uses the account password.
        let password = match keychain.get(&smtp_ref(&self.account.id)) {
            Ok(secret) => secret,
            Err(MailboxError::NotFound) => match keychain.get(&self.account.credential_ref) {
                Ok(secret) => secret,
                Err(_) => return refuse(FailReason::AuthFailed, true),
            },
            Err(_) => return refuse(FailReason::Storage, true),
        };
        // Forwarded attachments come from the original messages, fetched again now.
        let forwarded = match forwarded_bytes(draft).await {
            Ok(f) => f,
            Err(reason) => return refuse(reason, reason == FailReason::Network),
        };
        let (record, account) = (draft.clone(), self.account.clone());
        let prepared = tokio::task::spawn_blocking(move || {
            prepare_with(&record, &account, now(), &Limits::default(), &forwarded)
        })
        .await;
        let prepared = match prepared {
            Ok(Ok(p)) => p,
            Ok(Err(reason)) => return refuse(reason, false),
            Err(_) => return refuse(FailReason::Storage, true),
        };
        let result = mailbox_smtp::submit::submit(
            &smtp_endpoint(&self.account),
            &self.account.email_address,
            &password,
            &prepared.submission,
            PartialPolicy::AbortIfAnyRejected,
        )
        .await;
        match result {
            Ok(report) => {
                let (delivery, rejected) = map_report(&report);
                self.rejected = rejected;
                if delivery == Delivery::Accepted {
                    let sent = match (report.used_8bit, prepared.submission.fallback_7bit) {
                        (false, Some(fallback)) => fallback,
                        _ => prepared.submission.data,
                    };
                    if let Ok(mut slot) = LAST_SENT.lock() {
                        *slot = Some((draft.id.clone(), sent));
                    }
                }
                delivery
            }
            Err(e) => map_error(e),
        }
    }
}

/// Result of one send attempt, for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendResult {
    pub state: OutboxState,
    /// Recipients the server refused (addresses the user typed; shown, never logged).
    pub rejected: Vec<String>,
}

/// One explicit send attempt for a stored draft.
pub async fn send(draft_id: &str, intent: SendIntent) -> Result<SendResult, MailboxError> {
    let store = store().await?;
    let record = store.get_draft(OWNER, draft_id).await?;
    let account = store.get_account(OWNER, &record.account_id).await?;
    let mut transport = SmtpTransport {
        account,
        rejected: Vec::new(),
    };
    let state = submit(&store, &mut transport, OWNER, draft_id, intent).await?;
    Ok(SendResult {
        state,
        rejected: transport.rejected,
    })
}

fn imap_endpoint(account: &AccountRecord) -> MailServerEndpoint {
    MailServerEndpoint {
        hostname: account.imap_host.clone(),
        port: account.imap_port,
        tls_mode: TlsMode::ImplicitTls,
    }
}

async fn imap_session(
    account: &AccountRecord,
) -> Result<
    mailbox_imap::Session<impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>,
    AppendError,
> {
    let password = KeychainStore::default()
        .get(&account.credential_ref)
        .map_err(|_| AppendError::Connection(mailbox_imap::ConnectError::AuthFailed))?;
    let mut session =
        mailbox_imap::Session::connect(&imap_endpoint(account), &account.email_address, &password)
            .await?;
    session.refresh_capabilities().await?;
    Ok(session)
}

/// Sent folder from the persisted folder roles (a user override wins), if known.
async fn persisted_sent_folder(account: &AccountRecord) -> Option<String> {
    let store = store().await.ok()?;
    let folders = store.list_folders(OWNER, &account.id).await.ok()?;
    folders
        .iter()
        .find(|f| {
            mailbox_core::folders::role_of(f) == Some(mailbox_core::folders::FolderRole::Sent)
        })
        .map(|f| f.remote_name.clone())
}

/// Sent folder from the server listing (before the first folder sync).
async fn listed_sent_folder<S>(
    session: &mut mailbox_imap::Session<S>,
) -> Result<String, AppendError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let folders = session.list_folders().await?;
    find_folder(&folders, FolderRole::Sent)
        .map(|f| f.name.clone())
        .ok_or(AppendError::Refused)
}

/// Keep exactly one copy in the Sent folder (checks the `Message-ID` first, so it may run twice).
struct SentCopyJob {
    folder: Option<String>,
    message_id: String,
    bytes: Vec<u8>,
}

impl RepeatableJob for SentCopyJob {
    type Output = SentCopy;
    async fn run<'a, S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'a>(
        &'a self,
        session: &'a mut mailbox_imap::Session<S>,
    ) -> Result<SentCopy, AppendError> {
        let folder = match &self.folder {
            Some(f) => f.clone(),
            None => listed_sent_folder(session).await?,
        };
        save_sent_copy(session, &folder, &self.message_id, &self.bytes).await
    }
}

/// Read-only search of the Sent folder by `Message-ID`.
struct FindSentJob {
    folder: Option<String>,
    message_id: String,
}

impl RepeatableJob for FindSentJob {
    type Output = Vec<u32>;
    async fn run<'a, S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'a>(
        &'a self,
        session: &'a mut mailbox_imap::Session<S>,
    ) -> Result<Vec<u32>, AppendError> {
        let folder = match &self.folder {
            Some(f) => f.clone(),
            None => listed_sent_folder(session).await?,
        };
        session.find_by_message_id(&folder, &self.message_id).await
    }
}

/// Run a repeatable IMAP job on the account's held live-sync session (MB-140) when its worker
/// is connected, else on a connection of its own.
async fn on_account<J: RepeatableJob>(
    account: &AccountRecord,
    job: &J,
) -> Result<J::Output, AppendError> {
    let live = super::worker::handle(&account.id);
    live_or_fresh(live.as_ref(), job, || imap_session(account))
        .await
        .0
}

/// After `Sent`: make sure the server's Sent folder holds exactly one copy. Skips the APPEND when
/// the provider already saved the message. Records and returns the result.
pub async fn reconcile_sent_copy(draft_id: &str) -> SentCopyState {
    let outcome = async {
        let store = store().await.map_err(|_| AppendError::Refused)?;
        let record = store
            .get_draft(OWNER, draft_id)
            .await
            .map_err(|_| AppendError::Refused)?;
        let account = store
            .get_account(OWNER, &record.account_id)
            .await
            .map_err(|_| AppendError::Refused)?;
        let cached = LAST_SENT.lock().ok().and_then(|slot| {
            slot.as_ref()
                .filter(|(id, _)| id == draft_id)
                .map(|(_, b)| b.clone())
        });
        let bytes = match cached {
            Some(bytes) => bytes,
            // After a restart the message is rendered again; the Message-ID is the same.
            None => {
                let forwarded = forwarded_bytes(&record)
                    .await
                    .map_err(|_| AppendError::InvalidMessage)?;
                let (r, a) = (record.clone(), account.clone());
                tokio::task::spawn_blocking(move || {
                    prepare_with(&r, &a, now(), &Limits::default(), &forwarded)
                })
                .await
                .map_err(|_| AppendError::Refused)?
                .map_err(|_| AppendError::InvalidMessage)?
                .submission
                .data
            }
        };
        let job = SentCopyJob {
            folder: persisted_sent_folder(&account).await,
            message_id: record.message_id.clone(),
            bytes,
        };
        on_account(&account, &job).await
    }
    .await;
    let state = match outcome {
        Ok(SentCopy::Appended { .. }) => SentCopyState::Saved,
        Ok(SentCopy::AlreadyThere) => SentCopyState::AlreadyThere,
        Err(_) => SentCopyState::Failed,
    };
    if let Ok(store) = store().await {
        let _ = store.set_sent_copy(OWNER, draft_id, state).await;
    }
    state
}

/// For an `Unknown` message: look for it in the server's Sent folder by `Message-ID`.
/// `Some(true)`: found, and the message is now recorded as sent. `Some(false)`: not there (which
/// does not prove it was not sent). `None`: could not check.
pub async fn check_sent_folder(draft_id: &str) -> Option<bool> {
    let store = store().await.ok()?;
    let record = store.get_draft(OWNER, draft_id).await.ok()?;
    let account = store.get_account(OWNER, &record.account_id).await.ok()?;
    let job = FindSentJob {
        folder: persisted_sent_folder(&account).await,
        message_id: record.message_id.clone(),
    };
    let found = !on_account(&account, &job).await.ok()?.is_empty();
    if found && record.state == OutboxState::Unknown {
        decide(&store, OWNER, draft_id, OutboxEvent::MarkSent)
            .await
            .ok()?;
        let _ = store
            .set_sent_copy(OWNER, draft_id, SentCopyState::AlreadyThere)
            .await;
    }
    Some(found)
}

/// Schedule a stored draft for `at` (UTC epoch seconds), or change the time (MB-218).
pub async fn schedule(draft_id: &str, at: i64, now: i64) -> Result<(), MailboxError> {
    mailbox_core::outbox::schedule(&store().await?, OWNER, draft_id, at, now).await
}

/// Cancel a schedule: the message is an editable draft again. Refused once sending started.
pub async fn unschedule(draft_id: &str) -> Result<(), MailboxError> {
    mailbox_core::outbox::unschedule(&store().await?, OWNER, draft_id).await
}

/// Scheduled messages due at `now`, the next send time, and how many were marked missed now.
pub async fn take_due(now: i64) -> Result<(Vec<DraftRecord>, Option<i64>, usize), MailboxError> {
    let store = store().await?;
    let missed_before = missed_count(&store).await?;
    let (due, next) = mailbox_core::outbox::take_due(&store, OWNER, now).await?;
    let missed = missed_count(&store).await?.saturating_sub(missed_before);
    Ok((due, next, missed))
}

async fn missed_count(store: &SqliteStore) -> Result<usize, MailboxError> {
    Ok(store
        .list_drafts(OWNER)
        .await?
        .iter()
        .filter(|d| {
            matches!(
                d.state,
                OutboxState::Failed {
                    reason: FailReason::MissedSchedule,
                    ..
                }
            )
        })
        .count())
}

/// A user decision that needs no network: `MarkSent` or `Edit`.
pub async fn settle(draft_id: &str, event: OutboxEvent) -> Result<OutboxState, MailboxError> {
    decide(&store().await?, OWNER, draft_id, event).await
}

/// Check a separate SMTP password against the server and, only if it is accepted, keep it in
/// the OS keychain. It is never written anywhere else.
pub async fn store_smtp_password(account_id: &str, password: String) -> Result<(), CheckFailure> {
    let store = store().await.map_err(|_| CheckFailure::Storage)?;
    let account = store
        .get_account(OWNER, account_id)
        .await
        .map_err(|_| CheckFailure::Storage)?;
    let secret = Secret::new(password);
    mailbox_smtp::check(
        &smtp_endpoint(&account),
        Some((&account.email_address, &secret)),
    )
    .await
    .map_err(|e| match e {
        SmtpError::AuthFailed => CheckFailure::AuthFailed,
        SmtpError::AuthUnsupported => CheckFailure::AuthUnsupported,
        SmtpError::CertificateInvalid => CheckFailure::CertificateInvalid,
        SmtpError::TlsFailed => CheckFailure::TlsFailed,
        SmtpError::Timeout => CheckFailure::Timeout,
        SmtpError::NotSmtp => CheckFailure::NotSmtp,
        SmtpError::StartTlsUnavailable => CheckFailure::StartTlsUnavailable,
        SmtpError::InvalidEndpoint | SmtpError::InvalidAddress => CheckFailure::InvalidEndpoint,
        SmtpError::Unreachable => CheckFailure::Unreachable,
    })?;
    KeychainStore::default()
        .put(&smtp_ref(account_id), &secret)
        .map_err(|_| CheckFailure::Storage)
}

/// Remove the separate SMTP password of an account. Called by account removal.
pub fn forget_smtp_password(account_id: &str) -> Result<(), MailboxError> {
    KeychainStore::default().delete(&smtp_ref(account_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailbox_core::outbox::AttachmentRef;
    use mailbox_core::store::{FolderRecord, MessageRecord};
    use mailbox_smtp::submit::RcptStatus;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn account() -> AccountRecord {
        AccountRecord {
            id: "acc-t".into(),
            owner_id: OWNER.into(),
            display_name: "example.com".into(),
            email_address: "me@example.com".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 465,
            auth_type: "password".into(),
            credential_ref: "imap.acc-t".into(),
            status: "active".into(),
        }
    }

    fn record() -> DraftRecord {
        DraftRecord {
            id: "d-1".into(),
            owner_id: OWNER.into(),
            account_id: "acc-t".into(),
            message_id: "<t1@example.com>".into(),
            from_identity: "me@example.com".into(),
            to: vec!["\"Trần, Linh\" <linh@company.vn>".into()],
            cc: vec!["an@company.vn".into()],
            bcc: vec!["Sếp <boss@hidden.io>".into()],
            subject: "Kế hoạch".into(),
            body_text: "Xin chào\n".into(),
            ..DraftRecord::default()
        }
    }

    fn temp_file(name: &str, bytes: &[u8]) -> String {
        let dir = std::env::temp_dir().join(format!("mailbox-send-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn prepared_message_keeps_bcc_in_the_envelope_only_and_has_a_7bit_fallback() {
        let p = prepare(&record(), &account(), 1_794_614_400, &Limits::default()).unwrap();
        let s = p.submission;
        assert_eq!(s.from, "me@example.com");
        assert_eq!(
            s.recipients,
            vec!["linh@company.vn", "an@company.vn", "boss@hidden.io"]
        );
        let text = String::from_utf8(s.data.clone()).unwrap();
        assert!(text.contains("Message-ID: <t1@example.com>\r\n"));
        assert!(text.contains("From: me@example.com\r\n"));
        for leak in ["boss@hidden.io", "Bcc", "Sếp"] {
            assert!(!text.contains(leak), "{leak} leaked into the message");
        }
        assert!(
            text.contains("Xin chào"),
            "8-bit rendering for servers with 8BITMIME"
        );
        let fallback = s
            .fallback_7bit
            .expect("encoded rendering for servers without it");
        assert!(fallback.is_ascii() && !s.smtputf8);
    }

    #[test]
    fn attachments_are_read_at_send_time_with_clear_failures() {
        let mut r = record();
        r.attachments = vec![AttachmentRef {
            path: temp_file("ghi chú.txt", b"n\xc3\xb4i dung"),
            name: "ghi chú.txt".into(),
            size_bytes: 9,
            forwarded: None,
        }];
        let p = prepare(&r, &account(), 0, &Limits::default()).unwrap();
        let text = String::from_utf8(p.submission.data).unwrap();
        assert!(text.contains("multipart/mixed") && text.contains("Content-Type: text/plain\r\n"));
        assert!(text.contains("bsO0aSBkdW5n"), "file content, base64");

        // The limit is checked from file metadata before anything is read.
        let small = Limits {
            max_attachment_bytes: 4,
            ..Limits::default()
        };
        assert_eq!(
            prepare(&r, &account(), 0, &small),
            Err(FailReason::AttachmentsTooLarge)
        );

        r.attachments[0].path = "/nonexistent/mailbox/file.txt".into();
        assert_eq!(
            prepare(&r, &account(), 0, &Limits::default()),
            Err(FailReason::AttachmentMissing)
        );
        r.attachments[0].path = std::env::temp_dir().to_string_lossy().into_owned();
        assert_eq!(
            prepare(&r, &account(), 0, &Limits::default()),
            Err(FailReason::AttachmentMissing),
            "a directory is not an attachment"
        );
    }

    #[test]
    fn track_g_forwarded_attachments_use_the_fetched_bytes_and_never_a_guess() {
        let part = ForwardedPart {
            message_id: "acc-t/INBOX#7#5".into(),
            part: 1,
        };
        let mut r = record();
        r.attachments = vec![AttachmentRef {
            path: String::new(),
            name: "báo giá.pdf".into(),
            size_bytes: 6,
            forwarded: Some(part.clone()),
        }];
        let fetched = vec![(part.clone(), b"%PDF-1".to_vec())];
        let p = prepare_with(&r, &account(), 0, &Limits::default(), &fetched).unwrap();
        let text = String::from_utf8(p.submission.data).unwrap();
        assert!(text.contains("multipart/mixed"));
        assert!(text.contains("JVBERi0x"), "the original's bytes, base64");
        // Not fetched (or a different part): refused, nothing is sent without it.
        assert_eq!(
            prepare(&r, &account(), 0, &Limits::default()),
            Err(FailReason::AttachmentMissing)
        );
        let other = vec![(
            ForwardedPart {
                part: 2,
                ..part.clone()
            },
            b"x".to_vec(),
        )];
        assert_eq!(
            prepare_with(&r, &account(), 0, &Limits::default(), &other),
            Err(FailReason::AttachmentMissing)
        );
        let small = Limits {
            max_attachment_bytes: 3,
            ..Limits::default()
        };
        assert_eq!(
            prepare_with(&r, &account(), 0, &small, &fetched),
            Err(FailReason::AttachmentsTooLarge)
        );
    }

    #[test]
    fn a_draft_that_cannot_be_a_valid_message_is_refused_locally() {
        let mut r = record();
        r.from_identity = "ceo@example.com".into();
        assert_eq!(
            prepare(&r, &account(), 0, &Limits::default()),
            Err(FailReason::IdentityMismatch)
        );
        for breakage in [
            |r: &mut DraftRecord| r.to = vec!["not an address".into()],
            |r: &mut DraftRecord| r.subject = "Hi\r\nBcc: victim@evil.io".into(),
            |r: &mut DraftRecord| r.bcc = vec!["a@b.vn\r\nRCPT TO:<x@y.vn>".into()],
            |r: &mut DraftRecord| r.message_id = "<bad id>".into(),
            |r: &mut DraftRecord| {
                r.to.clear();
                r.cc.clear();
                r.bcc.clear();
            },
        ] {
            let mut r = record();
            breakage(&mut r);
            assert_eq!(
                prepare(&r, &account(), 0, &Limits::default()),
                Err(FailReason::InvalidMessage)
            );
        }
    }

    fn report(outcome: SendOutcome, stage: Option<Stage>, code: Option<u16>) -> SendReport {
        SendReport {
            outcome,
            recipients: vec![
                ("a@x.org".into(), RcptStatus::Accepted),
                ("b@y.org".into(), RcptStatus::Rejected { code: 550 }),
            ],
            stage,
            code,
            used_8bit: false,
        }
    }

    #[test]
    fn smtp_results_map_to_outbox_results_without_inventing_success() {
        let refused = |reason, retryable| Delivery::Refused { reason, retryable };
        assert_eq!(
            map_report(&report(SendOutcome::Accepted, None, None)).0,
            Delivery::Accepted
        );
        assert_eq!(
            map_report(&report(SendOutcome::Unknown, Some(Stage::Content), None)).0,
            Delivery::Unknown,
            "a lost connection after DATA is never reported as failed or sent"
        );
        let (delivery, rejected) = map_report(&report(
            SendOutcome::Rejected { permanent: true },
            Some(Stage::Rcpt),
            Some(550),
        ));
        assert_eq!(delivery, refused(FailReason::RecipientsRejected, false));
        assert_eq!(rejected, vec!["b@y.org"]);
        assert_eq!(
            map_report(&report(
                SendOutcome::Rejected { permanent: false },
                Some(Stage::Content),
                Some(451)
            ))
            .0,
            refused(FailReason::Rejected, true)
        );
        assert_eq!(
            map_report(&report(
                SendOutcome::Rejected { permanent: true },
                Some(Stage::Mail),
                Some(552)
            ))
            .0,
            refused(FailReason::TooLarge, false)
        );

        let cases = [
            (
                SubmitError::Session(SmtpError::AuthFailed),
                FailReason::AuthFailed,
                true,
            ),
            (
                SubmitError::Session(SmtpError::Timeout),
                FailReason::Network,
                true,
            ),
            (
                SubmitError::Session(SmtpError::Unreachable),
                FailReason::Network,
                true,
            ),
            (
                SubmitError::Session(SmtpError::CertificateInvalid),
                FailReason::Certificate,
                true,
            ),
            (
                SubmitError::Session(SmtpError::StartTlsUnavailable),
                FailReason::Network,
                true,
            ),
            (
                SubmitError::TooLarge {
                    limit: 1,
                    actual: 2,
                },
                FailReason::TooLarge,
                false,
            ),
            (
                SubmitError::SmtpUtf8Unavailable,
                FailReason::Unsupported,
                false,
            ),
            (
                SubmitError::EightBitUnavailable,
                FailReason::Unsupported,
                false,
            ),
            (
                SubmitError::InvalidEnvelope,
                FailReason::InvalidMessage,
                false,
            ),
        ];
        for (error, reason, retryable) in cases {
            assert_eq!(map_error(error), refused(reason, retryable), "{error:?}");
        }
    }

    #[test]
    fn first_save_fixes_the_identity_and_threads_a_reply() {
        rt().block_on(async {
            let store = SqliteStore::in_memory().await.unwrap();
            store.upsert_account(&account()).await.unwrap();
            let inbox = FolderRecord {
                id: "acc-t/INBOX".into(),
                account_id: "acc-t".into(),
                remote_name: "INBOX".into(),
                delimiter: None,
                special_use: Some("inbox".into()),
                uid_validity: Some(1),
                uid_next: None,
            };
            store.upsert_folder(OWNER, &inbox).await.unwrap();
            let original = MessageRecord {
                id: "acc-t/INBOX#1#7".into(),
                account_id: "acc-t".into(),
                folder_id: inbox.id.clone(),
                uid_validity: 1,
                imap_uid: 7,
                message_id_header: Some("<orig@sender.org>".into()),
                subject: "Hỏi".into(),
                from_display: String::new(),
                from_address: "a@sender.org".into(),
                preview: String::new(),
                internal_date: 1,
                size_bytes: 1,
                seen: true,
                flagged: false,
            };
            store.upsert_messages(OWNER, &[original]).await.unwrap();

            let mut new = record();
            new.id.clear();
            new.message_id.clear();
            let reply = Some(("acc-t/INBOX#1#7".to_string(), ReplyKind::Reply));
            let saved = save_in(&store, new.clone(), reply).await.unwrap();
            assert!(saved.id.starts_with("d-") && saved.version == 1);
            assert!(
                saved.message_id.starts_with('<') && saved.message_id.ends_with("@example.com>")
            );
            assert_eq!(saved.in_reply_to.as_deref(), Some("<orig@sender.org>"));
            assert_eq!(saved.references, vec!["<orig@sender.org>"]);

            // Later saves (autosave) keep the id and the Message-ID and move the version on.
            let mut again = new.clone();
            (again.id, again.version, again.message_id) =
                (saved.id.clone(), 1, saved.message_id.clone());
            again.in_reply_to = saved.in_reply_to.clone();
            again.references = saved.references.clone();
            again.subject = "Kế hoạch (sửa)".into();
            let second = save_in(&store, again, None).await.unwrap();
            assert_eq!((second.id.as_str(), second.version), (saved.id.as_str(), 2));
            assert_eq!(second.message_id, saved.message_id);
            let stored = store.get_draft(OWNER, &saved.id).await.unwrap();
            assert_eq!(stored.subject, "Kế hoạch (sửa)");
            assert_eq!(stored.in_reply_to.as_deref(), Some("<orig@sender.org>"));

            // A forward of an unknown message gets no invented threading; two drafts differ.
            let other = save_in(&store, new, Some(("nope".into(), ReplyKind::Forward)))
                .await
                .unwrap();
            assert!(other.in_reply_to.is_none() && other.references.is_empty());
            assert_ne!(other.message_id, saved.message_id);
            assert_ne!(other.id, saved.id);
        });
    }

    /// MB-201: a reply continues the original's stored `References` chain.
    #[test]
    fn a_reply_carries_the_full_references_chain_of_the_original() {
        rt().block_on(async {
            let store = SqliteStore::in_memory().await.unwrap();
            store.upsert_account(&account()).await.unwrap();
            let inbox = FolderRecord {
                id: "acc-t/INBOX".into(),
                account_id: "acc-t".into(),
                remote_name: "INBOX".into(),
                delimiter: None,
                special_use: Some("inbox".into()),
                uid_validity: Some(1),
                uid_next: None,
            };
            store.upsert_folder(OWNER, &inbox).await.unwrap();
            store
                .upsert_messages(
                    OWNER,
                    &[MessageRecord {
                        id: "acc-t/INBOX#1#9".into(),
                        account_id: "acc-t".into(),
                        folder_id: inbox.id.clone(),
                        uid_validity: 1,
                        imap_uid: 9,
                        message_id_header: Some("<c@x.org>".into()),
                        subject: "Re: Re: Kế hoạch".into(),
                        from_display: String::new(),
                        from_address: "a@x.org".into(),
                        preview: String::new(),
                        internal_date: 1,
                        size_bytes: 1,
                        seen: true,
                        flagged: false,
                    }],
                )
                .await
                .unwrap();
            store
                .store_headers(
                    OWNER,
                    &[mailbox_core::threading::HeaderRow {
                        message_id: "acc-t/INBOX#1#9".into(),
                        account_id: "acc-t".into(),
                        headers: mailbox_core::threading::EnvelopeHeaders {
                            in_reply_to: Some("<b@x.org>".into()),
                            references: vec!["<a@x.org>".into(), "<b@x.org>".into()],
                            ..Default::default()
                        },
                    }],
                )
                .await
                .unwrap();
            let mut new = record();
            new.id.clear();
            new.message_id.clear();
            let saved = save_in(
                &store,
                new.clone(),
                Some(("acc-t/INBOX#1#9".into(), ReplyKind::Reply)),
            )
            .await
            .unwrap();
            assert_eq!(saved.in_reply_to.as_deref(), Some("<c@x.org>"));
            assert_eq!(
                saved.references,
                vec!["<a@x.org>", "<b@x.org>", "<c@x.org>"]
            );
            let forward = save_in(
                &store,
                new,
                Some(("acc-t/INBOX#1#9".into(), ReplyKind::Forward)),
            )
            .await
            .unwrap();
            assert!(forward.in_reply_to.is_none());
            assert_eq!(forward.references.len(), 3);
        });
    }

    #[test]
    fn tokens_and_keychain_references_are_safe() {
        let (a, b) = (unique_token(), unique_token());
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '.'));
        assert!(new_message_id(&a, "me@example.com").is_some());
        let reference = smtp_ref(&super::super::account::account_id("a/b c@x.y"));
        assert!(mailbox_core::secrets::is_valid_reference(&reference));
        assert_ne!(
            reference, "imap.acc-a-b-c-x-y",
            "separate from the IMAP entry"
        );
    }
}
