//! UI state. Pure logic (`mail`, `compose`, `connect`) is renderer-independent and unit-tested;
//! `AppState` wires it into Dioxus signals shared through context.
use crate::i18n::tr;
pub mod compose;
pub mod connect;
pub mod demo;
pub mod folders;
pub mod link;
pub mod live;
pub mod mail;
pub mod outbox;
pub mod prefs;
pub mod schedule;
pub mod search;
pub mod shortcuts;

use crate::adapters::connect::CheckFailure;
use crate::adapters::{DataError, MailboxDataSource, PlatformSource, now_or_never};
use compose::{Draft, TEMPLATES};
use dioxus::prelude::*;
use mail::{Filter, MailState};
use mailbox_contracts::BootstrapResponse;
use std::collections::BTreeMap;

/// Navigation intents; `routes::Route` maps them 1:1 to URLs (docs/03-ui/SCREENS.md).
/// `Thread` carries a message id of the conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Inbox,
    Thread(String),
    Compose,
    Connect,
    Settings(SettingsTab),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Accounts,
    Sync,
    Appearance,
    Security,
    Signature,
    Notifications,
    Integrations,
    Advanced,
    About,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 9] = [
        SettingsTab::Accounts,
        SettingsTab::Sync,
        SettingsTab::Appearance,
        SettingsTab::Security,
        SettingsTab::Signature,
        SettingsTab::Notifications,
        SettingsTab::Integrations,
        SettingsTab::Advanced,
        SettingsTab::About,
    ];

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == key)
    }

    pub fn key(self) -> &'static str {
        match self {
            SettingsTab::Accounts => "accounts",
            SettingsTab::Sync => "sync",
            SettingsTab::Appearance => "appearance",
            SettingsTab::Security => "security",
            SettingsTab::Signature => "signature",
            SettingsTab::Notifications => "notifications",
            SettingsTab::Integrations => "integrations",
            SettingsTab::Advanced => "advanced",
            SettingsTab::About => "about",
        }
    }

    /// (title, helper, icon)
    pub fn meta(self) -> (&'static str, &'static str, &'static str) {
        match self {
            SettingsTab::Accounts => (tr("Tài khoản"), tr("Tài khoản và danh tính"), "user"),
            SettingsTab::Sync => (tr("Đồng bộ"), tr("Trạng thái, lịch đồng bộ"), "refresh"),
            SettingsTab::Appearance => (tr("Giao diện"), tr("Chủ đề, mật độ, cỡ chữ"), "palette"),
            SettingsTab::Security => (tr("Bảo mật"), tr("Phiên, ảnh từ xa"), "shield"),
            SettingsTab::Signature => (tr("Chữ ký"), tr("Chữ ký theo danh tính"), "pen"),
            SettingsTab::Notifications => (tr("Thông báo"), tr("Email, desktop, âm thanh"), "bell"),
            SettingsTab::Integrations => (tr("Tiện ích tích hợp"), "Sau MVP", "puzzle"),
            SettingsTab::Advanced => (tr("Nâng cao"), tr("Phím tắt, bộ nhớ đệm"), "sliders"),
            SettingsTab::About => (tr("Giới thiệu MailBox"), tr("Phiên bản, giấy phép"), "info"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    Light,
    Dark,
    /// Follows the OS; the demo resolves it to light until MB-105 adds media-query sync.
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub message: String,
    pub undo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    DiscardDraft,
    ApplyTemplate(&'static str),
    ApplyUserTemplate(String),
    ReplaceDraft(Box<Draft>),
    DetachAccount(String),
    /// Real mode: permanently delete a message that is in the trash folder.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    DeleteForever(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub title: String,
    pub body: String,
    pub ok_label: &'static str,
    pub danger: bool,
    pub action: ConfirmAction,
}

pub fn default_toggles() -> BTreeMap<&'static str, bool> {
    BTreeMap::from([
        ("auto-sync", true),
        ("reduce-motion", false),
        ("remote-images", false),
        ("app-lock", false),
        ("signature-reply", true),
        ("new-mail", true),
        ("desktop", false),
        ("show-content", false),
        ("sound", false),
        ("sync-errors", true),
        ("shortcuts", true),
    ])
}

/// Demo shows illustrative fixtures. Real shows only what real servers returned: until mail
/// sync exists (MB-121) that means an empty mailbox plus the real connection check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppMode {
    #[default]
    Demo,
    Real,
}

/// Where the initial mailbox snapshot stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadState {
    Loading,
    Ready,
    Failed(DataError),
}

