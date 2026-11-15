//! Live sync status (MB-141): top bar summary, per-account line in the sidebar, and the
//! settings "Đồng bộ" panel rows. Real mode only; Demo mode never renders these.
use super::icon::Icon;
use crate::i18n::{bi, tr};
use crate::state::live::{self, Attention, LIVE, LinkState};
use crate::state::{AppState, Screen};
use dioxus::prelude::*;

/// Worst account state next to "Chế độ thật". Empty until a worker reports.
#[component]
pub fn TopbarLive() -> Element {
    let map = LIVE.read();
    let Some(state) = live::summary(&map) else {
        return rsx! {};
    };
    let text = live::summary_label(state, map.len());
    rsx! {
        span { class: "live-chip live-{live::tone(state)}", "data-testid": "topbar-live-status", "data-live": state.key(),
            role: "status", "aria-live": "polite",
            span { class: "live-dot", "aria-hidden": "true" }
            "{text}"
        }
    }
}

/// The state of one account, its last sync, and what the user can do about a problem.
#[component]
pub fn AccountLiveLine(account_id: String, compact: bool) -> Element {
    let app = use_context::<AppState>();
    let map = LIVE.read();
    let Some(info) = map.get(&account_id).cloned() else {
        return rsx! {};
    };
    let state = info.state;
    let text = live::label(state);
    let synced = info.last_synced.map(|t| {
        bi(
            format!("Đồng bộ lần cuối {}", live::clock(t)),
            format!("Last synced {}", live::clock(t)),
        )
    });
    let partial = (info.failed_folders > 0).then(|| {
        bi(
            format!("{} thư mục chưa đồng bộ được", info.failed_folders),
            format!(
                "{} folder{} could not be synced",
                info.failed_folders,
                crate::i18n::s(info.failed_folders)
            ),
        )
    });
    let reauth = state == LinkState::NeedsAttention(Attention::AuthFailed);
    let retry_id = account_id.clone();
    rsx! {
        div { class: if compact { "account-live compact live-{live::tone(state)}" } else { "account-live live-{live::tone(state)}" },
            "data-testid": "account-live-{account_id}", "data-live": state.key(),
            span { class: "live-dot", "aria-hidden": "true" }
            span { class: "account-live-text",
                span { "{text}" }
                if let Some(s) = synced { small { "data-testid": "account-synced-{account_id}", "{s}" } }
                if let Some(p) = partial { small { class: "live-partial", "data-testid": "account-partial-{account_id}", "{p}" } }
            }
            if reauth {
                button { class: "btn btn-sm live-action", r#type: "button", "data-testid": "account-reauth-{account_id}",
                    title: tr("Mở biểu mẫu kết nối để nhập lại mật khẩu (MailBox không hiển thị mật khẩu cũ)"),
                    onclick: move |_| app.go(Screen::Connect),
                    Icon { name: "lock", size: 14 } {tr("Nhập lại mật khẩu")}
                }
            }
            if state.can_retry() {
                button { class: "btn btn-sm live-action", r#type: "button", "data-testid": "account-retry-{account_id}",
                    onclick: move |_| live::retry(&retry_id),
                    Icon { name: "refresh", size: 14 } {tr("Thử lại ngay")}
                }
            }
        }
    }
}

/// Settings → Đồng bộ in Real mode: one row per saved account with its real state.
#[component]
pub fn LiveStatusList() -> Element {
    let app = use_context::<AppState>();
    let accounts: Vec<(String, String)> = app
        .mail
        .read()
        .accounts
        .iter()
        .map(|a| (a.id.clone(), a.address.clone()))
        .collect();
    let started = !LIVE.read().is_empty();
    rsx! {
        div { class: "live-list", "data-testid": "settings-live-status",
            if accounts.is_empty() {
                p { class: "muted", {tr("Chưa có tài khoản thật nào trên máy này.")} }
            } else if !started {
                p { class: "muted", {tr("Đồng bộ nền chưa chạy cho tài khoản nào.")} }
            }
            for (id, address) in accounts.into_iter().filter(|_| started) {
                div { key: "{id}", class: "setting-row live-row",
                    span { class: "grow", strong { "{address}" } AccountLiveLine { account_id: id.clone(), compact: false } }
                }
            }
            p { class: "muted small",
                {tr("IDLE khi máy chủ hỗ trợ (gia hạn mỗi 25 phút), nếu không thì kiểm tra hộp thư đến mỗi 2 phút; các thư mục khác mỗi 10 phút. Mất kết nối thì thử lại với thời gian chờ tăng dần (tối đa 5 phút).")}
            }
        }
    }
}
