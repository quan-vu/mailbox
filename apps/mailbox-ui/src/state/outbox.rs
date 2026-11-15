//! Real-mode composer and outbox state (MB-133, MB-150, MB-151): what the user sees while a
//! draft is saved and a message is sent. The work itself is in `adapters::send` (Desktop only);
//! on the Web build every action here is a no-op and nothing claims to have been sent.
//!
//! The state is a global signal so this file needs no field in `AppState`.
// The Web build compiles the state but has no sender, so the Desktop-only helpers are unused there.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]
use super::AppState;
use super::compose::{Draft, DraftAttachment, parse_recipient};
use crate::i18n::tr;
use dioxus::prelude::*;
use mailbox_core::outbox::{
    AttachmentRef, DraftRecord, FailReason, OutboxState, SendIntent, SentCopyState,
};

/// True when this build can save drafts and send (Desktop).
pub const CAN_SEND: bool = cfg!(not(target_arch = "wasm32"));
/// Typing pauses this long before the draft is saved.
pub const AUTOSAVE_DELAY_MS: u64 = 800;

/// Where the send of the open draft stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SendPhase {
    #[default]
    Idle,
    /// Waiting for its send time (MB-218); content is frozen until the schedule is cancelled.
    Scheduled {
        at: Option<i64>,
    },
    Sending,
    Sent {
        copy: SentCopyState,
    },
    /// Definitely not sent.
    Failed {
        reason: FailReason,
        retryable: bool,
        /// Recipients the server refused.
        rejected: Vec<String>,
    },
    /// May have been sent. `checked`: result of looking in the server's Sent folder, if done.
    Unknown {
        checked: Option<bool>,
    },
}

impl SendPhase {
    pub fn key(&self) -> &'static str {
        match self {
            SendPhase::Idle => "idle",
            SendPhase::Scheduled { .. } => "scheduled",
            SendPhase::Sending => "sending",
            SendPhase::Sent { .. } => "sent",
            SendPhase::Failed { .. } => "failed",
            SendPhase::Unknown { .. } => "unknown",
        }
    }

    /// The draft can be edited only before a send starts.
    pub fn locks_editing(&self) -> bool {
        *self != SendPhase::Idle
    }

    pub fn from_state(state: OutboxState, copy: SentCopyState) -> Self {
        match state {
            OutboxState::Draft => SendPhase::Idle,
            OutboxState::Scheduled => SendPhase::Scheduled { at: None },
            OutboxState::Queued | OutboxState::Sending => SendPhase::Sending,
            OutboxState::Sent => SendPhase::Sent { copy },
            OutboxState::Failed { reason, retryable } => SendPhase::Failed {
                reason,
                retryable,
                rejected: Vec::new(),
            },
            OutboxState::Unknown => SendPhase::Unknown { checked: None },
        }
    }

    /// The phase of a stored record, including its send time when scheduled.
    pub fn from_record(record: &DraftRecord) -> Self {
        match record.state {
            OutboxState::Scheduled => SendPhase::Scheduled {
                at: record.scheduled_at,
            },
            state => Self::from_state(state, record.sent_copy),
        }
    }
}

/// What the autosave indicator says. It only says "saved" after the store confirmed a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SaveStatus {
    /// Nothing to save yet.
    #[default]
    Idle,
    /// Changed since the last save.
    Pending,
    Saving,
    Saved,
    Failed,
}