impl LoadState {
    pub fn key(&self) -> &'static str {
        match self {
            LoadState::Loading => "loading",
            LoadState::Ready => "ready",
            LoadState::Failed(_) => "failed",
        }
    }
}

#[derive(Clone, Copy)]
pub struct AppState {
    pub mode: Signal<AppMode>,
    pub load: Signal<LoadState>,
    pub mail: Signal<MailState>,
    pub filter: Signal<Filter>,
    pub selected: Signal<Option<String>>,
    /// Mobile only: reader pane replaces the list.
    pub reader_open: Signal<bool>,
    pub theme: Signal<Theme>,
    pub toast: Signal<Option<Toast>>,
    pub confirm: Signal<Option<Confirm>>,
    pub send_dialog: Signal<bool>,
    pub draft: Signal<Draft>,
    pub default_from: Signal<String>,
    pub menu_open: Signal<bool>,
    pub mobile_search: Signal<bool>,
    pub detached: Signal<Vec<String>>,
    pub toggles: Signal<BTreeMap<&'static str, bool>>,
}

impl AppState {
    pub fn init() -> Self {
        let mut state = Self {
            mode: Signal::new(AppMode::Demo),
            load: Signal::new(LoadState::Loading),
            mail: Signal::new(MailState::new(Vec::new(), Vec::new())),
            filter: Signal::new(Filter::default()),
            selected: Signal::new(None),
            reader_open: Signal::new(false),
            theme: Signal::new(Theme::Light),
            toast: Signal::new(None),
            confirm: Signal::new(None),
            send_dialog: Signal::new(false),
            draft: Signal::new(Draft::default()),
            default_from: Signal::new(String::new()),
            menu_open: Signal::new(false),
            mobile_search: Signal::new(false),
            detached: Signal::new(Vec::new()),
            toggles: Signal::new(default_toggles()),
        };
        // In-process sources (Desktop) are ready before the first frame; remote ones load in `reload`.
        if PlatformSource::LOCAL
            && let Some(result) = now_or_never(PlatformSource.bootstrap())
        {
            state.apply(result);
        }
        state
    }

    fn apply(&mut self, result: Result<BootstrapResponse, DataError>) {
        match result {
            Ok(data) => {
                let first_account = data
                    .accounts
                    .first()
                    .map(|a| a.id.clone())
                    .unwrap_or_default();
                self.selected
                    .set(data.messages.first().map(|m| m.id.clone()));
                self.mail.set(MailState::new(data.accounts, data.messages));
                if self.default_from.peek().is_empty() {
                    self.default_from.set(first_account.clone());
                    self.draft.set(Draft::new(&first_account));
                }
                self.load.set(LoadState::Ready);
            }
            Err(e) => self.load.set(LoadState::Failed(e)),
        }
    }

    /// (Re)load the mailbox snapshot from the platform data source.
    pub async fn reload(mut self) {
        self.load.set(LoadState::Loading);
        let result = PlatformSource.bootstrap().await;
        self.apply(result);
    }

    pub fn is_real(&self) -> bool {
        *self.mode.read() == AppMode::Real
    }

    /// Switch between illustrative data and real-only mode. Session state that belongs to the
    /// other mode (selection, filters, draft, detached demo accounts) is dropped, so demo
    /// content can never appear under the "real" label.
    pub async fn set_mode(mut self, mode: AppMode) {
        if *self.mode.peek() == mode {
            return;
        }
        self.mode.set(mode);
        self.filter.set(Filter::default());
        self.selected.set(None);
        self.reader_open.set(false);
        self.detached.set(Vec::new());
        self.default_from.set(String::new());
        self.draft.set(Draft::default());
        match mode {
            AppMode::Real => {
                self.reload_real().await;
                self.notify(tr("Chế độ thật: không dùng dữ liệu minh họa."), false);
            }
            AppMode::Demo => {
                // Demo mode talks to no server: stop the background sync (MB-140).
                #[cfg(not(target_arch = "wasm32"))]
                crate::adapters::worker::stop_all();
                self.reload().await;
                self.notify(tr("Chế độ demo: đang hiển thị dữ liệu minh họa."), false);
            }
        }
    }

