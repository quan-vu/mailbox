//! Settings panels for Real mode (MB-210, MB-211): every control reads and writes the stored
//! preferences (`state::prefs`). Demo mode keeps the in-memory panels in `settings.rs`.
use super::settings::Panel;
use crate::components::common::Switch;
use crate::components::icon::Icon;
use crate::i18n::{bi, tr};
use crate::state::AppState;
use crate::state::prefs::{
    CAN_STORE, PREFS, PREFS_ERROR, USER_TEMPLATES, is_dirty, new_template_id,
};
use dioxus::prelude::*;
use mailbox_core::prefs::{Density, NotifyMode, Signature, Template, ThemeChoice};

/// Shown on every Real panel: where the values live, and what is not stored.
#[component]
fn StorageNote() -> Element {
    let error = PREFS_ERROR.read().as_ref().copied();
    rsx! {
        if !CAN_STORE {
            div { class: "alert alert-warn", "data-testid": "settings-no-store",
                Icon { name: "info" }
                span { {tr("Bản Web chưa lưu cài đặt. Thay đổi chỉ có hiệu lực trong phiên này.")} }
            }
        } else if let Some(e) = error {
            div { class: "alert alert-warn", role: "alert", "data-testid": "settings-load-error", Icon { name: "alert" } span { "{e}" } }
        }
    }
}

#[component]
pub fn SaveState() -> Element {
    let dirty = is_dirty();
    rsx! {
        span { class: "muted small grow", "data-testid": "settings-save-state", "data-dirty": if dirty { "true" } else { "false" },
            if !CAN_STORE { {tr("Không ghi ra đĩa trên bản Web.")} }
            else if dirty { {tr("Có thay đổi chưa lưu.")} }
            else { {tr("Cài đặt lưu trên máy này (không có mật khẩu hay khóa).")} }
        }
    }
}

#[component]
pub fn RealAppearance() -> Element {
    let app = use_context::<AppState>();
    let p = PREFS.read().clone();
    rsx! {
        Panel { key_name: "appearance", icon: "palette", title: tr("Giao diện"),
            StorageNote {}
            div { class: "setting-col",
                strong { {tr("Chủ đề")} }
                div { class: "segmented", role: "radiogroup", "aria-label": tr("Chủ đề"),
                    for (t, key, label) in [(ThemeChoice::Light, "light", tr("Sáng")), (ThemeChoice::Dark, "dark", tr("Tối")), (ThemeChoice::System, "system", tr("Theo hệ thống"))] {
                        button { key: "{key}", class: if p.theme == t { "seg on" } else { "seg" }, r#type: "button", role: "radio",
                            "aria-checked": if p.theme == t { "true" } else { "false" },
                            "data-testid": "settings-theme-{key}", onclick: move |_| app.edit_prefs(|p| p.theme = t), "{label}" }
                    }
                }
                small { class: "muted", {tr("\"Theo hệ thống\" dùng giao diện sáng cho tới khi có đồng bộ với hệ điều hành (MB-105).")} }
            }
            div { class: "setting-col",
                strong { {tr("Mật độ danh sách thư")} }
                div { class: "segmented", role: "radiogroup", "aria-label": tr("Mật độ"),
                    for (d, key, label) in [(Density::Comfortable, "comfortable", tr("Thoải mái")), (Density::Compact, "compact", tr("Gọn"))] {
                        button { key: "{key}", class: if p.density == d { "seg on" } else { "seg" }, r#type: "button", role: "radio",
                            "aria-checked": if p.density == d { "true" } else { "false" },
                            "data-testid": "settings-density-{key}", onclick: move |_| app.edit_prefs(|p| p.density = d), "{label}" }
                    }
                }
            }
            crate::components::lang_picker::LanguagePicker {}
            div { class: "setting-row",
                span { class: "grow", strong { {tr("Giảm chuyển động")} } small { class: "muted", {tr("Tắt hiệu ứng chuyển cảnh")} } }
                Switch { testid: "settings-toggle-reduce-motion".to_string(), label: tr("Giảm chuyển động").to_string(), checked: p.reduce_motion,
                    onchange: move |v| app.edit_prefs(|p| p.reduce_motion = v) }
            }
        }
    }
}

#[component]
pub fn RealSecurity() -> Element {
    rsx! {
        Panel { key_name: "security", icon: "shield", title: tr("Bảo mật & quyền riêng tư"),
            StorageNote {}
            div { class: "setting-row", "data-testid": "settings-remote-images",
                span { class: "grow", strong { {tr("Tải ảnh từ xa")} } small { class: "muted", {tr("Luôn chặn trong bản 1.0 để chống theo dõi. Bạn có thể tải ảnh cho từng thư khi đọc.")} } }
                Switch { testid: "settings-toggle-remote-images".to_string(), label: tr("Tải ảnh từ xa").to_string(), checked: false, disabled: true, onchange: move |_| {} }
            }
            p { class: "muted small", {tr("Mật khẩu nằm trong Keychain của hệ điều hành, không bao giờ trong cài đặt. HTML trong thư luôn được làm sạch và cách ly.")} }
        }
    }
}