impl SaveStatus {
    pub fn key(self) -> &'static str {
        match self {
            SaveStatus::Idle => "empty",
            SaveStatus::Pending => "unsaved",
            SaveStatus::Saving => "saving",
            SaveStatus::Saved => "saved",
            SaveStatus::Failed => "save-failed",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SaveStatus::Idle => tr("Bản nháp sẽ được lưu trên máy này khi bạn bắt đầu viết"),
            SaveStatus::Pending => tr("Có thay đổi chưa lưu"),
            SaveStatus::Saving => tr("Đang lưu bản nháp…"),
            SaveStatus::Saved => tr("Đã lưu bản nháp trên máy này"),
            SaveStatus::Failed => {
                tr("Chưa lưu được bản nháp — nội dung vẫn ở đây, đừng đóng ứng dụng")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OutboxUi {
    /// Draft the `phase` belongs to; another draft is always `Idle`.
    pub draft_id: String,
    pub phase: SendPhase,
    pub save: SaveStatus,
    /// Content of the last successful save (ids cleared), to tell real changes from our own
    /// write-back of the draft id.
    pub saved_content: Option<Draft>,
    /// Local drafts and outbox messages, newest first.
    pub items: Vec<DraftRecord>,
    pub items_loaded: bool,
    pub items_error: bool,
    /// The SMTP server refused the account password: ask for a separate one.
    pub ask_smtp_password: bool,
    pub smtp_password_error: Option<&'static str>,
    /// A network action (check, password) is running.
    pub busy: bool,
}

pub static OUTBOX: GlobalSignal<OutboxUi> = Signal::global(OutboxUi::default);

/// Send phase of the given draft.
pub fn phase_for(draft_id: &str) -> SendPhase {
    let ui = OUTBOX.read();
    if !draft_id.is_empty() && ui.draft_id == draft_id {
        ui.phase.clone()
    } else {
        SendPhase::Idle
    }
}

/// User-facing reason and the safe next step. Never technical detail, never a TLS bypass.
pub fn fail_message(reason: FailReason) -> &'static str {
    match reason {
        FailReason::Rejected => tr(
            "Máy chủ gửi thư từ chối thư này. Thư chưa được gửi. Kiểm tra nội dung hoặc thử lại sau.",
        ),
        FailReason::RecipientsRejected => tr(
            "Máy chủ từ chối một số địa chỉ nhận nên thư chưa được gửi cho ai. Sửa hoặc gỡ các địa chỉ bị từ chối rồi gửi lại.",
        ),
        FailReason::AuthFailed => tr(
            "Máy chủ gửi thư không chấp nhận mật khẩu đã lưu. Thư chưa được gửi. Nhập mật khẩu gửi thư (SMTP) để thử lại.",
        ),
        FailReason::Network => tr(
            "Không kết nối được tới máy chủ gửi thư. Thư chưa được gửi. Kiểm tra mạng rồi thử lại.",
        ),
        FailReason::Certificate => tr(
            "Chứng chỉ của máy chủ gửi thư không hợp lệ nên MailBox dừng lại. Thư chưa được gửi. MailBox không cho phép bỏ qua kiểm tra này.",
        ),
        FailReason::TooLarge => tr(
            "Thư lớn hơn giới hạn của máy chủ gửi thư. Thư chưa được gửi. Gỡ bớt tệp đính kèm rồi gửi lại.",
        ),
        FailReason::Unsupported => tr(
            "Máy chủ gửi thư không hỗ trợ địa chỉ có ký tự ngoài bảng mã ASCII. Thư chưa được gửi. Dùng địa chỉ không dấu.",
        ),
        FailReason::InvalidMessage => {
            tr("Thư có địa chỉ hoặc tiêu đề không hợp lệ. Thư chưa được gửi. Sửa thư rồi gửi lại.")
        }
        FailReason::AttachmentMissing => tr(
            "Không đọc được một tệp đính kèm (tệp đã bị xóa, đổi tên hoặc chuyển chỗ). Thư chưa được gửi. Sửa thư và đính kèm lại.",
        ),
        FailReason::AttachmentsTooLarge => {
            tr("Tệp đính kèm vượt giới hạn 25 MB. Thư chưa được gửi. Gỡ bớt tệp rồi gửi lại.")
        }
        FailReason::IdentityMismatch => tr(
            "Địa chỉ gửi không khớp với tài khoản đăng nhập. Thư chưa được gửi. Chọn lại tài khoản gửi.",
        ),
        FailReason::Interrupted => tr(
            "Ứng dụng đã đóng trước khi thư được chuyển cho máy chủ. Thư chưa được gửi. Bấm Thử lại để gửi.",
        ),
        FailReason::Storage => tr(
            "Không ghi được dữ liệu trên máy này (cơ sở dữ liệu cục bộ hoặc Keychain). Thư chưa được gửi. Thử lại.",
        ),
        FailReason::MissedSchedule => tr(
            "Đã quá giờ hẹn hơn 12 giờ trong lúc MailBox đóng hoặc máy ngủ, nên thư chưa được gửi. Bấm Thử lại để gửi ngay, hoặc Sửa để hẹn giờ khác.",
        ),
    }
}

pub fn sent_copy_message(copy: SentCopyState) -> &'static str {
    match copy {
        SentCopyState::None | SentCopyState::Pending => {
            tr("Đang lưu bản sao vào thư mục Đã gửi trên máy chủ…")
        }
        SentCopyState::Saved => tr("Đã lưu một bản sao vào thư mục Đã gửi trên máy chủ."),
        SentCopyState::AlreadyThere => {
            tr("Máy chủ đã tự lưu thư vào thư mục Đã gửi; MailBox không tạo bản thứ hai.")
        }
        SentCopyState::Failed => {
            tr("Thư đã được gửi, nhưng chưa lưu được bản sao vào thư mục Đã gửi trên máy chủ.")
        }
    }
}

/// (label, key) of an outbox entry's state.
pub fn state_label(state: OutboxState) -> (&'static str, &'static str) {
    match state {
        OutboxState::Draft => (tr("Bản nháp"), "draft"),
        OutboxState::Scheduled => (tr("Đã hẹn giờ"), "scheduled"),
        OutboxState::Queued | OutboxState::Sending => (tr("Đang gửi"), "sending"),
        OutboxState::Sent => (tr("Đã gửi"), "sent"),
        OutboxState::Failed { .. } => (tr("Chưa gửi được"), "failed"),
        OutboxState::Unknown => (tr("Có thể đã gửi"), "unknown"),
    }
}

/// The draft's content without the identity the store gave it.
pub fn content_of(draft: &Draft) -> Draft {
    Draft {
        id: String::new(),
        version: 0,
        message_id: String::new(),
        in_reply_to: None,
        references: Vec::new(),
        ..draft.clone()
    }
}

/// The composer draft as a store record. Invalid chips are kept as typed so nothing is lost;
/// they block sending, not saving.
pub fn to_record(draft: &Draft, owner: &str, from_address: &str) -> DraftRecord {
    let entries = |list: &[super::compose::Recipient]| list.iter().map(|r| r.entry()).collect();
    DraftRecord {
        id: draft.id.clone(),
        owner_id: owner.to_string(),
        account_id: draft.from_account.clone(),
        version: draft.version,
        message_id: draft.message_id.clone(),
        from_identity: from_address.to_string(),
        to: entries(&draft.to),
        cc: entries(&draft.cc),
        bcc: entries(&draft.bcc),
        subject: draft.subject.clone(),
        body_text: draft.body.clone(),
        body_html: None,
        in_reply_to: draft.in_reply_to.clone(),
        references: draft.references.clone(),
        attachments: draft
            .attachments
            .iter()
            .filter_map(|a| match (&a.path, &a.forwarded) {
                (Some(path), _) => Some(AttachmentRef {
                    path: path.clone(),
                    name: a.name.clone(),
                    size_bytes: a.size,
                    forwarded: None,
                }),
                (None, Some(part)) => Some(AttachmentRef {
                    path: String::new(),
                    name: a.name.clone(),
                    size_bytes: a.size,
                    forwarded: Some(part.clone()),
                }),
                (None, None) => None,
            })
            .collect(),
        state: OutboxState::Draft,
        sent_copy: SentCopyState::None,
        updated_at: 0,
        scheduled_at: None,
    }
}

/// A stored record back in the composer.
pub fn from_record(record: &DraftRecord) -> Draft {
    let chips = |list: &[String]| list.iter().filter_map(|e| parse_recipient(e)).collect();
    Draft {
        from_account: record.account_id.clone(),
        to: chips(&record.to),
        cc: chips(&record.cc),
        bcc: chips(&record.bcc),
        subject: record.subject.clone(),
        body: record.body_text.clone(),
        attachments: record
            .attachments
            .iter()
            .map(|a| DraftAttachment {
                name: a.name.clone(),
                size: a.size_bytes,
                path: (a.forwarded.is_none()).then(|| a.path.clone()),
                forwarded: a.forwarded.clone(),
            })
            .collect(),
        reply_account: None,
        id: record.id.clone(),
        version: record.version,
        message_id: record.message_id.clone(),
        reply_source: None,
        in_reply_to: record.in_reply_to.clone(),
        references: record.references.clone(),
    }
}

fn from_address(app: &AppState, account_id: &str) -> Option<String> {
    app.mail
        .peek()
        .accounts
        .iter()
        .find(|a| a.id == account_id)
        .map(|a| a.address.clone())
}

fn set_phase(draft_id: &str, phase: SendPhase) {
    let mut ui = OUTBOX.write();
    ui.draft_id = draft_id.to_string();
    ui.phase = phase;
}

/// Forget the send state of the previous message (new draft, mode switch).
pub fn reset_composer() {
    let mut ui = OUTBOX.write();
    ui.draft_id.clear();
    ui.phase = SendPhase::Idle;
    ui.save = SaveStatus::Idle;
    ui.saved_content = None;
    ui.ask_smtp_password = false;
    ui.smtp_password_error = None;
}

/// Note that the draft changed; returns true when a save should be scheduled.
/// Reads with `peek` only, so the effect calling this depends on the draft alone.
pub fn note_change(app: &AppState) -> bool {
    let draft = app.draft.peek().clone();
    if !app.is_real() || !CAN_SEND {
        return false;
    }
    if draft.is_empty() && draft.id.is_empty() {
        return false;
    }
    let content = content_of(&draft);
    let (locked, unchanged, status) = {
        let ui = OUTBOX.peek();
        (
            !draft.id.is_empty() && ui.draft_id == draft.id && ui.phase.locks_editing(),
            ui.saved_content.as_ref() == Some(&content),
            ui.save,
        )
    };
    if locked || unchanged {
        return false;
    }
    if !matches!(status, SaveStatus::Saving | SaveStatus::Pending) {
        OUTBOX.write().save = SaveStatus::Pending;
    }
    true
}

/// Save the open draft now. Returns true when the store holds the current content.
/// Changes typed while a save is running are saved by the same call before it returns.
#[allow(unused_mut, unused_variables)]
pub async fn save_now(mut app: AppState) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !app.is_real() {
            return false;
        }
        if OUTBOX.peek().save == SaveStatus::Saving {
            // The running save loop picks up the newest content.
            return false;
        }
        loop {
            let draft = app.draft.peek().clone();
            let content = content_of(&draft);
            if OUTBOX.peek().saved_content.as_ref() == Some(&content) && !draft.id.is_empty() {
                OUTBOX.write().save = SaveStatus::Saved;
                return true;
            }
            let Some(address) = from_address(&app, &draft.from_account) else {
                OUTBOX.write().save = SaveStatus::Failed;
                return false;
            };
            OUTBOX.write().save = SaveStatus::Saving;
            let record = to_record(&draft, crate::adapters::account::OWNER, &address);
            match crate::adapters::send::save(record, draft.reply_source.clone()).await {
                Ok(saved) => {
                    {
                        // Only the identity is written back: what the user typed meanwhile stays.
                        let mut d = app.draft.write();
                        d.id = saved.id.clone();
                        d.version = saved.version;
                        d.message_id = saved.message_id;
                        d.in_reply_to = saved.in_reply_to;
                        d.references = saved.references;
                    }
                    let mut ui = OUTBOX.write();
                    ui.saved_content = Some(content);
                    ui.items_loaded = false;
                    if ui.draft_id != saved.id {
                        ui.draft_id = saved.id;
                        ui.phase = SendPhase::Idle;
                    }
                }
                Err(_) => {
                    OUTBOX.write().save = SaveStatus::Failed;
                    return false;
                }
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    false
}

/// Debounced autosave: wait for a pause in typing, then save.
#[allow(unused_variables)]
pub async fn autosave_after_pause(app: AppState, generation: Signal<u64>, mine: u64) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::time::sleep(std::time::Duration::from_millis(AUTOSAVE_DELAY_MS)).await;
        if *generation.peek() == mine {
            save_now(app).await;
        }
    }
}

