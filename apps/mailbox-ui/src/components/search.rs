//! Real-mode search bar, highlighted result rows and the "load older" footer (MB-200).
//! Highlights are rendered as text nodes inside `mark`; mail text is never inserted as markup.
use super::common::Avatar;
use super::icon::{Icon, pick};
use crate::i18n::{bi, tr, when};
use crate::state::AppState;
use crate::state::folders::RealState;
use crate::state::mail::ThreadRow;
use crate::state::search::{self, Older, SearchScope, SearchState, highlight};
use dioxus::prelude::*;

/// Text with the query's words wrapped in `mark` (escaped by the renderer like any text).
#[component]
pub fn Highlighted(text: String, query: String) -> Element {
    let parts = highlight(&text, &query);
    rsx! {
        for (i, (part, hit)) in parts.into_iter().enumerate() {
            if hit {
                mark { key: "{i}", class: "search-hit", "{part}" }
            } else {
                span { key: "{i}", "{part}" }
            }
        }
    }
}

/// Scope switch, result count and clear control, shown above the list while searching.
#[component]
pub fn SearchBar() -> Element {
    let app = use_context::<AppState>();
    let Some(state) = SearchState::get() else {
        return rsx! {};
    };
    let mut scope = state.scope;
    let current = *scope.read();
    let f = app.filter.read().clone();
    let view = RealState::get()
        .map(|r| r.view.read().clone())
        .unwrap_or_default();
    let has_account = search::scope_account(app, &f, &view).is_some();
    let busy = search::search_pending(&f);
    let result = state.result.read().clone();
    let (status, count_key) = match (&result, busy) {
        (_, true) | (None, false) => (tr("Đang tìm…").to_string(), "busy"),
        (Some(r), false) if r.failed => (
            tr("Không tìm được: lỗi bộ nhớ đệm cục bộ.").to_string(),
            "failed",
        ),
        (Some(r), false) if r.total as usize > r.messages.len() => (
            bi(
                format!(
                    "{} kết quả · hiển thị {} mới nhất",
                    r.total,
                    r.messages.len()
                ),
                format!(
                    "{} result{} · showing the {} newest",
                    r.total,
                    crate::i18n::s(r.total as usize),
                    r.messages.len()
                ),
            ),
            "many",
        ),
        (Some(r), false) => (
            bi(
                format!("{} kết quả", r.total),
                format!("{} result{}", r.total, crate::i18n::s(r.total as usize)),
            ),
            "done",
        ),
    };
    rsx! {
        div { class: "search-bar", role: "region", "aria-label": tr("Tìm kiếm"), "data-testid": "search-bar",
            div { class: "search-scope", role: "radiogroup", "aria-label": tr("Phạm vi tìm kiếm"),
                for s in SearchScope::ALL {
                    button {
                        key: "{s.key()}",
                        class: if s == current { "chip on" } else { "chip" },
                        r#type: "button",
                        role: "radio",
                        "data-testid": "search-scope-{s.key()}",
                        "aria-checked": if s == current { "true" } else { "false" },
                        disabled: s == SearchScope::Account && !has_account,
                        title: pick(s == SearchScope::Account && !has_account, tr("Chọn một tài khoản trong bộ lọc để tìm trong tài khoản đó"), ""),
                        onclick: move |_| scope.set(s),
                        "{s.label()}"
                    }
                }
            }
            span { class: "search-count muted small", role: "status", "aria-live": "polite",
                "data-testid": "search-count", "data-state": "{count_key}",
                "{status}"
            }
            button { class: "link-btn", r#type: "button", "data-testid": "search-clear",
                onclick: move |_| search::clear(app),
                Icon { name: "x", size: 14 } {tr("Xóa tìm kiếm")}
            }
        }
    }
}

/// One search hit: like a list row, with matches highlighted and the folder named.
#[component]
pub fn SearchRow(row: ThreadRow, current: bool, query: String) -> Element {
    let app = use_context::<AppState>();
    let head = row.head.clone();
    let id = head.id.clone();
    let was_unread = row.unread;
    let account = app.mail.read().account(&head.account_id).cloned();
    let (acc_name, acc_color) = account.map(|a| (a.name, a.color)).unwrap_or_default();
    let place = crate::state::folders::role_label(&head.folder_id);
    let class = match (current, row.unread) {
        (true, _) => "row selected",
        (false, true) => "row unread",
        _ => "row",
    };
    rsx! {
        div { class: "row-wrap",
            button {
                class: "{class}",
                r#type: "button",
                "data-testid": "search-result-{head.id}",
                "data-row": "true",
                "data-unread": if row.unread { "true" } else { "false" },
                "data-thread-count": "{row.count}",
                "aria-current": if current { "true" } else { "false" },
                onclick: move |_| { app.open_message(&id); crate::state::folders::opened(app, &id, was_unread); },
                span { class: if row.unread { "unread-dot" } else { "unread-dot hidden" }, "aria-hidden": "true" }
                Avatar { name: head.from_name.clone() }
                span { class: "row-main",
                    span { class: "row-top",
                        span { class: "sender", Highlighted { text: head.from_name.clone(), query: query.clone() } }
                        time { class: "muted small", title: "{when(&head.received_at_full)}", "{when(&head.received_at)}" }
                    }
                    span { class: "subject",
                        Highlighted { text: head.subject.clone(), query: query.clone() }
                        if row.count > 1 { span { class: "thread-count", "{row.count}" } }
                    }
                    span { class: "row-account", "data-testid": "message-row-account", "data-account": "{head.account_id}",
                        span { class: "dot", style: "background:{acc_color}" }
                        "{acc_name} · {place}"
                    }
                }
            }
        }
    }
}

/// End of a real folder list: load older mail (cache first, then server), or say the start of
/// the folder is reached.
#[component]
pub fn LoadOlder() -> Element {
    let app = use_context::<AppState>();
    let Some(state) = SearchState::get() else {
        return rsx! {};
    };
    let older = *state.older.read();
    rsx! {
        div { class: "list-footer", "data-testid": "list-footer", "data-older": match older {
                Older::Idle => "idle",
                Older::Loading => "loading",
                Older::AtStart => "at-start",
                Older::Offline => "offline",
            },
            match older {
                Older::AtStart => rsx! {
                    span { class: "muted small", "data-testid": "list-history-end", {tr("Đã hiển thị đến thư cũ nhất của thư mục.")} }
                },
                Older::Loading => rsx! {
                    span { class: "muted small", role: "status", {tr("Đang tải thư cũ hơn…")} }
                },
                Older::Offline => rsx! {
                    span { class: "muted small", {tr("Không kết nối được máy chủ; đang hiển thị thư đã lưu trên máy.")} }
                    button { class: "btn btn-sm", r#type: "button", "data-testid": "list-load-older",
                        onclick: move |_| search::load_older(app),
                        Icon { name: "refresh", size: 14 } {tr("Thử lại")}
                    }
                },
                Older::Idle => rsx! {
                    button { class: "btn btn-sm", r#type: "button", "data-testid": "list-load-older",
                        onclick: move |_| search::load_older(app),
                        Icon { name: "chevron-down", size: 14 } {tr("Tải thư cũ hơn")}
                    }
                },
            }
        }
    }
}