#[component]
pub fn RealIdentity() -> Element {
    let app = use_context::<AppState>();
    let accounts = app.mail.read().accounts.clone();
    let current = PREFS
        .read()
        .default_identity
        .clone()
        .unwrap_or_else(|| accounts.first().map(|a| a.id.clone()).unwrap_or_default());
    rsx! {
        div { class: "card", "data-testid": "settings-identity",
            div { class: "card-head", Icon { name: "send" } h2 { {tr("Tài khoản gửi mặc định")} } }
            p { class: "muted small", {tr("Dùng khi soạn thư mới. Trả lời luôn dùng tài khoản đã nhận thư gốc.")} }
            select { class: "input", "data-testid": "settings-default-identity", "aria-label": tr("Tài khoản gửi mặc định"), value: "{current}",
                onchange: move |e| { let v = e.value(); app.edit_prefs(|p| p.default_identity = Some(v)); },
                for a in accounts.iter() { option { key: "{a.id}", value: "{a.id}", selected: a.id == current, "{a.address}" } }
            }
        }
    }
}

#[component]
pub fn RealSignature() -> Element {
    let app = use_context::<AppState>();
    let accounts = app.mail.read().accounts.clone();
    let p = PREFS.read().clone();
    rsx! {
        Panel { key_name: "signature", icon: "pen", title: tr("Chữ ký"),
            StorageNote {}
            if accounts.is_empty() {
                p { class: "muted", {tr("Thêm tài khoản để đặt chữ ký.")} }
            }
            for a in accounts.iter() {
                {
                    let id = a.id.clone();
                    let id2 = a.id.clone();
                    let sig = p.signatures.get(&a.id).cloned().unwrap_or_default();
                    rsx! {
                        div { key: "{a.id}", class: "sig-block", "data-testid": "settings-signature-block-{a.id}",
                            label { class: "field", span { {bi(format!("Chữ ký cho {} (văn bản thuần)", a.address), format!("Signature for {} (plain text)", a.address))} }
                                textarea { class: "input textarea", "data-testid": "settings-signature-{a.id}", value: "{sig.text}",
                                    oninput: move |e| {
                                        let text = e.value();
                                        let id = id.clone();
                                        app.edit_prefs(move |p| { p.signatures.entry(id).or_default().text = text; });
                                    } }
                            }
                            div { class: "setting-row",
                                span { class: "grow", strong { {tr("Thêm khi trả lời / chuyển tiếp")} } small { class: "muted", {tr("Thư mới luôn có chữ ký nếu đã nhập")} } }
                                Switch { testid: format!("settings-signature-reply-{}", a.id), label: tr("Thêm khi trả lời").to_string(), checked: sig.use_for_replies,
                                    onchange: move |v| {
                                        let id = id2.clone();
                                        app.edit_prefs(move |p| { p.signatures.entry(id).or_insert_with(Signature::default).use_for_replies = v; });
                                    } }
                            }
                        }
                    }
                }
            }
            p { class: "muted small", {tr("Chữ ký được chèn một lần dưới phần soạn thảo và trên phần trích dẫn; đổi tài khoản gửi sẽ thay chữ ký, không thêm trùng.")} }
        }
        TemplatesPanel {}
    }
}