/// One explicit send attempt for the open draft, then the Sent-folder copy.
#[allow(unused_variables)]
pub async fn send(app: AppState, intent: SendIntent) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !app.is_real() {
            return;
        }
        // A draft is saved first so the outbox sends exactly what is on screen.
        if intent == SendIntent::Send && !save_now(app).await {
            let id = app.draft.peek().id.clone();
            if !id.is_empty() {
                set_phase(
                    &id,
                    SendPhase::Failed {
                        reason: FailReason::Storage,
                        retryable: false,
                        rejected: Vec::new(),
                    },
                );
            }
            return;
        }
        let id = app.draft.peek().id.clone();
        if id.is_empty() || phase_for(&id) == SendPhase::Sending {
            return;
        }
        set_phase(&id, SendPhase::Sending);
        let result = crate::adapters::send::send(&id, intent).await;
        finish_send(&id, result, true).await;
        refresh_items().await;
    }
}

/// Show the result of a send attempt and save the Sent copy. `open`: the draft is the one in the
/// composer (otherwise the composer's phase is left alone).
#[cfg(not(target_arch = "wasm32"))]
async fn finish_send(
    id: &str,
    result: Result<crate::adapters::send::SendResult, mailbox_core::MailboxError>,
    open: bool,
) -> Option<OutboxState> {
    let is_open = || open || OUTBOX.peek().draft_id == id;
    match result {
        Ok(result) => {
            let mut phase = SendPhase::from_state(result.state, SentCopyState::Pending);
            if let SendPhase::Failed {
                reason, rejected, ..
            } = &mut phase
            {
                *rejected = result.rejected;
                if *reason == FailReason::AuthFailed && is_open() {
                    OUTBOX.write().ask_smtp_password = true;
                }
            }
            let sent = result.state == OutboxState::Sent;
            if is_open() {
                set_phase(id, phase);
            }
            if sent {
                let copy = crate::adapters::send::reconcile_sent_copy(id).await;
                if is_open() && phase_for(id).key() == "sent" {
                    set_phase(id, SendPhase::Sent { copy });
                }
            }
            Some(result.state)
        }
        // The outbox refused to start (already in flight, or the store failed). Show what
        // the store says rather than guessing.
        Err(_) => {
            if is_open() {
                show_stored_state(id).await;
            }
            None
        }
    }
}