    /// Enter real mode with a given snapshot of the local store (empty when no account is saved).
    pub fn enter_real_with(&mut self, data: BootstrapResponse) {
        self.mode.set(AppMode::Real);
        self.default_from.set(String::new());
        self.apply(Ok(data));
    }

    /// Reload real mode from the local store. On the Web build real mode has no data source yet.
    pub async fn reload_real(mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        let result = crate::adapters::account::load_real().await;
        #[cfg(target_arch = "wasm32")]
        let result: Result<BootstrapResponse, DataError> = Ok(BootstrapResponse {
            mode: "real".into(),
            accounts: Vec::new(),
            messages: Vec::new(),
        });
        match result {
            Ok(data) => {
                self.enter_real_with(data);
                // Settings first: the stored auto-sync choice decides whether workers start.
                self.load_prefs().await;
                // One live sync worker per saved account (MB-140).
                #[cfg(not(target_arch = "wasm32"))]
                crate::adapters::worker::ensure_running().await;
            }
            Err(e) => {
                self.mode.set(AppMode::Real);
                self.mail.set(MailState::new(Vec::new(), Vec::new()));
                self.load.set(LoadState::Failed(e));
            }
        }
    }

    /// Desktop start-up: if real accounts are saved on this device, open in real mode.
    #[allow(unused_mut)] // only mutated on native targets
    pub async fn restore_real_if_any(mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(data) = crate::adapters::account::load_real().await
            && !data.accounts.is_empty()
        {
            self.filter.set(Filter::default());
            self.detached.set(Vec::new());
            self.enter_real_with(data);
            self.load_prefs().await;
            crate::adapters::worker::ensure_running().await;
        }
    }

