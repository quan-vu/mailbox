//! Real-mode folders and message actions (MB-122/143/132/144).
//!
//! Screens call the functions here for star, read/unread, archive, delete, move and undo. In
//! Demo mode they do exactly what the demo always did (local session state, "(demo)" labels).
//! In Real mode they change the local cache, queue the change for the mail server through
//! `adapters::actions`, and show the row as pending until the server confirms or refuses.
// Real mode has no data source on the Web build yet, so parts of this are native-only.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]
use super::AppState;
use super::mail::{Filter, Folder, ThreadRow};
#[cfg(not(target_arch = "wasm32"))]
use super::{Confirm, ConfirmAction};
pub use crate::adapters::actions::{RealFolder, RowOp};
use crate::i18n::tr;
use dioxus::prelude::*;
use std::collections::BTreeMap;

/// Which real folder the message list shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RealView {
    /// Inboxes of all accounts (the unified view).
    #[default]
    Inbox,
    /// A role across accounts: `sent`, `drafts`, `archive`, `trash`, `junk`.
    Role(String),
    /// One of the user's own folders, by folder id.
    Folder(String),
}

impl RealView {
    /// `folder_key` is `MessageSummary::folder_id`: a role key or a folder id.
    pub fn matches(&self, folder_key: &str) -> bool {
        match self {
            RealView::Inbox => folder_key == "inbox",
            RealView::Role(role) => folder_key == role,
            RealView::Folder(id) => folder_key == id,
        }
    }
}

/// Role folders offered in the sidebar after the inbox, in this order.
pub const NAV_ROLES: [&str; 5] = ["sent", "drafts", "archive", "trash", "junk"];

pub fn role_label(role: &str) -> &'static str {
    match role {
        "inbox" => tr("Hộp thư đến"),
        "sent" => tr("Đã gửi"),
        "drafts" => tr("Bản nháp"),
        "archive" => tr("Lưu trữ"),
        "trash" => tr("Thùng rác"),
        "junk" => tr("Thư rác"),
        _ => tr("Thư mục"),
    }
}

pub fn role_icon(role: &str) -> &'static str {
    match role {
        "sent" => "send",
        "drafts" => "file",
        "archive" => "archive",
        "trash" => "trash",
        "junk" => "ban",
        _ => "inbox",
    }
}

/// Roles that at least one account really has, in sidebar order.
pub fn nav_roles(folders: &[RealFolder]) -> Vec<&'static str> {
    NAV_ROLES
        .into_iter()
        .filter(|role| folders.iter().any(|f| f.role.as_deref() == Some(role)))
        .collect()
}

/// A `data-testid`-safe form of a folder id.
pub fn slug(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

/// Rows of the selected real folder. The starred view spans folders but leaves out trash and junk.
pub fn filter_rows(rows: Vec<ThreadRow>, view: &RealView, starred: bool) -> Vec<ThreadRow> {
    rows.into_iter()
        .filter(|r| {
            let key = r.head.folder_id.as_str();
            if starred {
                key != "trash" && key != "junk"
            } else {
                view.matches(key)
            }
        })
        .collect()
}

pub fn unread_in(rows: &[ThreadRow], folder_key: &str, account: Option<&str>) -> usize {
    rows.iter()
        .filter(|r| r.unread && r.head.folder_id == folder_key)
        .filter(|r| account.is_none_or(|a| r.head.account_id == a))
        .count()
}

/// Real-mode UI state next to `AppState`. Filled from the local store; never holds demo data.
#[derive(Clone, Copy)]
pub struct RealState {
    pub folders: Signal<Vec<RealFolder>>,
    pub view: Signal<RealView>,
    /// Message id → state of its last change.
    pub ops: Signal<BTreeMap<String, RowOp>>,
    /// The move the toast's "Hoàn tác" refers to: (operation id, account id).
    pub undo: Signal<Option<(String, String)>>,
}

impl RealState {
    pub fn init() -> Self {
        Self {
            folders: Signal::new(Vec::new()),
            view: Signal::new(RealView::default()),
            ops: Signal::new(BTreeMap::new()),
            undo: Signal::new(None),
        }
    }

    /// The state provided by `App`, if any (component tests may render without it).
    pub fn get() -> Option<Self> {
        try_consume_context::<RealState>()
    }
}

fn real_of(app: AppState) -> Option<RealState> {
    if app.is_real() {
        RealState::get()
    } else {
        None
    }
}

/// The message list for the current real folder; Demo mode passes rows through untouched.
pub fn rows_in_view(app: AppState, folder: Folder, rows: Vec<ThreadRow>) -> Vec<ThreadRow> {
    if !app.is_real() {
        return rows;
    }
    let view = RealState::get()
        .map(|r| r.view.read().clone())
        .unwrap_or_default();
    filter_rows(rows, &view, folder == Folder::Starred)
}

/// Title of the list when a real folder other than the unified inbox is shown.
pub fn view_title(app: AppState, filter: &Filter) -> Option<String> {
    let real = real_of(app)?;
    if filter.folder != Folder::Inbox || filter.account.is_some() {
        return None;
    }
    match &*real.view.read() {
        RealView::Inbox => None,
        RealView::Role(role) => Some(role_label(role).to_string()),
        RealView::Folder(id) => real
            .folders
            .read()
            .iter()
            .find(|f| &f.folder_id == id)
            .map(|f| f.label.clone()),
    }
}

/// `pending`, `failed` or `none`, for the row's `data-op` attribute.
pub fn row_op_key(message_id: &str) -> &'static str {
    match RealState::get().and_then(|r| r.ops.read().get(message_id).copied()) {
        Some(RowOp::Pending) => "pending",
        Some(RowOp::Failed) => "failed",
        None => "none",
    }
}

