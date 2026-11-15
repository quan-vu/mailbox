use crate::components::common::{DemoBadge, Switch};
use crate::components::icon::Icon;
use crate::components::shell::account_icon;
use crate::i18n::{bi, tr};
use crate::screens::settings_real::{
    RealAppearance, RealIdentity, RealNotifications, RealSecurity, RealSignature, SaveState,
};
use crate::state::{
    AppState, Confirm, ConfirmAction, Screen, SettingsTab, Theme, default_toggles, demo,
};
use dioxus::prelude::*;

#[component]
pub fn SettingsScreen(tab: SettingsTab) -> Element {
    let app = use_context::<AppState>();
    let mut toggles = app.toggles;
    let mut theme = app.theme;
    let real = app.is_real();
    rsx! {
        div { class: "settings-layout", "data-testid": "screen-settings",
            nav { class: "settings-nav", "aria-label": tr("Mục cài đặt"),
                div { class: "settings-nav-head desktop-only",
                    button { class: "icon-btn", r#type: "button", "aria-label": tr("Quay lại hộp thư"), onclick: move |_| app.go(Screen::Inbox), Icon { name: "arrow-left", size: 20 } }
                    h2 { {tr("Cài đặt")} }
                }
                for t in SettingsTab::ALL {
                    {
                        let (title, helper, icon) = t.meta();
                        rsx! {
                            button { key: "{t.key()}", class: if t == tab { "settings-tab active" } else { "settings-tab" }, r#type: "button",
                                "data-testid": "settings-tab-{t.key()}",
                                "aria-current": if t == tab { "page" } else { "false" },
                                onclick: move |_| app.go(Screen::Settings(t)),
                                Icon { name: icon }
                                span { strong { "{title}" } small { class: "muted desktop-only", "{helper}" } }
                            }
                        }
                    }
                }
            }
            main { class: "settings-main",
                div { class: "settings-title",
                    h1 { {tr("Quản lý tài khoản và cài đặt MailBox")} }
                    p { class: "muted", {tr("Tài khoản email, đồng bộ, chữ ký, thông báo và bảo mật. Thay đổi chỉ lưu trong phiên demo.")} }
                }
                match tab {
                    SettingsTab::Accounts => rsx! { AccountsPanel {} },
                    SettingsTab::Sync => rsx! { SyncPanel {} },
                    SettingsTab::Appearance if real => rsx! { RealAppearance {} },
                    SettingsTab::Appearance => rsx! { AppearancePanel {} },
                    SettingsTab::Security if real => rsx! { RealSecurity {} },
                    SettingsTab::Security => rsx! { SecurityPanel {} },
                    SettingsTab::Signature if real => rsx! { RealSignature {} },
                    SettingsTab::Signature => rsx! { SignaturePanel {} },
                    SettingsTab::Notifications if real => rsx! { RealNotifications {} },
                    SettingsTab::Notifications => rsx! { NotificationsPanel {} },
                    SettingsTab::Integrations => rsx! { IntegrationsPanel {} },
                    SettingsTab::Advanced => rsx! { AdvancedPanel {} },
                    SettingsTab::About => rsx! { AboutPanel {} },
                }
                div { class: "settings-footer",
                    if real {
                        SaveState {}
                    } else {
                        span { class: "muted small grow", {tr("Không có gì được ghi ra đĩa hoặc gửi lên máy chủ.")} }
                    }
                    button { class: "btn", r#type: "button", "data-testid": "settings-restore",
                        onclick: move |_| {
                            if app.is_real() {
                                spawn(app.restore_prefs());
                            } else {
                                toggles.set(default_toggles());
                                theme.set(Theme::Light);
                                app.notify(tr("Đã khôi phục cài đặt mặc định (demo)."), false);
                            }
                        },
                        Icon { name: "refresh" } {tr("Khôi phục mặc định")}
                    }
                    button { class: "btn btn-primary", r#type: "button", "data-testid": "settings-save",
                        onclick: move |_| {
                            if app.is_real() {
                                spawn(app.save_prefs());
                            } else {
                                app.notify(tr("Đã lưu thay đổi trong phiên demo (không ghi ra đĩa)."), false);
                            }
                        },
                        Icon { name: "check" } {tr("Lưu thay đổi")}
                    }
                }
            }
        }
    }
}

#[component]
fn ToggleRow(
    key_name: &'static str,
    title: &'static str,
    helper: &'static str,
    #[props(default)] disabled: bool,
) -> Element {
    let app = use_context::<AppState>();
    let mut toggles = app.toggles;
    let checked = toggles.read().get(key_name).copied().unwrap_or(false);
    rsx! {
        div { class: "setting-row",
            span { class: "grow", strong { "{title}" } small { class: "muted", "{helper}" } }
            Switch { testid: format!("settings-toggle-{key_name}"), label: title.to_string(), checked, disabled,
                onchange: move |v| { toggles.write().insert(key_name, v); } }
        }
    }
}

#[component]
pub(crate) fn Panel(
    key_name: &'static str,
    icon: &'static str,
    title: &'static str,
    children: Element,
) -> Element {
    rsx! {
        section { class: "card panel", "data-testid": "settings-panel-{key_name}", "aria-labelledby": "panel-{key_name}",
            div { class: "card-head", Icon { name: icon } h2 { id: "panel-{key_name}", "{title}" } }
            {children}
        }
    }
}

#[component]
fn AccountsPanel() -> Element {
    let app = use_context::<AppState>();
    let mut confirm = app.confirm;
    let mut default_from = app.default_from;
    let detached = app.detached.read().clone();
    let accounts: Vec<_> = app
        .mail
        .read()
        .accounts
        .iter()
        .filter(|a| !detached.contains(&a.id))
        .cloned()
        .collect();
    let def = default_from.read().clone();
    let def_address = accounts
        .iter()
        .find(|a| a.id == def)
        .map(|a| a.address.clone())
        .unwrap_or_default();
    // (account id, "used / total GB" or None when the server has no QUOTA, bar width %)
    let quotas: Vec<(String, Option<String>, String)> = demo::QUOTAS
        .iter()
        .filter(|(id, _, _)| !detached.iter().any(|d| d == id))
        .map(|(id, used, total)| match total {
            Some(t) => (
                id.to_string(),
                Some(format!("{used} / {t} GB")),
                format!("{:.0}", used / t * 100.0),
            ),
            None => (id.to_string(), None, "0".to_string()),
        })
        .collect();
    if app.is_real() {
        return rsx! {
            section { class: "panel-stack", "data-testid": "settings-panel-accounts",
                div { class: "alert alert-info", "data-testid": "settings-real-note",
                    Icon { name: "info" }
                    span { strong { {tr("Chế độ thật. ")} } {tr("Tài khoản lưu trên máy này: mật khẩu trong Keychain, tiêu đề thư trong cơ sở dữ liệu cục bộ. MailBox chỉ đọc hộp thư đến.")} }
                }
                div { class: "section-head",
                    h2 { {bi(format!("Tài khoản đã thêm ({})", accounts.len()), format!("Added accounts ({})", accounts.len()))} }
                    button { class: "btn btn-primary", r#type: "button", "data-testid": "settings-real-connect", onclick: move |_| app.go(Screen::Connect), Icon { name: "plus" } {tr("Thêm tài khoản")} }
                }
                if !accounts.is_empty() {
                    RealIdentity {}
                    div { class: "card list-card",
                        for a in accounts.iter() {
                            {
                                let id = a.id.clone();
                                let address = a.address.clone();
                                rsx! {
                                    div { key: "{a.id}", class: "account-card", "data-testid": "settings-account-{a.id}",
                                        span { class: "acc-icon", style: "color:{a.color}", Icon { name: "mail" } }
                                        span { class: "acc-name", strong { "{a.address}" } small { class: "muted", {bi(format!("{} thư chưa đọc trong hộp thư đến", a.unread), format!("{} unread in the inbox", a.unread))} } }
                                        span { class: "badge badge-ok", "data-testid": "settings-account-status-{a.id}", "data-status": "verified", Icon { name: "check", size: 14 } {tr("Đã xác minh IMAP")} }
                                        span { class: "acc-actions",
                                            button { class: "btn btn-sm danger-text", r#type: "button", "data-testid": "settings-detach-{a.id}",
                                                onclick: move |_| confirm.set(Some(Confirm {
                                                    title: bi(format!("Gỡ tài khoản {address} khỏi máy này?"), format!("Remove account {address} from this computer?")),
                                                    body: tr("Xóa thông tin tài khoản, tiêu đề thư đã lưu và mật khẩu trong Keychain trên máy này. Thư trên máy chủ không bị xóa.").into(),
                                                    ok_label: tr("Gỡ khỏi MailBox"),
                                                    danger: true,
                                                    action: ConfirmAction::DetachAccount(id.clone()),
                                                })),
                                                Icon { name: "trash", size: 14 } {tr("Gỡ khỏi MailBox")}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        };
    }
    rsx! {
        section { class: "panel-stack", "data-testid": "settings-panel-accounts",
            div { class: "card overview",
                div { class: "grow", strong { {tr("Tổng quan tài khoản")} } small { class: "muted", {bi(format!("{} tài khoản minh họa · Mặc định gửi: {}", accounts.len(), def_address), format!("{} sample accounts · Default sender: {}", accounts.len(), def_address))} } }
                span { class: "stat", Icon { name: "mail" } span { strong { "{accounts.len()}" } small { class: "muted", {tr("Tài khoản (demo)")} } } }
                span { class: "stat stat-warn", Icon { name: "info" } span { strong { {tr("Chưa đồng bộ thật")} } small { {tr("Dữ liệu minh họa")} } } }
            }
            div { class: "section-head",
                h2 { {bi(format!("Tài khoản đã thêm ({})", accounts.len()), format!("Added accounts ({})", accounts.len()))} }
                button { class: "btn", r#type: "button", onclick: move |_| app.go(Screen::Connect), Icon { name: "plus" } {tr("Thêm tài khoản")} }
            }
            div { class: "card list-card",
                for a in accounts.iter() {
                    {
                        let id = a.id.clone();
                        let name = a.name.clone();
                        rsx! {
                            div { key: "{a.id}", class: "account-card", "data-testid": "settings-account-{a.id}",
                                span { class: "acc-icon", style: "color:{a.color}", Icon { name: account_icon(&a.id) } }
                                span { class: "acc-name",
                                    strong { "{a.name}" }
                                    if a.id == def { span { class: "badge badge-blue", {tr("Mặc định")} } }
                                    small { class: "muted", "{a.address}" }
                                }
                                span { "data-testid": "settings-account-status-{a.id}", "data-status": "demo",
                                    DemoBadge { label: tr("Demo · chưa kiểm tra IMAP/SMTP") }
                                }
                                span { class: "acc-actions",
                                    button { class: "btn btn-sm", r#type: "button", onclick: move |_| app.notify(tr("Chỉnh sửa tài khoản thật có ở MB-111."), false), Icon { name: "pen", size: 14 } {tr("Sửa")} }
                                    button { class: "btn btn-sm danger-text", r#type: "button", "data-testid": "settings-detach-{a.id}",
                                        onclick: move |_| confirm.set(Some(Confirm {
                                            title: bi(format!("Gỡ tài khoản {name} khỏi MailBox?"), format!("Remove account {name} from MailBox?")),
                                            body: tr("Chỉ xóa cấu hình và bộ nhớ đệm trên thiết bị này. Thư trên máy chủ không bị xóa.").into(),
                                            ok_label: tr("Gỡ khỏi MailBox"),
                                            danger: true,
                                            action: ConfirmAction::DetachAccount(id.clone()),
                                        })),
                                        Icon { name: "trash", size: 14 } {tr("Gỡ khỏi MailBox")}
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "grid-2",
                div { class: "card",
                    div { class: "card-head", Icon { name: "database" } h2 { {tr("Dung lượng hộp thư")} } }
                    for (id, label, width) in quotas.iter() {
                        div { key: "{id}", class: "quota-row",
                            strong { "{id}" }
                            if let Some(label) = label {
                                span { class: "meter grow", span { style: "width:{width}%" } }
                                span { class: "small", "{label}" }
                            } else {
                                span { class: "muted small grow", {tr("Không rõ — máy chủ không hỗ trợ QUOTA")} }
                            }
                        }
                    }
                    p { class: "muted small", {tr("Số liệu minh họa; bản thật đọc từ QUOTA của máy chủ.")} }
                }
                div { class: "card",
                    div { class: "card-head", Icon { name: "send" } h2 { {tr("Tài khoản gửi mặc định")} } }
                    p { class: "muted small", {tr("Dùng khi soạn thư mới. Trả lời luôn dùng tài khoản đã nhận thư gốc.")} }
                    select { class: "input", "data-testid": "settings-default-identity", "aria-label": tr("Tài khoản gửi mặc định"), value: "{def}",
                        onchange: move |e| default_from.set(e.value()),
                        for a in accounts.iter() { option { key: "{a.id}", value: "{a.id}", selected: a.id == def, "{a.name} – {a.address}" } }
                    }
                    div { class: "card-head", Icon { name: "user" } h2 { {tr("Bí danh (Aliases)")} } }
                    for alias in demo::ALIASES {
                        div { key: "{alias}", class: "setting-row", span { class: "grow", "{alias}" } span { class: "badge badge-neutral", {tr("Chưa xác minh")} } }
                    }
                }
            }
        }
    }
}

#[component]
fn SyncPanel() -> Element {
    let real = use_context::<AppState>().is_real();
    rsx! {
        Panel { key_name: "sync", icon: "refresh", title: tr("Đồng bộ"),
            if real {
                crate::components::live::LiveStatusList {}
            } else {
                div { class: "alert alert-warn", Icon { name: "info" } span { {tr("Chưa kích hoạt: bản demo không kết nối máy chủ. Khi có kết nối thật, mỗi tài khoản hiển thị thời điểm và lỗi riêng.")} } }
            }
            if real {
                super::settings_sync::RealAutoSync {}
            } else {
                ToggleRow { key_name: "auto-sync", title: tr("Đồng bộ tự động"), helper: tr("IDLE nếu máy chủ hỗ trợ, nếu không thì định kỳ") }
            }
            div { class: "setting-row", span { class: "grow", strong { {tr("Chu kỳ kiểm tra")} } } select { class: "input input-sm", "aria-label": tr("Chu kỳ kiểm tra"), option { {tr("5 phút")} } option { {tr("15 phút")} } option { {tr("30 phút")} } } }
            div { class: "setting-row", span { class: "grow", strong { {tr("Tải trước")} } small { class: "muted", {tr("Số ngày thư lưu sẵn trên thiết bị")} } } select { class: "input input-sm", "aria-label": tr("Số ngày tải trước"), option { {tr("30 ngày")} } option { {tr("90 ngày")} } } }
            button { class: "btn", r#type: "button", disabled: true, Icon { name: "file" } {tr("Nhật ký đồng bộ (đã ẩn thông tin cá nhân) — sau MB-140")} }
        }
    }
}

#[component]
fn AppearancePanel() -> Element {
    let app = use_context::<AppState>();
    let mut theme = app.theme;
    let current = *theme.read();
    rsx! {
        Panel { key_name: "appearance", icon: "palette", title: tr("Giao diện"),
            div { class: "setting-col",
                strong { {tr("Chủ đề")} }
                div { class: "segmented", role: "radiogroup", "aria-label": tr("Chủ đề"),
                    for (t, key, label) in [(Theme::Light, "light", tr("Sáng")), (Theme::Dark, "dark", tr("Tối")), (Theme::System, "system", tr("Theo hệ thống"))] {
                        button { key: "{key}", class: if current == t { "seg on" } else { "seg" }, r#type: "button", role: "radio",
                            "aria-checked": if current == t { "true" } else { "false" },
                            "data-testid": "settings-theme-{key}", onclick: move |_| theme.set(t), "{label}" }
                    }
                }
                small { class: "muted", {tr("\"Theo hệ thống\" dùng giao diện sáng trong bản demo.")} }
            }
            crate::components::lang_picker::LanguagePicker {}
            ToggleRow { key_name: "reduce-motion", title: tr("Giảm chuyển động"), helper: tr("Tắt hiệu ứng chuyển cảnh") }
        }
    }
}

#[component]
fn SecurityPanel() -> Element {
    rsx! {
        Panel { key_name: "security", icon: "shield", title: tr("Bảo mật & quyền riêng tư"),
            ToggleRow { key_name: "remote-images", title: tr("Tải ảnh từ xa"), helper: tr("Chặn mặc định để chống theo dõi") }
            ToggleRow { key_name: "app-lock", title: tr("Khóa ứng dụng (Desktop)"), helper: "Sau MVP", disabled: true }
            div { class: "setting-row", Icon { name: "laptop" } span { class: "grow", strong { {tr("Phiên này")} } small { class: "muted", {tr("Thiết bị hiện tại · minh họa")} } } span { class: "badge badge-blue", {tr("Hiện tại")} } }
            p { class: "muted small", {tr("Mật khẩu không bao giờ hiển thị hoặc lưu dạng văn bản. HTML trong thư luôn được làm sạch và cách ly.")} }
        }
    }
}

#[component]
fn SignaturePanel() -> Element {
    let mut signature =
        use_signal(|| tr("Minh Nguyễn\nProduct Manager · Công ty\nminh@company.vn").to_string());
    rsx! {
        Panel { key_name: "signature", icon: "pen", title: tr("Chữ ký"),
            label { class: "field", span { {tr("Chữ ký cho Work (văn bản thuần trong bản demo)")} }
                textarea { class: "input textarea", "data-testid": "settings-signature", value: "{signature}", oninput: move |e| signature.set(e.value()) }
            }
            ToggleRow { key_name: "signature-reply", title: tr("Thêm khi trả lời / chuyển tiếp"), helper: tr("Áp dụng theo danh tính gửi") }
        }
    }
}

#[component]
fn NotificationsPanel() -> Element {
    rsx! {
        Panel { key_name: "notifications", icon: "bell", title: tr("Thông báo"),
            ToggleRow { key_name: "new-mail", title: tr("Email mới"), helper: tr("Trong ứng dụng") }
            ToggleRow { key_name: "desktop", title: tr("Thông báo hệ điều hành"), helper: "Desktop · MB-211" }
            ToggleRow { key_name: "show-content", title: tr("Hiện người gửi và tiêu đề"), helper: tr("Tắt: chỉ \"Có thư mới\"") }
            ToggleRow { key_name: "sound", title: tr("Âm thanh"), helper: tr("Phát âm thanh khi có thư mới") }
            ToggleRow { key_name: "sync-errors", title: tr("Báo lỗi đồng bộ"), helper: tr("Khi một tài khoản bị gián đoạn") }
        }
    }
}

#[component]
fn IntegrationsPanel() -> Element {
    rsx! {
        Panel { key_name: "integrations", icon: "puzzle", title: tr("Tiện ích tích hợp"),
            div { class: "empty-inline", Icon { name: "puzzle", size: 26 } strong { {tr("Chưa có tích hợp")} } small { class: "muted", {tr("Lịch, danh bạ và ứng dụng bên thứ ba nằm ngoài phạm vi MVP.")} } }
        }
    }
}

#[component]
fn AdvancedPanel() -> Element {
    let app = use_context::<AppState>();
    rsx! {
        Panel { key_name: "advanced", icon: "sliders", title: tr("Nâng cao"),
            crate::components::shortcuts::ShortcutSettings {}
            div { class: "setting-row",
                span { class: "grow", strong { {tr("Xóa bộ nhớ đệm cục bộ")} } small { class: "muted", {tr("Không ảnh hưởng thư trên máy chủ")} } }
                button { class: "btn btn-sm danger-text", r#type: "button", onclick: move |_| app.notify(tr("Bản demo không có bộ nhớ đệm để xóa."), false), {tr("Xóa…")} }
            }
        }
    }
}

#[component]
fn AboutPanel() -> Element {
    rsx! {
        Panel { key_name: "about", icon: "info", title: tr("Giới thiệu MailBox"),
            div { class: "setting-row", span { class: "grow", {tr("Phiên bản")} } strong { {env!("CARGO_PKG_VERSION")} } }
            div { class: "setting-row", span { class: "grow", {tr("Nền tảng")} } strong { "Rust · Dioxus 0.7" } }
            div { class: "setting-row", span { class: "grow", {tr("Giấy phép")} } strong { "Apache-2.0" } }
            div { class: "setting-row", span { class: "grow", {tr("Chế độ dữ liệu")} } DemoBadge { label: tr("Minh họa") } }
            p { class: "muted small", {tr("Biểu tượng theo phong cách Lucide (ISC).")} }
        }
    }
}