/// Schedule the open draft for `at` (UTC epoch seconds). The draft is saved first so exactly
/// what is on screen is scheduled. Returns a user-facing error, if any.
#[allow(unused_variables)]
pub async fn schedule_send(app: AppState, at: i64) -> Result<(), &'static str> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use mailbox_core::outbox::check_schedule;
        if !app.is_real() {
            return Err(tr("Bản demo không gửi thư."));
        }
        let now = super::schedule::now_epoch();
        check_schedule(at, now).map_err(super::schedule::error_text)?;
        if !save_now(app).await {
            return Err(tr("Chưa lưu được bản nháp nên chưa hẹn giờ. Thử lại."));
        }
        let id = app.draft.peek().id.clone();
        if id.is_empty() {
            return Err(tr("Chưa lưu được bản nháp nên chưa hẹn giờ. Thử lại."));
        }
        let result = crate::adapters::send::schedule(&id, at, now).await;
        refresh_items().await;
        match result {
            Ok(()) => {
                set_phase(&id, SendPhase::Scheduled { at: Some(at) });
                Ok(())
            }
            Err(_) => {
                show_stored_state(&id).await;
                Err(tr(
                    "Không hẹn giờ được: thư có thể đã bắt đầu gửi. Xem trạng thái bên dưới.",
                ))
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    Err(tr("Bản web không gửi thư."))
}

/// Cancel the schedule of the open draft: it becomes an editable draft again. Nothing is sent.
#[allow(unused_variables)]
pub async fn cancel_schedule(app: AppState) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let id = app.draft.peek().id.clone();
        let ok = cancel_item(&id).await;
        if ok {
            set_phase(&id, SendPhase::Idle);
            OUTBOX.write().save = SaveStatus::Saved;
        } else {
            show_stored_state(&id).await;
        }
        ok
    }
    #[cfg(target_arch = "wasm32")]
    false
}

