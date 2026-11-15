//! Application shell: shared state context, layout and overlays.
use crate::components::common::{ConfirmHost, ToastHost};
use crate::components::icon::Icon;
use crate::components::shell::{BottomNav, Sidebar, Topbar};
use crate::i18n::tr;
use crate::routes::Route;
use crate::state::{AppState, LoadState};
use dioxus::prelude::*;

static TOKENS_CSS: Asset = asset!("/assets/tokens.css");
static APP_CSS: Asset = asset!("/assets/main.css");
static READER_CSS: Asset = asset!("/assets/reader.css");
static FOLDERS_CSS: Asset = asset!("/assets/folders.css");
static SEARCH_CSS: Asset = asset!("/assets/search.css");
static STATUS_CSS: Asset = asset!("/assets/status.css");
static SETTINGS_EXTRA_CSS: Asset = asset!("/assets/settings-extra.css");

#[component]
pub fn App() -> Element {
    let app = use_context_provider(AppState::init);
    use_context_provider(crate::state::folders::RealState::init);
    use_context_provider(crate::state::search::SearchState::init);
    // Remote sources (Web) load after mount; local ones were applied in `AppState::init`.
    use_future(move || async move {
        if *app.load.peek() == LoadState::Loading {
            app.reload().await;
        }
        // Desktop: saved real accounts take precedence over demo data.
        app.restore_real_if_any().await;
    });
    // Remembered interface language (Desktop); Vietnamese shows until it is read.
    #[cfg(not(target_arch = "wasm32"))]
    use_future(|| async {
        if let Ok(lang) = crate::adapters::prefs::load_language().await {
            crate::i18n::set_lang(lang);
        }
    });
    // Live sync status and in-place list refresh (MB-140/141, Desktop).
    #[cfg(not(target_arch = "wasm32"))]
    use_future(move || crate::state::live::pump(app));
    // Scheduled send: due messages go out while the app runs (MB-218, Desktop).
    #[cfg(not(target_arch = "wasm32"))]
    use_future(move || crate::state::schedule::run(app));
    // Clicks on links in mail arrive from the WebView's navigation handler (S-09, Desktop).
    #[cfg(not(target_arch = "wasm32"))]
    use_future(crate::adapters::link::pump);
    rsx! { Router::<Route> {} }
}

/// Shell layout around the routed screen (`Outlet`).
#[component]
pub fn Body() -> Element {
    let app = use_context::<AppState>();
    let dark = app.is_dark();
    let load = app.load.read().clone();
    // The floating compose button only belongs to the mail list; elsewhere it would cover form actions.
    let show_fab = use_route::<Route>() == Route::Inbox {};
    rsx! {
        document::Title { {tr("MailBox — Hộp thư hợp nhất (demo)")} }
        document::Stylesheet { href: TOKENS_CSS }
        document::Stylesheet { href: APP_CSS }
        document::Stylesheet { href: READER_CSS }
        document::Stylesheet { href: FOLDERS_CSS }
        document::Stylesheet { href: SEARCH_CSS }
        document::Stylesheet { href: STATUS_CSS }
        document::Stylesheet { href: SETTINGS_EXTRA_CSS }
        div {
            class: "app-shell",
            "data-testid": "app-shell",
            lang: crate::i18n::code(crate::i18n::lang()),
            "data-theme": if dark { "dark" } else { "light" },
            "data-density": if app.is_real() && crate::state::prefs::PREFS.read().density == mailbox_core::prefs::Density::Compact { "compact" } else { "comfortable" },
            "data-source": app.source_key(),
            "data-load": load.key(),
            onkeydown: move |e| {
                if e.key() == Key::Escape && app.escape() {
                    e.prevent_default();
                }
            },
            a { class: "skip-link", href: "#main-content", {tr("Bỏ qua đến nội dung chính")} }
            Topbar {}
            div { class: "app-frame",
                Sidebar {}
                main { class: "content", id: "main-content", tabindex: "-1",
                    match load {
                        LoadState::Loading => rsx! {
                            div { class: "data-loading", "data-testid": "data-loading", role: "status", "aria-live": "polite",
                                span { class: "sr-only", {tr("Đang tải thư…")} }
                                for i in 0..5 {
                                    div { key: "{i}", class: "skeleton-row", "aria-hidden": "true",
                                        span { class: "skeleton skeleton-avatar" }
                                        span { class: "skeleton-lines", span { class: "skeleton" } span { class: "skeleton" } span { class: "skeleton short" } }
                                    }
                                }
                            }
                        },
                        LoadState::Failed(error) => rsx! {
                            div { class: "data-failed",
                                div { class: "alert alert-danger", role: "alert", "data-testid": "data-error",
                                    Icon { name: "alert" }
                                    span { class: "grow", "{error.message()}" }
                                    button { class: "btn btn-sm", r#type: "button", "data-testid": "data-retry",
                                        onclick: move |_| { spawn(app.reload()); },
                                        Icon { name: "refresh", size: 14 } {tr("Thử lại")}
                                    }
                                }
                                Outlet::<Route> {}
                            }
                        },
                        LoadState::Ready => rsx! { Outlet::<Route> {} },
                    }
                }
            }
            if show_fab {
                button { class: "fab mobile-only", r#type: "button", "data-testid": "fab-compose", "aria-label": tr("Soạn thư"),
                    onclick: move |_| app.start_compose(None),
                    Icon { name: "pen", size: 22 }
                }
            }
            BottomNav {}
            ToastHost {}
            ConfirmHost {}
            crate::components::link_confirm::LinkConfirmHost {}
            crate::components::shortcuts::ShortcutHost {}
            crate::components::notify_view::NotifyViewHost {}
        }
    }
}
