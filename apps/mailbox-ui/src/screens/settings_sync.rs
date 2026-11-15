//! Real-mode tr("Đồng bộ tự động") setting (MB-140 wiring, track G). Bound to the stored preference:
//! off stops every live sync worker at once, on starts them again; manual sync keeps working.
use crate::components::common::Switch;
use crate::components::icon::Icon;
use crate::i18n::tr;
use crate::state::AppState;
use crate::state::prefs::PREFS;
use dioxus::prelude::*;

#[component]
pub fn RealAutoSync() -> Element {
    let app = use_context::<AppState>();
    let on = PREFS.read().auto_sync;
    rsx! {
        div { class: "setting-row", "data-testid": "settings-auto-sync", "data-on": if on { "true" } else { "false" },
            span { class: "grow",
                strong { {tr("Đồng bộ tự động")} }
                small { class: "muted", {tr("Giữ kết nối tới máy chủ khi MailBox đang mở: nhận thư ngay nếu máy chủ hỗ trợ IDLE, nếu không thì kiểm tra mỗi 2 phút.")} }
            }
            Switch { testid: "settings-toggle-auto-sync".to_string(), label: tr("Đồng bộ tự động").to_string(), checked: on,
                onchange: move |v| app.edit_prefs(|p| p.auto_sync = v) }
        }
        if !on {
            div { class: "alert alert-warn", "data-testid": "settings-auto-sync-off",
                Icon { name: "info" }
                span { {tr("Đồng bộ tự động đang tắt: MailBox không giữ kết nối và không tự kiểm tra thư mới. Bấm Đồng bộ để lấy thư khi cần. Bấm Lưu thay đổi để giữ lựa chọn này sau khi mở lại.")} }
            }
        }
    }
}