#[component]
fn TemplatesPanel() -> Element {
    let app = use_context::<AppState>();
    let list = USER_TEMPLATES.read().clone();
    let mut editing = use_signal(|| None::<Template>);
    rsx! {
        Panel { key_name: "templates", icon: "file", title: tr("Mẫu thư"),
            p { class: "muted small", {tr("Dùng {{name}}, {{first_name}} và {{email}}: điền từ người nhận đầu tiên khi áp dụng mẫu, luôn là văn bản thuần.")} }
            if list.is_empty() && editing.read().is_none() {
                div { class: "empty-inline", "data-testid": "settings-templates-empty", strong { {tr("Chưa có mẫu nào")} } small { class: "muted", {tr("Tạo mẫu để chèn nhanh trong màn hình soạn thư.")} } }
            }
            for t in list.iter() {
                {
                    let t_edit = t.clone();
                    let id = t.id.clone();
                    rsx! {
                        div { key: "{t.id}", class: "setting-row", "data-testid": "settings-template-{t.id}",
                            span { class: "grow", strong { "{t.name}" } small { class: "muted", "{t.subject}" } }
                            button { class: "btn btn-sm", r#type: "button", "data-testid": "settings-template-edit-{t.id}", onclick: move |_| editing.set(Some(t_edit.clone())), Icon { name: "pen", size: 14 } {tr("Sửa")} }
                            button { class: "btn btn-sm danger-text", r#type: "button", "data-testid": "settings-template-delete-{t.id}",
                                onclick: move |_| { spawn(app.delete_template(id.clone())); }, Icon { name: "trash", size: 14 } {tr("Xóa")} }
                        }
                    }
                }
            }
            if let Some(t) = editing.read().clone() {
                div { class: "template-editor", "data-testid": "settings-template-editor",
                    label { class: "field", span { {tr("Tên mẫu")} }
                        input { class: "input", "data-testid": "settings-template-name", value: "{t.name}",
                            oninput: move |e| { if let Some(x) = editing.write().as_mut() { x.name = e.value(); } } } }
                    label { class: "field", span { {tr("Chủ đề")} }
                        input { class: "input", "data-testid": "settings-template-subject", value: "{t.subject}",
                            oninput: move |e| { if let Some(x) = editing.write().as_mut() { x.subject = e.value(); } } } }
                    label { class: "field", span { {tr("Nội dung")} }
                        textarea { class: "input textarea", "data-testid": "settings-template-body", value: "{t.body}",
                            oninput: move |e| { if let Some(x) = editing.write().as_mut() { x.body = e.value(); } } } }
                    div { class: "row-actions",
                        button { class: "btn", r#type: "button", onclick: move |_| editing.set(None), {tr("Hủy")} }
                        button { class: "btn btn-primary", r#type: "button", "data-testid": "settings-template-save",
                            onclick: move |_| {
                                let Some(t) = editing.read().clone() else { return };
                                spawn(async move { if app.save_template(t).await { editing.set(None); } });
                            }, Icon { name: "check", size: 14 } {tr("Lưu mẫu")} }
                    }
                }
            } else {
                button { class: "btn", r#type: "button", "data-testid": "settings-template-new",
                    onclick: move |_| {
                        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
                        editing.set(Some(Template { id: new_template_id(nanos), name: String::new(), subject: String::new(), body: String::new() }));
                    },
                    Icon { name: "plus", size: 14 } {tr("Thêm mẫu")}
                }
            }
        }
    }
}

#[component]
pub fn RealNotifications() -> Element {
    let app = use_context::<AppState>();
    let p = PREFS.read().clone();
    rsx! {
        Panel { key_name: "notifications", icon: "bell", title: tr("Thông báo"),
            StorageNote {}
            div { class: "setting-col",
                strong { {tr("Thông báo hệ điều hành cho thư mới")} }
                div { class: "segmented", role: "radiogroup", "aria-label": tr("Thông báo thư mới"),
                    for (m, key, label) in [(NotifyMode::Off, "off", tr("Tắt")), (NotifyMode::CountOnly, "count", tr("Chỉ số lượng")), (NotifyMode::SenderAndSubject, "detail", tr("Người gửi và tiêu đề"))] {
                        button { key: "{key}", class: if p.notify_mode == m { "seg on" } else { "seg" }, r#type: "button", role: "radio",
                            "aria-checked": if p.notify_mode == m { "true" } else { "false" },
                            "data-testid": "settings-notify-{key}", onclick: move |_| app.edit_prefs(|p| p.notify_mode = m), "{label}" }
                    }
                }
                small { class: "muted", {tr("Mặc định chỉ hiện \"Có N thư mới\". Nội dung thư không bao giờ hiện trong thông báo. Chọn \"Người gửi và tiêu đề\" để cho phép hiện hai thông tin đó trên màn hình khóa và trung tâm thông báo.")} }
            }
            div { class: "setting-row",
                span { class: "grow", strong { {tr("Âm thanh")} } small { class: "muted", {tr("Phát âm thanh khi có thư mới")} } }
                Switch { testid: "settings-toggle-sound".to_string(), label: tr("Âm thanh").to_string(), checked: p.notify_sound, onchange: move |v| app.edit_prefs(|p| p.notify_sound = v) }
            }
            div { class: "setting-row",
                span { class: "grow", strong { {tr("Báo lỗi đồng bộ")} } small { class: "muted", {tr("Khi một tài khoản bị gián đoạn")} } }
                Switch { testid: "settings-toggle-sync-errors".to_string(), label: tr("Báo lỗi đồng bộ").to_string(), checked: p.notify_sync_errors, onchange: move |v| app.edit_prefs(|p| p.notify_sync_errors = v) }
            }
        }
    }
}
