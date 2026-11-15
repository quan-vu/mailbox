//! Real-mode folder navigation, row change states and the "move to" menu (MB-122/143/132/144).
//! Rendered only in Real mode; Demo mode keeps its own fixed folder list.
use super::icon::Icon;
use crate::i18n::{bi, tr};
use crate::routes::Route;
use crate::state::folders::{
    self, RealFolder, RealState, RealView, RowOp, nav_roles, role_icon, role_label, slug, unread_in,
};
use crate::state::mail::{Filter, Folder};
use crate::state::{AppState, Screen};
use dioxus::prelude::*;
use mailbox_contracts::MessageSummary;

#[derive(Clone, PartialEq)]
struct Entry {
    testid: String,
    label: String,
    icon: &'static str,
    /// `MessageSummary::folder_id` of this entry's mail.
    key: String,
    view: RealView,
    starred: bool,
}

/// Sidebar entries for real accounts: unified inbox, starred, the role folders the server has,
/// then the user's own folders.
#[component]
pub fn RealFolderNav() -> Element {
    let app = use_context::<AppState>();
    let real = RealState::get();
    let mut filter = app.filter;
    let on_inbox = use_route::<Route>() == Route::Inbox {};
    // Folder list and row states come from the local store: reload them when mail changes.
    use_effect(move || {
        let _ = app.mail.read().accounts.len();
        if let Some(real) = real {
            spawn(folders::refresh_meta(real));
        }
    });
    let f = filter.read().clone();
    let (list, view): (Vec<RealFolder>, RealView) = match real {
        Some(r) => (r.folders.read().clone(), r.view.read().clone()),
        None => (Vec::new(), RealView::Inbox),
    };
    let rows = app.mail.read().visible(&Filter::default());
    let multi = app.mail.read().accounts.len() > 1;

    let mut entries = vec![
        Entry {
            testid: "nav-inbox".into(),
            label: Folder::Inbox.label().into(),
            icon: "inbox",
            key: "inbox".into(),
            view: RealView::Inbox,
            starred: false,
        },
        Entry {
            testid: "nav-folder-starred".into(),
            label: Folder::Starred.label().into(),
            icon: "star",
            key: String::new(),
            view: RealView::Inbox,
            starred: true,
        },
    ];
    for role in nav_roles(&list) {
        entries.push(Entry {
            testid: format!("nav-folder-{role}"),
            label: role_label(role).into(),
            icon: role_icon(role),
            key: role.into(),
            view: RealView::Role(role.into()),
            starred: false,
        });
    }
    for folder in list.iter().filter(|x| x.role.is_none()) {
        let account = app
            .mail
            .read()
            .account(&folder.account_id)
            .map(|a| a.name.clone());
        entries.push(Entry {
            testid: format!("nav-real-folder-{}", slug(&folder.folder_id)),
            label: match account {
                Some(name) if multi => format!("{} ({name})", folder.label),
                _ => folder.label.clone(),
            },
            icon: "tag",
            key: folder.key.clone(),
            view: RealView::Folder(folder.folder_id.clone()),
            starred: false,
        });
    }

    rsx! {
        for entry in entries {
            {
                let unread = if entry.starred { 0 } else { unread_in(&rows, &entry.key, None) };
                let in_folder_view = on_inbox && f.account.is_none();
                let active = if entry.starred {
                    in_folder_view && f.folder == Folder::Starred
                } else {
                    in_folder_view && f.folder == Folder::Inbox && view == entry.view
                };
                let target = entry.view.clone();
                let starred = entry.starred;
                rsx! {
                    button {
                        key: "{entry.testid}",
                        class: if active { "nav-item active" } else { "nav-item" },
                        r#type: "button",
                        "data-testid": "{entry.testid}",
                        "data-real-folder": "true",
                        "aria-label": if unread > 0 { bi(format!("{}, {} chưa đọc", entry.label, unread), format!("{}, {} unread", entry.label, unread)) } else { "{entry.label}" },
                        "aria-current": if active { "page" } else { "false" },
                        onclick: move |_| {
                            let mut w = filter.write();
                            w.folder = if starred { Folder::Starred } else { Folder::Inbox };
                            w.account = None;
                            drop(w);
                            if let Some(mut real) = real {
                                real.view.set(target.clone());
                            }
                            app.go(Screen::Inbox);
                        },
                        Icon { name: entry.icon }
                        span { class: "nav-label", "{entry.label}" }
                        if unread > 0 {
                            span { class: "count-pill", "aria-hidden": "true", "{unread}" }
                        }
                    }
                }
            }
        }
    }
}

/// Marker next to a row whose last change is still on its way to, or was refused by, the server.
#[component]
pub fn RowOpBadge(id: String) -> Element {
    let app = use_context::<AppState>();
    let state = RealState::get().and_then(|r| r.ops.read().get(&id).copied());
    let dismiss_id = id.clone();
    rsx! {
        match state {
            Some(RowOp::Pending) => rsx! {
                span { class: "row-op row-op-pending", role: "status", "data-testid": "row-op-pending-{id}",
                    Icon { name: "clock", size: 13 } {tr("Đang gửi lên máy chủ")}
                }
            },
            Some(RowOp::Failed) => rsx! {
                span { class: "row-op row-op-failed", role: "alert", "data-testid": "row-op-failed-{id}",
                    Icon { name: "alert", size: 13 } {tr("Máy chủ từ chối, đã hoàn tác")}
                    button { class: "row-op-dismiss", r#type: "button", "data-testid": "row-op-dismiss-{id}",
                        "aria-label": tr("Ẩn thông báo lỗi của thư này"),
                        onclick: move |_| folders::dismiss_failed(app, &dismiss_id),
                        Icon { name: "x", size: 13 }
                    }
                }
            },
            None => rsx! {},
        }
    }
}

/// "Chuyển đến…": the other real folders of the message's account. Real mode only.
#[component]
pub fn MoveMenu(message: MessageSummary) -> Element {
    let app = use_context::<AppState>();
    let targets: Vec<RealFolder> = match RealState::get() {
        Some(real) if app.is_real() => real
            .folders
            .read()
            .iter()
            .filter(|f| f.account_id == message.account_id && f.key != message.folder_id)
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    if targets.is_empty() {
        return rsx! {};
    }
    let labels: Vec<(String, String)> = targets
        .iter()
        .map(|f| {
            let label = f
                .role
                .as_deref()
                .map_or_else(|| f.label.clone(), |r| role_label(r).to_string());
            (f.folder_id.clone(), label)
        })
        .collect();
    let lookup = labels.clone();
    let id = message.id.clone();
    rsx! {
        label { class: "move-menu",
            Icon { name: "forward" }
            span { class: "sr-only", {tr("Chuyển thư đến thư mục")} }
            select { "data-testid": "reader-move", "aria-label": tr("Chuyển thư đến thư mục"), value: "",
                onchange: move |e| {
                    let folder_id = e.value();
                    if let Some((_, label)) = lookup.iter().find(|(fid, _)| *fid == folder_id) {
                        folders::move_to(app, &id, &folder_id, label);
                    }
                },
                option { value: "", selected: true, disabled: true, {tr("Chuyển đến…")} }
                for (folder_id, label) in labels {
                    option { key: "{folder_id}", value: "{folder_id}", "{label}" }
                }
            }
        }
    }
}