/// Cancel the schedule of an outbox entry (outbox list). Nothing is sent.
#[allow(unused_variables)]
pub async fn cancel_item(draft_id: &str) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let ok = crate::adapters::send::unschedule(draft_id).await.is_ok();
        if ok && OUTBOX.peek().draft_id == draft_id {
            OUTBOX.write().phase = SendPhase::Idle;
        }
        refresh_items().await;
        ok
    }
    #[cfg(target_arch = "wasm32")]
    false
}

/// Send one due scheduled message (called by the schedule loop). Tells the user the result
/// with a toast; the composer shows it too when this draft is open.
#[cfg(not(target_arch = "wasm32"))]
pub async fn send_scheduled(app: AppState, draft_id: &str) {
    if OUTBOX.peek().draft_id == draft_id {
        set_phase(draft_id, SendPhase::Sending);
    }
    let result = crate::adapters::send::send(draft_id, SendIntent::Scheduled).await;
    match finish_send(draft_id, result, false).await {
        Some(OutboxState::Sent) => app.notify(tr("Đã gửi một thư hẹn giờ."), false),
        Some(OutboxState::Unknown) => app.notify(
            tr("Thư hẹn giờ có thể đã gửi hoặc chưa (mất kết nối giữa chừng). Mở Bản nháp để kiểm tra; MailBox không tự gửi lại."),
            false,
        ),
        Some(OutboxState::Failed { .. }) => app.notify(
            tr("Thư hẹn giờ chưa gửi được. Mở Bản nháp để xem lý do và thử lại."),
            false,
        ),
        _ => {}
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn show_stored_state(id: &str) {
    let stored = crate::adapters::send::list()
        .await
        .ok()
        .and_then(|items| items.into_iter().find(|d| d.id == id));
    match stored {
        Some(d) => set_phase(id, SendPhase::from_record(&d)),
        None => set_phase(
            id,
            SendPhase::Failed {
                reason: FailReason::Storage,
                retryable: false,
                rejected: Vec::new(),
            },
        ),
    }
}

/// Reload the drafts/outbox list from the local store.
pub async fn refresh_items() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let result = crate::adapters::send::list().await;
        let mut ui = OUTBOX.write();
        ui.items_loaded = true;
        match result {
            Ok(items) => {
                ui.items = items;
                ui.items_error = false;
            }
            Err(_) => ui.items_error = true,
        }
    }
}

