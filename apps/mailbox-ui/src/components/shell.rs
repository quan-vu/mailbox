use super::common::DemoBadge;
use super::icon::{Icon, pick};
use crate::adapters::connect::REAL_CHECK;
use crate::i18n::{bi, tr};
use crate::routes::Route;
use crate::state::AppMode;
use crate::state::mail::{Filter, Folder};
use crate::state::{AppState, Screen, SettingsTab, Theme};
use dioxus::prelude::*;

pub fn account_icon(id: &str) -> &'static str {
    match id {
        "personal" => "user",
        "support" => "headset",
        _ => "briefcase",
    }
}

#[component]
pub fn Topbar() -> Element {
    let app = use_context::<AppState>();
    let mut filter = app.filter;
    let mut theme = app.theme;
    let mut menu = app.menu_open;
    let query = filter.read().query.clone();
    let dark = app.is_dark();
    let real = app.is_real();
    let on_inbox = use_route::<Route>() == Route::Inbox {};
    rsx! {
        header { class: "topbar",
            button { class: "icon-btn mobile-only", r#type: "button", "aria-label": tr("Mở menu thư mục và tài khoản"), "data-testid": "mobile-menu-button",
                "aria-expanded": if menu() { "true" } else { "false" },
                onclick: move |_| menu.set(true),
                Icon { name: "menu", size: 22 }
            }
            button { class: "brand", r#type: "button", "aria-label": tr("MailBox — về Hộp thư hợp nhất"),
                onclick: move |_| { filter.set(Filter::default()); app.go(Screen::Inbox); },
                span { class: "logo", Icon { name: "mail", size: 22 } }
                span { class: "brand-name", "MailBox" }
            }
            label { class: "global-search desktop-only",
                Icon { name: "search", size: 18 }
                input {
                    id: "global-search",
                    "data-testid": "global-search-input",
                    r#type: "search",
                    "aria-label": tr("Tìm kiếm trong tất cả hộp thư"),
                    placeholder: tr("Tìm kiếm trong tất cả hộp thư (Ctrl + K)"),
                    value: "{query}",
                    oninput: move |e| {
                        filter.write().query = e.value();
                        if !on_inbox { app.go(Screen::Inbox); }
                    },
                }
            }
            div { class: "topbar-actions",
                div { class: "sync-summary",
                    if real {
                        span { class: "badge badge-real", "data-testid": "topbar-real-status", "data-status": "real",
                            "data-live": crate::state::live::summary(&crate::state::live::LIVE.read()).map_or("none", |s| s.key()),
                            Icon { name: "server", size: 14 } {tr("Chế độ thật")}
                        }
                        super::live::TopbarLive {}
                    } else {
                        DemoBadge { label: tr("Chế độ demo"), testid: "topbar-demo-status" }
                        span { class: "muted small desktop-only", {tr("Dữ liệu minh họa")} }
                    }
                    // Real mode needs a native connection check, so the switch exists on Desktop only.
                    if REAL_CHECK {
                        button { class: "btn btn-sm", r#type: "button", "data-testid": "topbar-mode-toggle",
                            "aria-pressed": if real { "true" } else { "false" },
                            title: tr("Chuyển giữa dữ liệu minh họa và chế độ thật"),
                            onclick: move |_| { spawn(app.set_mode(if real { AppMode::Demo } else { AppMode::Real })); },
                            if real { {tr("Về demo")} } else { {tr("Dùng thật")} }
                        }
                    }
                }
                button { class: "icon-btn desktop-only", r#type: "button", "aria-label": tr("Thông báo (chưa có thông báo thật)"), Icon { name: "bell", size: 20 } }
                button { class: "icon-btn", r#type: "button", "data-testid": "topbar-theme-toggle",
                    "aria-label": if dark { tr("Chuyển sang giao diện sáng")  } else { tr("Chuyển sang giao diện tối")  },
                    onclick: move |_| theme.set(if dark { Theme::Light } else { Theme::Dark }),
                    Icon { name: pick(dark, "sun", "moon"), size: 20 }
                }
                button { class: "icon-btn", r#type: "button", "aria-label": tr("Cài đặt"), "data-testid": "topbar-settings",
                    onclick: move |_| app.go(Screen::Settings(SettingsTab::Accounts)),
                    Icon { name: "settings", size: 20 }
                }
                span { class: "avatar-chip desktop-only", title: tr("Tài khoản demo — chưa có đăng nhập thật"), "MV" }
            }
        }
    }
}

#[component]
pub fn Sidebar() -> Element {
    let app = use_context::<AppState>();
    let mut filter = app.filter;
    let mut menu = app.menu_open;
    let mail = app.mail.read();
    let f = filter.read().clone();
    let on_inbox = use_route::<Route>() == Route::Inbox {};
    let real = app.is_real();
    let detached = app.detached.read().clone();
    let accounts: Vec<_> = mail
        .accounts
        .iter()
        .filter(|a| !detached.contains(&a.id))
        .cloned()
        .collect();
    let unread_all = mail.unread_count(None);
    // Real mode keeps mail of every folder: an account's badge counts its inbox only.
    let real_rows = if real {
        mail.visible(&Filter::default())
    } else {
        Vec::new()
    };
    let counts: Vec<usize> = accounts
        .iter()
        .map(|a| {
            if real {
                crate::state::folders::unread_in(&real_rows, "inbox", Some(&a.id))
            } else {
                mail.unread_count(Some(&a.id))
            }
        })
        .collect();
    rsx! {
        if menu() {
            div { class: "drawer-scrim mobile-only", onclick: move |_| menu.set(false) }
        }
        aside { class: if menu() { "sidebar open" } else { "sidebar" }, "aria-label": tr("Điều hướng chính"), "data-testid": "sidebar",
            button { class: "compose-cta", r#type: "button", "data-testid": "nav-compose", onclick: move |_| app.start_compose(None),
                Icon { name: "pen", size: 20 } {tr("Soạn thư")}
            }
            nav { class: "menu", "aria-label": tr("Thư mục"),
                if real { super::folders::RealFolderNav {} }
                for folder in Folder::ALL.into_iter().filter(|_| !real) {
                    {
                        let active = on_inbox && f.folder == folder && f.account.is_none();
                        let testid = if folder == Folder::Inbox { "nav-inbox".to_string() } else { format!("nav-folder-{}", folder.key()) };
                        rsx! {
                            button {
                                key: "{folder.key()}",
                                class: if active { "nav-item active" } else { "nav-item" },
                                r#type: "button",
                                "data-testid": "{testid}",
                                // Labels are hidden in the icon rail, so the name must not depend on visible text.
                                "aria-label": if folder == Folder::Inbox && unread_all > 0 { bi(format!("{}, {} chưa đọc", folder.label(), unread_all), format!("{}, {} unread", folder.label(), unread_all)) } else { "{folder.label()}" },
                                "aria-current": if active { "page" } else { "false" },
                                onclick: move |_| {
                                    let mut w = filter.write();
                                    w.folder = folder;
                                    w.account = None;
                                    drop(w);
                                    app.go(Screen::Inbox);
                                },
                                Icon { name: folder.icon() }
                                span { class: "nav-label", "{folder.label()}" }
                                if folder == Folder::Inbox && unread_all > 0 {
                                    span { class: "count-pill", "aria-hidden": "true", "{unread_all}" }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "sidebar-heading", {tr("Tài khoản email")} }
            div { class: "account-list",
                for (account, count) in accounts.into_iter().zip(counts) {
                    {
                        let id = account.id.clone();
                        let active = on_inbox && f.account.as_deref() == Some(id.as_str());
                        rsx! {
                            button {
                                key: "{id}",
                                class: if active { "account-row active" } else { "account-row" },
                                r#type: "button",
                                "data-testid": "nav-account-{account.id}",
                                "aria-label": bi(format!("Tài khoản {}, {}, {} chưa đọc", account.name, account.address, count), format!("Account {}, {}, {} unread", account.name, account.address, count)),
                                "aria-current": if active { "page" } else { "false" },
                                onclick: move |_| {
                                    let mut w = filter.write();
                                    w.folder = Folder::Inbox;
                                    w.account = Some(id.clone());
                                    drop(w);
                                    crate::state::folders::show_inbox();
                                    app.go(Screen::Inbox);
                                },
                                span { class: "dot", style: "background:{account.color}" }
                                Icon { name: account_icon(&account.id) }
                                span { class: "account-caption",
                                    strong { "{account.name}" }
                                    small { "{account.address}" }
                                }
                                if count > 0 { span { class: "count", "{count}" } }
                            }
                            if real { super::live::AccountLiveLine { key: "live-{account.id}", account_id: account.id.clone(), compact: true } }
                        }
                    }
                }
            }
            button { class: "add-account", r#type: "button", "data-testid": "nav-add-account", onclick: move |_| app.go(Screen::Connect),
                Icon { name: "plus" } {tr("Thêm tài khoản")}
            }
            div { class: "sidebar-heading", {tr("Trạng thái MailBox")} }
            if real {
                div { class: "mini-status", "data-testid": "sidebar-real-status",
                    Icon { name: "server", size: 22 }
                    span { strong { {tr("Chế độ thật")} } small { {tr("Đồng bộ thư mục, trạng thái đã đọc, gắn sao và di chuyển thư với máy chủ của tài khoản đã lưu.")} } }
                }
            } else {
                div { class: "mini-status",
                    DemoBadge { label: "Demo" }
                    span { strong { {tr("Chưa kết nối email thật")} } small { {tr("Đang hiển thị dữ liệu minh họa")} } }
                }
                div { class: "mini-status",
                    Icon { name: "database", size: 22 }
                    span { class: "grow",
                        strong { {tr("Dung lượng sử dụng")} }
                        small { {tr("12,4 / 25 GB (minh họa)")} }
                        span { class: "meter", "aria-hidden": "true", span { style: "width:49%" } }
                    }
                }
            }
        }
    }
}

#[component]
pub fn BottomNav() -> Element {
    let app = use_context::<AppState>();
    let mut filter = app.filter;
    let mut search = app.mobile_search;
    let screen = use_route::<Route>().screen();
    let folder = filter.read().folder;
    let tab = |active: bool| {
        if active {
            "bottom-tab on"
        } else {
            "bottom-tab"
        }
    };
    rsx! {
        nav { class: "bottom-nav mobile-only", "aria-label": tr("Điều hướng"), "data-testid": "bottom-nav",
            button { class: tab(screen == Some(Screen::Inbox) && folder == Folder::Inbox), r#type: "button", "data-testid": "bottom-nav-inbox",
                onclick: move |_| { filter.set(Filter::default()); app.go(Screen::Inbox); },
                Icon { name: "inbox", size: 20 } span { {tr("Hộp thư")} }
            }
            button { class: tab(search()), r#type: "button", "data-testid": "bottom-nav-search",
                onclick: move |_| { search.toggle(); app.go(Screen::Inbox); },
                Icon { name: "search", size: 20 } span { {tr("Tìm kiếm")} }
            }
            button { class: tab(screen == Some(Screen::Inbox) && folder == Folder::Starred), r#type: "button", "data-testid": "bottom-nav-starred",
                onclick: move |_| { filter.set(Filter { folder: Folder::Starred, ..Filter::default() }); app.go(Screen::Inbox); },
                Icon { name: "star", size: 20 } span { {tr("Gắn sao")} }
            }
            button { class: tab(matches!(screen, Some(Screen::Settings(_)))), r#type: "button", "data-testid": "bottom-nav-settings",
                onclick: move |_| app.go(Screen::Settings(SettingsTab::Accounts)),
                Icon { name: "settings", size: 20 } span { {tr("Cài đặt")} }
            }
        }
    }
}
