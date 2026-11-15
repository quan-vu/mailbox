//! Live sync status of real accounts (MB-141). The background workers (`adapters::worker`,
//! Desktop only) publish one [`AccountLive`] per account; components read [`LIVE`].
//!
//! Demo mode never shows these states: the components check the app mode first, and the map
//! stays empty unless real accounts have workers.
// Real mode has no live workers on the Web build.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]
use crate::i18n::{bi, tr};
use dioxus::prelude::*;
pub use mailbox_core::live::{AccountLive, Attention, LinkState, LiveMode};
use std::collections::BTreeMap;

/// Per-account live state, by account id.
pub static LIVE: GlobalSignal<BTreeMap<String, AccountLive>> = Signal::global(BTreeMap::new);

/// `HH:MM UTC` of a UNIX time. Time zones arrive with localisation (MB-105).
pub fn clock(epoch: i64) -> String {
    let secs = epoch.rem_euclid(86_400);
    format!("{:02}:{:02} UTC", secs / 3600, (secs % 3600) / 60)
}

/// Visual tone of a state, used as a CSS modifier.
pub fn tone(state: LinkState) -> &'static str {
    match state {
        LinkState::Online(_) => "ok",
        LinkState::Connecting | LinkState::Syncing => "busy",
        LinkState::Offline { .. } => "warn",
        LinkState::NeedsAttention(_) => "danger",
    }
}

/// What the account row says. Never claims "online" before a sync has succeeded.
pub fn label(state: LinkState) -> String {
    match state {
        LinkState::Connecting => tr("Đang kết nối…").into(),
        LinkState::Syncing => tr("Đang đồng bộ…").into(),
        LinkState::Online(LiveMode::Idle) => tr("Trực tuyến · nhận thư ngay khi đến").into(),
        LinkState::Online(LiveMode::Polling) => tr("Trực tuyến · kiểm tra thư mỗi 2 phút").into(),
        LinkState::Offline { retry_at } => bi(
            format!("Mất kết nối · tự thử lại lúc {}", clock(retry_at)),
            format!("Offline · retrying at {}", clock(retry_at)),
        ),
        LinkState::NeedsAttention(Attention::AuthFailed) => {
            tr("Máy chủ từ chối mật khẩu đã lưu").into()
        }
        LinkState::NeedsAttention(Attention::Certificate) => {
            tr("Chứng chỉ máy chủ không hợp lệ · MailBox không kết nối").into()
        }
        LinkState::NeedsAttention(Attention::Storage) => {
            tr("Không dùng được dữ liệu trên máy (CSDL hoặc keychain)").into()
        }
    }
}

/// How bad a state is, for the one-line summary in the top bar.
fn severity(state: LinkState) -> u8 {
    match state {
        LinkState::NeedsAttention(_) => 4,
        LinkState::Offline { .. } => 3,
        LinkState::Connecting => 2,
        LinkState::Syncing => 1,
        LinkState::Online(_) => 0,
    }
}

/// The worst state among accounts, or `None` when no account has a worker.
pub fn summary(live: &BTreeMap<String, AccountLive>) -> Option<LinkState> {
    live.values().map(|a| a.state).max_by_key(|s| severity(*s))
}

/// Short text for the top bar next to "Chế độ thật".
pub fn summary_label(state: LinkState, accounts: usize) -> String {
    let some = if accounts > 1 {
        tr("Một tài khoản")
    } else {
        tr("Tài khoản")
    };
    match state {
        LinkState::Online(_) => tr("Đã đồng bộ").into(),
        LinkState::Syncing => tr("Đang đồng bộ…").into(),
        LinkState::Connecting => tr("Đang kết nối…").into(),
        LinkState::Offline { .. } => bi(format!("{some} mất kết nối"), format!("{some} offline")),
        LinkState::NeedsAttention(_) => bi(
            format!("{some} cần xử lý"),
            format!("{some} needs attention"),
        ),
    }
}

/// "Retry now" for one account.
pub fn retry(account_id: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    crate::adapters::worker::retry_now(account_id);
    #[cfg(target_arch = "wasm32")]
    let _ = account_id;
}

/// Keep [`LIVE`] in step with the workers and reload the mail list whenever a worker changed
/// the local cache. Runs for the lifetime of the app (Desktop).
#[cfg(not(target_arch = "wasm32"))]
pub async fn pump(app: super::AppState) {
    use crate::adapters::worker;
    let mut seen: BTreeMap<String, u64> = BTreeMap::new();
    let mut known: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
    loop {
        let snapshot = worker::snapshot();
        // Accounts whose cache a worker changed, plus accounts seen for the first time (those
        // only get a baseline for new-mail notices).
        let touched: Vec<String> = snapshot
            .iter()
            .filter(|(id, a)| seen.get(*id) != Some(&a.generation))
            .map(|(id, _)| id.clone())
            .collect();
        let changed = snapshot
            .iter()
            .any(|(id, a)| seen.get(id).is_some_and(|g| *g != a.generation));
        if !touched.is_empty() && app.is_real() {
            worker::after_change(&touched, &mut known).await;
        }
        seen = snapshot
            .iter()
            .map(|(id, a)| (id.clone(), a.generation))
            .collect();
        if *LIVE.peek() != snapshot {
            *LIVE.write() = snapshot;
        }
        if changed
            && app.is_real()
            && let Some(real) = super::folders::RealState::get()
        {
            super::folders::refresh(app, real).await;
        }
        worker::changed().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_never_claim_success_early_and_summary_shows_the_worst_account() {
        assert!(!label(LinkState::Connecting).contains("Trực tuyến"));
        assert!(!label(LinkState::Syncing).contains("Trực tuyến"));
        assert_eq!(
            label(LinkState::Offline { retry_at: 3_660 }),
            "Mất kết nối · tự thử lại lúc 01:01 UTC"
        );
        let mut live = BTreeMap::new();
        assert_eq!(summary(&live), None);
        let at = |state| AccountLive {
            state,
            ..AccountLive::default()
        };
        live.insert("a".to_string(), at(LinkState::Online(LiveMode::Idle)));
        live.insert("b".to_string(), at(LinkState::Offline { retry_at: 0 }));
        assert_eq!(summary(&live), Some(LinkState::Offline { retry_at: 0 }));
        assert_eq!(
            summary_label(LinkState::Offline { retry_at: 0 }, 2),
            "Một tài khoản mất kết nối"
        );
        live.insert(
            "c".to_string(),
            at(LinkState::NeedsAttention(Attention::AuthFailed)),
        );
        assert_eq!(
            summary(&live).map(tone),
            Some("danger"),
            "auth failure outranks offline"
        );
    }
}