/// Unknown outcome: look in the server's Sent folder for this message.
#[allow(unused_variables)]
pub async fn check_sent_folder(app: AppState) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let id = app.draft.peek().id.clone();
        if !matches!(phase_for(&id), SendPhase::Unknown { .. }) || OUTBOX.peek().busy {
            return;
        }
        OUTBOX.write().busy = true;
        let found = crate::adapters::send::check_sent_folder(&id).await;
        OUTBOX.write().busy = false;
        match found {
            Some(true) => set_phase(
                &id,
                SendPhase::Sent {
                    copy: SentCopyState::AlreadyThere,
                },
            ),
            other => set_phase(
                &id,
                SendPhase::Unknown {
                    checked: other.or(Some(false)).filter(|_| other.is_some()),
                },
            ),
        }
        refresh_items().await;
    }
}

/// Unknown outcome: the user confirmed the message arrived. Nothing is sent.
#[allow(unused_variables)]
pub async fn mark_sent(app: AppState) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use mailbox_core::outbox::OutboxEvent;
        let id = app.draft.peek().id.clone();
        if crate::adapters::send::settle(&id, OutboxEvent::MarkSent)
            .await
            .is_ok()
        {
            set_phase(
                &id,
                SendPhase::Sent {
                    copy: SentCopyState::None,
                },
            );
            let copy = crate::adapters::send::reconcile_sent_copy(&id).await;
            set_phase(&id, SendPhase::Sent { copy });
        }
        refresh_items().await;
    }
}

/// Failed or unknown: back to an editable draft. Nothing is sent.
#[allow(unused_variables)]
pub async fn edit_again(app: AppState) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use mailbox_core::outbox::OutboxEvent;
        let id = app.draft.peek().id.clone();
        if crate::adapters::send::settle(&id, OutboxEvent::Edit)
            .await
            .is_ok()
        {
            set_phase(&id, SendPhase::Idle);
            let mut ui = OUTBOX.write();
            ui.ask_smtp_password = false;
            ui.save = SaveStatus::Saved;
        }
        refresh_items().await;
    }
}

/// Verify and store a separate SMTP password in the keychain, then retry the send. The retry is
/// part of the same explicit action ("save and send").
#[allow(unused_variables)]
pub async fn save_smtp_password_and_retry(app: AppState, password: String) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let account = app.draft.peek().from_account.clone();
        if password.is_empty() || OUTBOX.peek().busy {
            return;
        }
        OUTBOX.write().busy = true;
        let result = crate::adapters::send::store_smtp_password(&account, password).await;
        OUTBOX.write().busy = false;
        match result {
            Ok(()) => {
                {
                    let mut ui = OUTBOX.write();
                    ui.ask_smtp_password = false;
                    ui.smtp_password_error = None;
                }
                send(app, SendIntent::Retry).await;
            }
            Err(e) => OUTBOX.write().smtp_password_error = Some(e.message()),
        }
    }
}

/// Delete a local draft or outbox entry (after the user confirmed). The server is untouched.
#[allow(unused_mut, unused_variables)]
pub async fn discard(mut app: AppState, draft_id: String) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let deleted =
            draft_id.is_empty() || crate::adapters::send::discard(&draft_id).await.is_ok();
        if deleted {
            if app.draft.peek().id == draft_id {
                let from = app.default_from.peek().clone();
                app.draft.set(Draft::new(&from));
                reset_composer();
            }
            refresh_items().await;
        }
        deleted
    }
    #[cfg(target_arch = "wasm32")]
    false
}

/// Open a stored draft or outbox entry in the composer, showing its real state.
pub fn open_item(mut app: AppState, record: &DraftRecord) {
    let draft = from_record(record);
    {
        let mut ui = OUTBOX.write();
        ui.draft_id = record.id.clone();
        ui.phase = SendPhase::from_record(record);
        ui.saved_content = Some(content_of(&draft));
        ui.save = SaveStatus::Saved;
        ui.ask_smtp_password = false;
        ui.smtp_password_error = None;
    }
    app.draft.set(draft);
    app.go(super::Screen::Compose);
}

/// Forward: what MailBox found out about the original's attachments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardFiles {
    Looking,
    /// This many attachments of the original were added to the draft.
    Added(usize),
    /// The original has no attachments.
    NoFiles,
    /// The original's body is not downloaded on this device, so its attachments are unknown.
    NotDownloaded,
    Failed,
}