    /// Save a verified account (keychain + local database), run the first sync, show real mode.
    /// Returns (folders found, messages stored).
    #[allow(clippy::too_many_arguments)]
    pub async fn save_account(
        self,
        email: String,
        imap_host: String,
        imap_port: u16,
        smtp_host: String,
        smtp_port: u16,
        password: String,
    ) -> Result<(usize, usize), CheckFailure> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::adapters::account::{NewAccount, account_id, save_and_sync};
            let id = account_id(&email);
            let summary = save_and_sync(
                NewAccount {
                    email,
                    imap_host,
                    imap_port,
                    smtp_host,
                    smtp_port,
                },
                password,
            )
            .await?;
            self.reload_real().await;
            // Re-entered password for an account whose worker waits on it: go now (MB-141).
            crate::adapters::worker::retry_now(&id);
            Ok((summary.folders, summary.new_messages))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (email, imap_host, imap_port, smtp_host, smtp_port, password);
            Err(CheckFailure::Unsupported)
        }
    }

    /// Fetch new mail for every saved account, then refresh the view.
    pub async fn sync_now(self) {
        // With live workers the refresh button asks them for a full sync: the status line shows
        // progress and the list updates in place, keeping the open message (MB-140).
        #[cfg(not(target_arch = "wasm32"))]
        if crate::adapters::worker::sync_all() {
            self.notify(tr("Đang đồng bộ tất cả thư mục với máy chủ…"), false);
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        match crate::adapters::account::resync_all().await {
            Ok(n) => {
                self.reload_real().await;
                self.notify(
                    if n == 0 {
                        tr("Không có thư mới.").to_string()
                    } else {
                        crate::i18n::bi(
                            format!("Đã tải {n} thư mới."),
                            format!("Loaded {n} new message{}.", crate::i18n::s(n)),
                        )
                    },
                    false,
                );
            }
            Err(e) => self.notify(e.message(), false),
        }
    }

    /// Remove a saved account from this device (local cache + keychain). The server is untouched.
    pub async fn remove_real_account(self, account_id: String) {
        #[cfg(not(target_arch = "wasm32"))]
        match crate::adapters::account::remove_account(&account_id).await {
            Ok(()) => {
                self.reload_real().await;
                self.notify(
                    tr("Đã gỡ tài khoản khỏi máy này. Thư trên máy chủ không bị xóa."),
                    false,
                );
            }
            Err(e) => self.notify(e.message(), false),
        }
        #[cfg(target_arch = "wasm32")]
        let _ = account_id;
    }

    pub fn source_key(&self) -> &'static str {
        PlatformSource::KEY
    }

    pub fn is_dark(&self) -> bool {
        *self.theme.read() == Theme::Dark
    }

    /// Navigate to a screen through the router so the URL and history stay in sync.
    pub fn go(mut self, screen: Screen) {
        self.menu_open.set(false);
        navigator().push(crate::routes::Route::from(screen));
    }

    pub fn notify(mut self, message: impl Into<String>, undo: bool) {
        self.toast.set(Some(Toast {
            message: message.into(),
            undo,
        }));
    }

    /// Open a conversation in the reader (desktop) or full-screen pane (mobile).
    pub fn open_message(mut self, id: &str) {
        let key = self
            .mail
            .read()
            .message(id)
            .map(|m| m.thread_key().to_string());
        if let Some(key) = key {
            self.mail.write().mark_read(&key);
        }
        self.selected.set(Some(id.to_string()));
        self.reader_open.set(true);
    }

    /// Start composing. Never silently replaces a non-empty draft.
    pub fn start_compose(mut self, next: Option<Draft>) {
        let current_empty = self.draft.read().is_empty();
        match next {
            None => {
                if current_empty {
                    let from = self.default_from.read().clone();
                    self.draft.set(Draft::new(&from));
                }
                self.go(Screen::Compose);
            }
            Some(d) if current_empty => {
                self.draft.set(d);
                self.go(Screen::Compose);
            }
            Some(d) => {
                self.confirm.set(Some(Confirm {
                    title: tr("Thay bản nháp hiện tại?").into(),
                    body: tr("Bạn đang có một bản nháp chưa gửi. Tiếp tục sẽ thay nó bằng thư trả lời mới.").into(),
                    ok_label: tr("Thay bản nháp"),
                    danger: true,
                    action: ConfirmAction::ReplaceDraft(Box::new(d)),
                }));
            }
        }
    }

    pub fn apply_template(mut self, key: &'static str) {
        if let Some(t) = TEMPLATES.iter().find(|t| t.key == key) {
            let mut d = self.draft.write();
            d.subject = tr(t.subject).to_string();
            d.body = tr(t.body).to_string();
        }
    }

    pub fn run_confirm(mut self) {
        let Some(c) = self.confirm.take() else { return };
        match c.action {
            ConfirmAction::DiscardDraft => {
                let from = self.default_from.read().clone();
                self.draft.set(Draft::new(&from));
                self.go(Screen::Inbox);
            }
            ConfirmAction::ApplyTemplate(key) => self.apply_template(key),
            ConfirmAction::ApplyUserTemplate(id) => self.apply_user_template(&id),
            ConfirmAction::ReplaceDraft(d) => {
                self.draft.set(*d);
                self.go(Screen::Compose);
            }
            ConfirmAction::DeleteForever(id) => folders::delete_forever(self, &id),
            ConfirmAction::DetachAccount(id) if self.is_real() => {
                spawn(self.remove_real_account(id));
            }
            ConfirmAction::DetachAccount(id) => {
                self.detached.write().push(id);
                self.notify(
                    tr("Đã gỡ tài khoản khỏi MailBox (demo). Thư trên máy chủ không bị xóa."),
                    false,
                );
            }
        }
    }

    /// Escape closes the top-most overlay. Returns true when something was closed.
    pub fn escape(mut self) -> bool {
        if self.confirm.read().is_some() {
            self.confirm.set(None);
        } else if *self.send_dialog.read() {
            self.send_dialog.set(false);
        } else if *self.menu_open.read() {
            self.menu_open.set(false);
        } else {
            return false;
        }
        true
    }
}
