//! Autoconfiguration result (MB-202): what was found, where each value came from, and an
//! explicit "use these" button. Nothing is applied or saved until the user presses it.
use crate::components::icon::Icon;
use crate::i18n::{bi, tr};
use dioxus::prelude::*;
use mailbox_autoconfig::{Detection, Found};

fn line(label: &'static str, testid: &'static str, found: &Option<Found>) -> Element {
    match found {
        Some(f) => rsx! {
            div { class: "autoconfig-row", "data-testid": "{testid}", "data-source": "{f.source.key()}",
                strong { "{label}" }
                span { class: "mono", "{f.endpoint.host}:{f.endpoint.port} · {f.endpoint.security.label()}" }
                small { class: "muted", {bi(format!("Nguồn: {}", f.source.label()), format!("Source: {}", f.source.label()))} }
            }
        },
        None => rsx! {
            div { class: "autoconfig-row", "data-testid": "{testid}", "data-source": "none",
                strong { "{label}" }
                span { class: "muted", {tr("Chưa phát hiện được. Nhập thủ công.")} }
            }
        },
    }
}

#[component]
pub fn AutoconfigResult(
    detection: Detection,
    domain: String,
    onapply: EventHandler<()>,
) -> Element {
    let found_any = detection.imap.is_some() || detection.smtp.is_some();
    rsx! {
        section { class: if found_any { "alert alert-info autoconfig" } else { "alert alert-warn autoconfig" }, role: "status", "aria-live": "polite", "data-testid": "autoconfig-result",
            Icon { name: "server" }
            div { class: "grow",
                if let Some(name) = detection.provider { p { strong { {bi(format!("Nhà cung cấp: {name}"), format!("Provider: {name}"))} } } }
                {line("IMAP", "autoconfig-imap", &detection.imap)}
                {line("SMTP", "autoconfig-smtp", &detection.smtp)}
                if let Some(hint) = detection.hint { p { class: "small", "data-testid": "autoconfig-hint", "{hint}" } }
                for note in detection.notes.iter() { p { class: "small muted", "{note}" } }
                p { class: "small muted", {bi(format!("Chỉ tên miền \"{domain}\" được dùng để dò; địa chỉ đầy đủ và mật khẩu không được gửi đi. Đây chỉ là đề xuất: chưa có gì được lưu và bạn vẫn sửa tay được."), format!("Only the domain \"{domain}\" is used for detection; the full address and password are not sent. This is only a suggestion: nothing is saved yet and you can still edit it by hand."))} }
                if found_any {
                    button { class: "btn btn-sm btn-primary", r#type: "button", "data-testid": "autoconfig-apply", onclick: move |_| onapply.call(()), Icon { name: "check", size: 14 } {tr("Dùng đề xuất này")} }
                }
            }
        }
    }
}