/// Show the unified inbox again (used when another sidebar entry takes over).
pub fn show_inbox() {
    if let Some(mut real) = RealState::get() {
        real.view.set(RealView::Inbox);
    }
}

/// Reload folder list and row states from the local store.
pub async fn refresh_meta(real: RealState) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use crate::adapters::actions;
        let (mut folders, mut ops) = (real.folders, real.ops);
        folders.set(actions::real_folders().await);
        ops.set(actions::row_ops().await);
    }
    #[cfg(target_arch = "wasm32")]
    let _ = real;
}

/// Reload mail, folders and row states from the local store, keeping the open message open.
/// Unlike `AppState::reload_real` this never touches the draft.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn refresh(app: AppState, real: RealState) {
    use super::AppMode;
    use super::mail::MailState;
    if let Ok(data) = crate::adapters::account::load_real().await
        && *app.mode.peek() == AppMode::Real
    {
        let (mut mail, mut selected, mut reader_open) = (app.mail, app.selected, app.reader_open);
        let open = selected.peek().clone();
        let still_there = open
            .as_ref()
            .is_some_and(|id| data.messages.iter().any(|m| &m.id == id));
        mail.set(MailState::new(data.accounts, data.messages));
        if open.is_some() && !still_there {
            selected.set(None);
            reader_open.set(false);
        }
    }
    refresh_meta(real).await;
}

