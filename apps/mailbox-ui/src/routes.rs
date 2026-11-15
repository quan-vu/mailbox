//! URL routes (MB-104). Targets follow docs/03-ui/SCREENS.md. Only the screen is in the URL;
//! demo state such as filters and drafts stays in memory.
// rustc 1.100 reports `unreachable_code` inside the code that the `Routable` derive of
// Dioxus 0.7.10 generates for `Route`. An attribute on the enum does not reach that code, so
// the lint is off for this file; nothing written here is unreachable.
#![allow(unreachable_code)]
use crate::app::Body;
use crate::components::common::EmptyState;
use crate::i18n::tr;
use crate::screens::{
    compose::ComposeScreen, connect::ConnectScreen, inbox::InboxScreen, outbox::OutboxScreen,
    settings::SettingsScreen, thread::ThreadScreen,
};
use crate::state::{AppState, Screen, SettingsTab};
use dioxus::prelude::*;

#[derive(Clone, Debug, PartialEq, Routable)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Body)]
        #[redirect("/", || Route::Inbox {})]
        #[route("/inbox")]
        Inbox {},
        #[route("/messages/:id/thread")]
        Thread { id: String },
        #[route("/compose")]
        Compose {},
        #[route("/accounts/connect")]
        Connect {},
        #[route("/settings")]
        SettingsHome {},
        #[route("/settings/:tab")]
        Settings { tab: String },
        #[route("/:..segments")]
        NotFound { segments: Vec<String> },
}

impl Route {
    /// The screen a route shows, used for navigation highlighting. `None` for unknown paths.
    pub fn screen(&self) -> Option<Screen> {
        match self {
            Route::Inbox {} => Some(Screen::Inbox),
            Route::Thread { id } => Some(Screen::Thread(id.clone())),
            Route::Compose {} => Some(Screen::Compose),
            Route::Connect {} => Some(Screen::Connect),
            Route::SettingsHome {} => Some(Screen::Settings(SettingsTab::Accounts)),
            Route::Settings { tab } => SettingsTab::from_key(tab).map(Screen::Settings),
            Route::NotFound { .. } => None,
        }
    }
}

impl From<Screen> for Route {
    fn from(screen: Screen) -> Self {
        match screen {
            Screen::Inbox => Route::Inbox {},
            Screen::Thread(id) => Route::Thread { id },
            Screen::Compose => Route::Compose {},
            Screen::Connect => Route::Connect {},
            Screen::Settings(SettingsTab::Accounts) => Route::SettingsHome {},
            Screen::Settings(tab) => Route::Settings {
                tab: tab.key().to_string(),
            },
        }
    }
}

#[component]
fn Inbox() -> Element {
    let app = use_context::<AppState>();
    // Real mode: the Drafts entry shows the local drafts and outbox (MB-133), not fixture mail.
    let local_drafts =
        app.is_real() && app.filter.read().folder == crate::state::mail::Folder::Drafts;
    if local_drafts {
        rsx! { OutboxScreen {} }
    } else {
        rsx! { InboxScreen {} }
    }
}

/// `id` is a message id; the screen shows the whole conversation that message belongs to.
#[component]
fn Thread(id: String) -> Element {
    let app = use_context::<AppState>();
    let key = app
        .mail
        .read()
        .message(&id)
        .map(|m| m.thread_key().to_string());
    match key {
        Some(thread) => rsx! { ThreadScreen { key: "{thread}", thread: thread.clone() } },
        None => rsx! { Missing { what: tr("Không tìm thấy hội thoại này") } },
    }
}

#[component]
fn Compose() -> Element {
    rsx! { ComposeScreen {} }
}

#[component]
fn Connect() -> Element {
    rsx! { ConnectScreen {} }
}

#[component]
fn SettingsHome() -> Element {
    rsx! { SettingsScreen { tab: SettingsTab::Accounts } }
}

#[component]
fn Settings(tab: String) -> Element {
    match SettingsTab::from_key(&tab) {
        Some(tab) => rsx! { SettingsScreen { tab } },
        None => rsx! { Missing { what: tr("Không có mục cài đặt này") } },
    }
}

#[component]
fn NotFound(segments: Vec<String>) -> Element {
    let _ = segments;
    rsx! { Missing { what: tr("Không tìm thấy trang") } }
}

#[component]
fn Missing(what: &'static str) -> Element {
    let app = use_context::<AppState>();
    rsx! {
        section { class: "not-found", "data-testid": "not-found",
            EmptyState { icon: "search", title: what.to_string(), detail: tr("Đường dẫn có thể đã thay đổi hoặc thư không còn trong bản demo.").to_string(), testid: "not-found-message" }
            button { class: "btn btn-primary", r#type: "button", "data-testid": "not-found-home", onclick: move |_| app.go(Screen::Inbox), {tr("Về hộp thư")} }
        }
    }
}