impl ForwardFiles {
    pub fn key(self) -> &'static str {
        match self {
            ForwardFiles::Looking => "looking",
            ForwardFiles::Added(_) => "added",
            ForwardFiles::NoFiles => "none",
            ForwardFiles::NotDownloaded => "not-downloaded",
            ForwardFiles::Failed => "failed",
        }
    }

    pub fn message(self) -> Option<String> {
        match self {
            ForwardFiles::Looking | ForwardFiles::NoFiles => None,
            ForwardFiles::Added(n) => Some(crate::i18n::bi(
                format!(
                    "Đã thêm {n} tệp đính kèm của thư gốc. MailBox tải lại chúng từ máy chủ lúc gửi."
                ),
                format!(
                    "Added {n} attachment(s) from the original message. MailBox fetches them from the server again when sending."
                ),
            )),
            ForwardFiles::NotDownloaded => Some(
                tr("Nội dung thư gốc chưa được tải về máy này nên MailBox chưa biết thư có tệp đính kèm hay không: tệp đính kèm (nếu có) chưa được chuyển tiếp. Mở thư gốc để tải nội dung, rồi bấm Chuyển tiếp lại.")
                    .to_string(),
            ),
            ForwardFiles::Failed => Some(
                tr("Không đọc được danh sách tệp đính kèm của thư gốc trên máy này; tệp đính kèm chưa được chuyển tiếp.")
                    .to_string(),
            ),
        }
    }
}

/// (original message id, result) of the last forward lookup.
pub static FORWARD_FILES: GlobalSignal<Option<(String, ForwardFiles)>> = Signal::global(|| None);

/// The attachments to add for a forward, from the cached body's list. Inline parts (images
/// referenced from the HTML) are left out: the forward is sent as plain text.
pub fn forward_attachments(
    source_id: &str,
    list: &[mailbox_core::body::AttachmentMeta],
) -> Vec<DraftAttachment> {
    list.iter()
        .filter(|a| !a.inline)
        .map(|a| DraftAttachment {
            name: a.filename.clone(),
            size: a.size_bytes,
            path: None,
            forwarded: Some(mailbox_core::outbox::ForwardedPart {
                message_id: source_id.to_string(),
                part: a.part,
            }),
        })
        .collect()
}

/// Real mode, forward: add the original's attachments from the cached body (no network). The
/// composer calls it once per opened forward; parts already in the draft are not added twice.
#[allow(unused_mut)]
pub async fn attach_forwarded(mut app: AppState) {
    use super::compose::ReplyKind;
    let Some((source, ReplyKind::Forward)) = app.draft.peek().reply_source.clone() else {
        return;
    };
    let looking = FORWARD_FILES
        .peek()
        .as_ref()
        .is_some_and(|(s, f)| *s == source && *f == ForwardFiles::Looking);
    if !app.is_real() || looking {
        return;
    }
    *FORWARD_FILES.write() = Some((source.clone(), ForwardFiles::Looking));
    #[cfg(not(target_arch = "wasm32"))]
    let status = match crate::adapters::body::cached_attachments(&source).await {
        Ok(Some(list)) => {
            let files = forward_attachments(&source, &list);
            let n = files.len();
            let same = app.draft.peek().reply_source.as_ref().map(|(s, _)| s) == Some(&source);
            if same && n > 0 {
                let mut d = app.draft.write();
                for f in files {
                    if !d.attachments.iter().any(|a| a.forwarded == f.forwarded) {
                        d.attachments.push(f);
                    }
                }
            }
            if n > 0 {
                ForwardFiles::Added(n)
            } else {
                ForwardFiles::NoFiles
            }
        }
        Ok(None) => ForwardFiles::NotDownloaded,
        Err(_) => ForwardFiles::Failed,
    };
    #[cfg(target_arch = "wasm32")]
    let status = ForwardFiles::NotDownloaded;
    *FORWARD_FILES.write() = Some((source, status));
}