#[cfg(not(target_arch = "wasm32"))]
fn send_flag(
    app: AppState,
    real: RealState,
    message_id: String,
    flag: mailbox_core::actions::MessageFlag,
    on: bool,
) {
    use crate::adapters::actions;
    let mut ops = real.ops;
    ops.write().insert(message_id.clone(), RowOp::Pending);
    spawn(async move {
        match actions::queue_flag(&message_id, flag, on).await {
            Ok(Some(account)) => {
                if let Some(message) = actions::flush(&account).await.message() {
                    app.notify(message, false);
                }
            }
            Ok(None) => {}
            Err(problem) => app.notify(problem.message(), false),
        }
        refresh(app, real).await;
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn send_move(
    app: AppState,
    real: RealState,
    message_id: String,
    dest: crate::adapters::actions::Destination,
    done: String,
) {
    use crate::adapters::actions;
    spawn(async move {
        let queued = match actions::queue_move(&message_id, dest).await {
            Ok(queued) => queued,
            Err(problem) => {
                app.notify(problem.message(), false);
                return;
            }
        };
        let mut undo = real.undo;
        undo.set(Some((queued.op_id.clone(), queued.account_id.clone())));
        // The row leaves its folder at once; the toast offers to take it back.
        refresh(app, real).await;
        app.notify(done, true);
        let flush = actions::flush(&queued.account_id).await;
        refresh(app, real).await;
        if let Some(message) = flush.message() {
            // A refused move is already back in place: there is nothing left to undo.
            let can_undo = flush.rejected == 0 && flush.dropped == 0;
            if !can_undo {
                undo.set(None);
            }
            app.notify(message, can_undo);
        }
    });
}

fn thread_ids(app: AppState, thread: &str) -> Vec<(String, String)> {
    // Real mode: the conversation's messages in the same folder (MB-201).
    if app.is_real() {
        let ids = super::search::same_folder_conversation(app, thread);
        if !ids.is_empty() {
            return ids;
        }
    }
    app.mail
        .peek()
        .thread(thread)
        .into_iter()
        .map(|m| (m.id, m.folder_id))
        .collect()
}

/// Star or unstar one message.
pub fn toggle_star(app: AppState, message_id: &str) {
    let on = !app
        .mail
        .peek()
        .message(message_id)
        .is_some_and(|m| m.starred);
    let mut mail = app.mail;
    mail.write().toggle_star(message_id);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(real) = real_of(app) {
        send_flag(
            app,
            real,
            message_id.to_string(),
            mailbox_core::actions::MessageFlag::Flagged,
            on,
        );
    }
    let _ = on;
}

/// Mark a conversation read or unread.
pub fn set_unread(app: AppState, thread: &str, unread: bool) {
    let mut mail = app.mail;
    mail.write().set_thread_unread(thread, unread);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(real) = real_of(app) {
        for (id, _) in thread_ids(app, thread) {
            send_flag(
                app,
                real,
                id,
                mailbox_core::actions::MessageFlag::Seen,
                !unread,
            );
        }
    }
}

/// A message was opened in the reader: in Real mode that marks it read on the server too.
pub fn opened(app: AppState, message_id: &str, was_unread: bool) {
    #[cfg(not(target_arch = "wasm32"))]
    if was_unread && let Some(real) = real_of(app) {
        send_flag(
            app,
            real,
            message_id.to_string(),
            mailbox_core::actions::MessageFlag::Seen,
            true,
        );
    }
    let _ = (app, message_id, was_unread);
}

fn close_reader(app: AppState) {
    let (mut selected, mut reader_open) = (app.selected, app.reader_open);
    selected.set(None);
    reader_open.set(false);
}

/// Archive a conversation. `from_reader` closes the reader pane (the inbox reader does, the
/// full-screen thread view navigates away itself).
pub fn archive(app: AppState, thread: &str, from_reader: bool) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(real) = real_of(app) {
        for (id, _) in thread_ids(app, thread) {
            send_move(
                app,
                real,
                id,
                crate::adapters::actions::Destination::Archive,
                tr("Đã lưu trữ thư.").to_string(),
            );
        }
        return;
    }
    let mut mail = app.mail;
    mail.write().archive(thread);
    if from_reader {
        close_reader(app);
    }
    app.notify(tr("Đã lưu trữ hội thoại (demo)"), true);
}

/// Delete a conversation: move it to the trash. Mail already in the trash asks before it is
/// deleted for good.
pub fn trash(app: AppState, thread: &str, from_reader: bool) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(real) = real_of(app) {
        for (id, folder_key) in thread_ids(app, thread) {
            if folder_key == "trash" {
                let mut confirm = app.confirm;
                confirm.set(Some(Confirm {
                    title: tr("Xóa vĩnh viễn thư này?").into(),
                    body: tr(
                        "Thư sẽ bị xóa khỏi Thùng rác trên máy chủ thư và không thể khôi phục.",
                    )
                    .into(),
                    ok_label: tr("Xóa vĩnh viễn"),
                    danger: true,
                    action: ConfirmAction::DeleteForever(id),
                }));
            } else {
                send_move(
                    app,
                    real,
                    id,
                    crate::adapters::actions::Destination::Trash,
                    tr("Đã chuyển thư vào Thùng rác.").to_string(),
                );
            }
        }
        return;
    }
    let mut mail = app.mail;
    mail.write().trash(thread);
    if from_reader {
        close_reader(app);
    }
    app.notify(tr("Đã chuyển vào Thùng rác (demo)"), true);
}

/// Move one message to a real folder chosen in the "Chuyển đến…" menu. Real mode only.
pub fn move_to(app: AppState, message_id: &str, folder_id: &str, label: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(real) = real_of(app) {
        send_move(
            app,
            real,
            message_id.to_string(),
            crate::adapters::actions::Destination::Folder(folder_id.to_string()),
            crate::i18n::bi(
                format!("Đã chuyển thư sang \"{label}\"."),
                format!("Moved the message to \"{label}\"."),
            ),
        );
    }
    let _ = (app, message_id, folder_id, label);
}

/// Permanent delete after the confirm dialog. Real mode, trash folder only.
pub fn delete_forever(app: AppState, message_id: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(real) = real_of(app) {
        use crate::adapters::actions;
        let id = message_id.to_string();
        spawn(async move {
            match actions::queue_delete_forever(&id).await {
                Ok(queued) => {
                    refresh(app, real).await;
                    let flush = actions::flush(&queued.account_id).await;
                    app.notify(
                        flush.message().unwrap_or(tr("Đã xóa vĩnh viễn thư.")),
                        false,
                    );
                }
                Err(problem) => app.notify(problem.message(), false),
            }
            refresh(app, real).await;
        });
    }
    let _ = (app, message_id);
}

/// The toast's "Hoàn tác".
pub fn undo(app: AppState) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(real) = real_of(app) {
        use crate::adapters::actions;
        let mut pending = real.undo;
        let Some((op_id, account_id)) = pending.take() else {
            return;
        };
        spawn(async move {
            match actions::undo(&op_id, &account_id).await {
                Ok(flush) => app.notify(
                    flush
                        .message()
                        .unwrap_or(tr("Đã hoàn tác: thư đã về thư mục cũ.")),
                    false,
                ),
                Err(problem) => app.notify(problem.message(), false),
            }
            refresh(app, real).await;
        });
        return;
    }
    let mut mail = app.mail;
    mail.write().undo();
}

