//! Keyboard shortcut host, help overlay and settings block (MB-210). The page listener only
//! reports key presses; `state::shortcuts::resolve` decides, `dispatch` acts.
use crate::components::common::Switch;
use crate::components::icon::Icon;
use crate::i18n::tr;
use crate::routes::Route;
use crate::state::compose::{Draft, ReplyKind};
use crate::state::prefs::PREFS;
use crate::state::shortcuts::{Action, BINDINGS, KeyInput, resolve, swallowed_keys};
use crate::state::{AppState, Screen};
use dioxus::prelude::*;

pub static HELP_OPEN: GlobalSignal<bool> = Signal::global(|| false);

/// Whether shortcuts are on: the stored preference in Real mode, the in-memory toggle in Demo.
pub fn shortcuts_enabled(app: AppState) -> bool {
    if app.is_real() {
        PREFS.read().shortcuts_enabled
    } else {
        app.toggles.read().get("shortcuts").copied().unwrap_or(true)
    }
}

fn listener_script() -> String {
    let keys = swallowed_keys()
        .iter()
        .map(|k| format!("{k:?}"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"
if (window.__mbShortcutsHandler) document.removeEventListener('keydown', window.__mbShortcutsHandler, true);
const swallow = new Set([{keys}]);
window.__mbShortcutsHandler = (e) => {{
  if (e.isComposing) return;
  const t = e.target;
  const tag = t && t.tagName ? t.tagName : '';
  const role = t && t.getAttribute ? (t.getAttribute('role') || '') : '';
  const typing = !!t && (t.isContentEditable || tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || role === 'textbox' || role === 'combobox');
  const interactive = typing || tag === 'BUTTON' || tag === 'A' || tag === 'SUMMARY' || ['button','switch','radio','menuitem','tab','link'].includes(role);
  const mod = e.ctrlKey || e.metaKey;
  if (window.__mbShortcutsOn !== false) {{
    if (mod && (e.key.toLowerCase() === 'k' || e.key === 'Enter')) e.preventDefault();
    else if (!mod && !typing && !e.altKey && swallow.has(e.key)) e.preventDefault();
  }}
  dioxus.send((mod ? '1' : '0') + (e.altKey ? '1' : '0') + (typing ? '1' : '0') + (interactive ? '1' : '0') + e.key);
}};
document.addEventListener('keydown', window.__mbShortcutsHandler, true);
"#
    )
}

fn parse(msg: &str) -> Option<KeyInput> {
    let flags = msg.get(..4)?;
    let key = msg.get(4..)?;
    let f: Vec<bool> = flags.chars().map(|c| c == '1').collect();
    (!key.is_empty()).then(|| KeyInput {
        key: key.to_string(),
        modifier: f[0],
        alt: f[1],
        typing: f[2],
        interactive: f[3],
    })
}

#[component]
pub fn ShortcutHost() -> Element {
    let app = use_context::<AppState>();
    use_future(move || async move {
        let mut ev = document::eval(&listener_script());
        while let Ok(msg) = ev.recv::<String>().await {
            let Some(input) = parse(&msg) else { continue };
            if let Some(action) = resolve(&input, shortcuts_enabled(app)) {
                dispatch(app, action);
            }
        }
    });
    use_effect(move || {
        let on = shortcuts_enabled(app);
        let _ = document::eval(&format!("window.__mbShortcutsOn = {on};"));
    });
    rsx! { if *HELP_OPEN.read() { ShortcutHelp {} } }
}

/// The message a shortcut acts on: the open conversation's last message, or the selection.
fn target(app: AppState, route: &Route) -> Option<mailbox_contracts::MessageSummary> {
    let mail = app.mail.read();
    match route {
        Route::Thread { id } => {
            let key = mail.message(id)?.thread_key().to_string();
            mail.thread(&key).last().cloned()
        }
        Route::Inbox {} => app
            .selected
            .read()
            .as_deref()
            .and_then(|id| mail.message(id).cloned()),
        _ => None,
    }
}

fn dispatch(app: AppState, action: Action) {
    let route = router().current::<Route>();
    let on_thread = matches!(route, Route::Thread { .. });
    let on_inbox = matches!(route, Route::Inbox {});
    match action {
        Action::Help => {
            let open = *HELP_OPEN.peek();
            *HELP_OPEN.write() = !open;
        }
        Action::Close => {
            if *HELP_OPEN.peek() {
                *HELP_OPEN.write() = false;
            }
        }
        Action::Compose => app.start_compose(None),
        Action::Search => {
            let mut search = app.mobile_search;
            search.set(true);
            if !on_inbox {
                app.go(Screen::Inbox);
            }
            let _ = document::eval("document.getElementById('global-search')?.focus();");
        }
        Action::Send if matches!(route, Route::Compose {}) => {
            let _ = document::eval(
                "document.querySelector('[data-testid=\"compose-send\"]')?.click();",
            );
        }
        Action::Reply | Action::ReplyAll | Action::Forward => {
            if let Some(m) = target(app, &route) {
                let kind = match action {
                    Action::Reply => ReplyKind::Reply,
                    Action::ReplyAll => ReplyKind::ReplyAll,
                    _ => ReplyKind::Forward,
                };
                app.start_compose(Some(Draft::reply_to(&m, kind)));
            }
        }
        Action::Archive | Action::Delete | Action::Star | Action::MarkUnread => {
            let Some(m) = target(app, &route) else { return };
            let key = m.thread_key().to_string();
            match action {
                Action::Archive => {
                    crate::state::folders::archive(app, &key, on_inbox);
                    if on_thread {
                        app.go(Screen::Inbox);
                    }
                }
                Action::Delete => {
                    crate::state::folders::trash(app, &key, on_inbox);
                    if on_thread {
                        app.go(Screen::Inbox);
                    }
                }
                Action::Star => crate::state::folders::toggle_star(app, &m.id),
                _ => crate::state::folders::set_unread(app, &key, true),
            }
        }
        Action::Next | Action::Previous if on_inbox => step_selection(app, action == Action::Next),
        Action::Open => {
            if on_inbox && let Some(id) = app.selected.read().clone() {
                app.go(Screen::Thread(id));
            }
        }
        _ => {}
    }
}

fn step_selection(app: AppState, forward: bool) {
    let f = app.filter.read().clone();
    let mail = app.mail.read();
    let rows = crate::state::folders::rows_in_view(app, f.folder, mail.visible(&f));
    let current = app.selected.read().clone().and_then(|id| {
        rows.iter()
            .position(|r| mail.thread(r.head.thread_key()).iter().any(|m| m.id == id))
    });
    let next = match (current, forward) {
        (None, _) => 0,
        (Some(i), true) => (i + 1).min(rows.len().saturating_sub(1)),
        (Some(i), false) => i.saturating_sub(1),
    };
    let Some(row) = rows.get(next) else { return };
    let (id, unread) = (row.head.id.clone(), row.unread);
    drop(mail);
    app.open_message(&id);
    crate::state::folders::opened(app, &id, unread);
}

#[component]
pub fn ShortcutHelp() -> Element {
    let mut groups: Vec<&'static str> = Vec::new();
    for b in BINDINGS {
        if !groups.contains(&b.group) {
            groups.push(b.group);
        }
    }
    rsx! {
        div { class: "shortcut-help-backdrop", "data-testid": "shortcut-help-backdrop", onclick: move |_| *HELP_OPEN.write() = false,
            div { class: "shortcut-help card", role: "dialog", "aria-modal": "true", "aria-label": tr("Phím tắt"), "data-testid": "shortcut-help",
                onclick: move |e| e.stop_propagation(),
                div { class: "card-head",
                    Icon { name: "sliders" }
                    h2 { {tr("Phím tắt")} }
                    button { class: "icon-btn push", r#type: "button", "aria-label": tr("Đóng bảng phím tắt"), "data-testid": "shortcut-help-close", onclick: move |_| *HELP_OPEN.write() = false, Icon { name: "x", size: 18 } }
                }
                for g in groups {
                    section { key: "{g}",
                        h3 { {tr(g)} }
                        for b in BINDINGS.iter().filter(|b| b.group == g) {
                            div { key: "{b.label}", class: "setting-row", "data-testid": "shortcut-row",
                                span { class: "grow", {tr(b.label)} }
                                kbd { {tr(b.shown)} }
                            }
                        }
                    }
                }
                p { class: "muted small", {tr("Phím tắt không hoạt động khi bạn đang gõ trong ô nhập liệu. Có thể tắt trong Cài đặt › Nâng cao.")} }
            }
        }
    }
}

/// Switch plus the list of bindings, used on the Advanced settings tab (both modes).
#[component]
pub fn ShortcutSettings() -> Element {
    let app = use_context::<AppState>();
    let on = shortcuts_enabled(app);
    let mut toggles = app.toggles;
    rsx! {
        div { class: "setting-row", "data-testid": "settings-shortcuts",
            span { class: "grow", strong { {tr("Phím tắt")} } small { class: "muted", {tr("Bật hoặc tắt toàn bộ phím tắt (Esc luôn đóng hộp thoại). Nhấn ? để xem bảng.")} } }
            Switch { testid: "settings-toggle-shortcuts".to_string(), label: tr("Phím tắt").to_string(), checked: on,
                onchange: move |v| {
                    if app.is_real() { app.edit_prefs(|p| p.shortcuts_enabled = v); } else { toggles.write().insert("shortcuts", v); }
                } }
        }
        for b in BINDINGS {
            div { key: "{b.label}", class: "setting-row", span { class: "grow", {tr(b.label)} } kbd { {tr(b.shown)} } }
        }
        button { class: "btn btn-sm", r#type: "button", "data-testid": "settings-shortcut-help", onclick: move |_| *HELP_OPEN.write() = true, {tr("Mở bảng phím tắt")} }
    }
}