/// Start a fresh message after one was sent.
pub fn new_message(mut app: AppState) {
    let from = app.default_from.peek().clone();
    app.draft.set(Draft::new(&from));
    reset_composer();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::compose::RecipientField;

    #[test]
    fn every_failure_says_not_sent_and_none_offers_a_tls_bypass() {
        for reason in FailReason::ALL {
            let m = fail_message(reason).to_lowercase();
            assert!(
                m.contains("chưa được gửi"),
                "{reason:?} must say the mail was not sent"
            );
            for risky in [
                "tắt ssl",
                "tắt tls",
                "không mã hóa",
                "cổng 25",
                "tin cậy chứng chỉ",
                "vẫn tiếp tục",
            ] {
                assert!(!m.contains(risky), "{reason:?} suggests `{risky}`");
            }
        }
        assert!(fail_message(FailReason::Certificate).contains("không cho phép bỏ qua"));
    }

    #[test]
    fn phases_follow_the_stored_state() {
        let failed = OutboxState::Failed {
            reason: FailReason::Network,
            retryable: true,
        };
        let cases = [
            (OutboxState::Draft, "idle", false),
            (OutboxState::Queued, "sending", true),
            (OutboxState::Sending, "sending", true),
            (OutboxState::Sent, "sent", true),
            (failed, "failed", true),
            (OutboxState::Unknown, "unknown", true),
        ];
        for (state, key, locked) in cases {
            let phase = SendPhase::from_state(state, SentCopyState::Pending);
            assert_eq!(
                (phase.key(), phase.locks_editing()),
                (key, locked),
                "{state:?}"
            );
        }
        // An interrupted send is shown as uncertain, never as a plain draft or as sent.
        assert_eq!(
            SendPhase::from_state(OutboxState::Unknown, SentCopyState::None),
            SendPhase::Unknown { checked: None }
        );
        assert_eq!(state_label(OutboxState::Unknown).0, "Có thể đã gửi");
    }

    #[test]
    fn the_save_indicator_only_says_saved_for_the_saved_state() {
        let all = [
            SaveStatus::Idle,
            SaveStatus::Pending,
            SaveStatus::Saving,
            SaveStatus::Saved,
            SaveStatus::Failed,
        ];
        for status in all {
            let claims_saved = status.label().starts_with("Đã lưu");
            assert_eq!(claims_saved, status == SaveStatus::Saved, "{status:?}");
        }
        assert!(SaveStatus::Failed.label().contains("Chưa lưu được"));
    }

    #[test]
    fn drafts_round_trip_through_the_store_record() {
        let mut d = Draft::new("acc-x");
        d.add_recipient(
            RecipientField::To,
            "\"Trần, Linh\" <linh@company.vn>, an@company.vn",
        );
        d.add_recipient(RecipientField::Bcc, "Sếp <boss@company.vn>");
        d.add_recipient(RecipientField::Cc, "broken@");
        d.subject = "Kế hoạch".into();
        d.body = "Xin chào\n".into();
        d.id = "d-1".into();
        d.version = 3;
        d.message_id = "<d-1@company.vn>".into();
        d.in_reply_to = Some("<o@x>".into());
        d.references = vec!["<o@x>".into()];
        d.attachments = vec![
            DraftAttachment {
                name: "a.pdf".into(),
                size: 10,
                path: Some("/tmp/a.pdf".into()),
                forwarded: None,
            },
            DraftAttachment {
                name: "demo.pdf".into(),
                size: 5,
                path: None,
                forwarded: None,
            },
        ];
        let record = to_record(&d, "local", "me@company.vn");
        assert_eq!(
            record.to,
            vec!["\"Trần, Linh\" <linh@company.vn>", "an@company.vn"]
        );
        assert_eq!(record.bcc, vec!["\"Sếp\" <boss@company.vn>"]);
        assert_eq!(
            (record.from_identity.as_str(), record.account_id.as_str()),
            ("me@company.vn", "acc-x")
        );
        assert_eq!(
            record.attachments.len(),
            1,
            "only files that exist on this device are stored"
        );
        assert_eq!(record.state, OutboxState::Draft);

        let back = from_record(&record);
        assert_eq!(back.to, d.to);
        assert_eq!(back.bcc, d.bcc);
        assert!(
            !back.cc[0].valid,
            "an invalid chip stays visible and invalid after a restart"
        );
        assert_eq!(
            (back.id.as_str(), back.version, back.message_id.as_str()),
            ("d-1", 3, "<d-1@company.vn>")
        );
        assert_eq!(back.in_reply_to, d.in_reply_to);
        assert_eq!(content_of(&back).id, "");
        assert_eq!(
            content_of(&d),
            content_of(&Draft {
                id: "other".into(),
                version: 9,
                ..d.clone()
            })
        );
    }

    #[test]
    fn track_g_forward_takes_real_attachments_and_skips_inline_images() {
        use mailbox_core::body::AttachmentMeta;
        let meta = |part, inline| AttachmentMeta {
            part,
            filename: format!("f{part}"),
            media_type: "application/pdf".into(),
            size_bytes: 10,
            content_id: None,
            inline,
        };
        let files = forward_attachments("m-1", &[meta(0, true), meta(1, false), meta(2, false)]);
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.sendable() && f.path.is_none()));
        assert_eq!(files[0].forwarded.as_ref().unwrap().part, 1);
        // Stored and read back as a forwarded reference, not as a local file.
        let mut d = Draft::new("acc-x");
        d.attachments = files;
        let record = to_record(&d, "local", "me@example.com");
        assert!(
            record
                .attachments
                .iter()
                .all(|a| a.forwarded.is_some() && a.path.is_empty())
        );
        assert_eq!(from_record(&record).attachments, d.attachments);
        assert!(
            d.real_send_blockers()
                .iter()
                .all(|b| !b.contains("tệp đính kèm"))
        );
        assert!(
            ForwardFiles::NotDownloaded
                .message()
                .unwrap()
                .contains("chưa được tải về")
        );
    }
}