/// Hide the "server refused" marker of a message.
pub fn dismiss_failed(app: AppState, message_id: &str) {
    if let Some(real) = real_of(app) {
        let mut ops = real.ops;
        ops.write().remove(message_id);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let id = message_id.to_string();
            spawn(async move {
                crate::adapters::actions::dismiss(&id).await;
                refresh_meta(real).await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailbox_contracts::fixtures;

    fn row(id: &str, folder: &str, account: &str, unread: bool) -> ThreadRow {
        let mut head = fixtures::bootstrap().messages.remove(0);
        head.id = id.into();
        head.thread_id = String::new();
        head.folder_id = folder.into();
        head.account_id = account.into();
        ThreadRow {
            head,
            count: 1,
            unread,
            starred: false,
            has_attachments: false,
        }
    }

    fn folder(key: &str, role: Option<&str>) -> RealFolder {
        RealFolder {
            key: key.into(),
            folder_id: format!("acc/{key}"),
            account_id: "acc".into(),
            label: key.into(),
            role: role.map(str::to_string),
        }
    }

    fn rows() -> Vec<ThreadRow> {
        vec![
            row("a", "inbox", "x", true),
            row("b", "inbox", "y", true),
            row("c", "sent", "x", false),
            row("d", "trash", "x", true),
            row("e", "x/Projects", "x", true),
            row("f", "junk", "x", true),
        ]
    }

    fn ids(rows: &[ThreadRow]) -> Vec<&str> {
        rows.iter().map(|r| r.head.id.as_str()).collect()
    }

    #[test]
    fn the_unified_inbox_is_the_default_and_shows_only_inbox_mail_of_all_accounts() {
        assert_eq!(RealView::default(), RealView::Inbox);
        assert_eq!(
            ids(&filter_rows(rows(), &RealView::Inbox, false)),
            vec!["a", "b"]
        );
    }

    #[test]
    fn a_role_or_folder_view_shows_exactly_that_folder() {
        assert_eq!(
            ids(&filter_rows(rows(), &RealView::Role("trash".into()), false)),
            vec!["d"]
        );
        assert_eq!(
            ids(&filter_rows(
                rows(),
                &RealView::Folder("x/Projects".into()),
                false
            )),
            vec!["e"]
        );
        // Starred spans folders, without trash and junk.
        assert_eq!(
            ids(&filter_rows(rows(), &RealView::Role("trash".into()), true)),
            vec!["a", "b", "c", "e"]
        );
    }

    #[test]
    fn unread_counts_are_per_folder_and_optionally_per_account() {
        let rows = rows();
        assert_eq!(unread_in(&rows, "inbox", None), 2);
        assert_eq!(unread_in(&rows, "inbox", Some("x")), 1);
        assert_eq!(unread_in(&rows, "sent", None), 0);
        assert_eq!(unread_in(&rows, "x/Projects", None), 1);
    }

    #[test]
    fn the_sidebar_offers_only_roles_some_account_really_has() {
        let folders = vec![
            folder("inbox", Some("inbox")),
            folder("trash", Some("trash")),
            folder("sent", Some("sent")),
            folder("acc/Projects", None),
        ];
        assert_eq!(nav_roles(&folders), vec!["sent", "trash"]);
        assert!(nav_roles(&[]).is_empty(), "no invented folders");
        assert_eq!(slug("acc-x/INBOX.Dự án"), "acc-x-inbox-d---n");
        for role in NAV_ROLES {
            assert_ne!(role_label(role), "Thư mục");
        }
    }
}
