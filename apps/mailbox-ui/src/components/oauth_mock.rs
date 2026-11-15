//! MB-203, shipped as a labelled mock. "Sign in with Google / Microsoft" needs an OAuth client
//! registered by the project owner, so these buttons only open this explanation. They start no
//! browser, ask for no token or password, and make no network call.
use crate::components::icon::Icon;
use crate::i18n::{bi, tr};
use dioxus::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthProvider {
    Google,
    Microsoft,
}

impl OAuthProvider {
    pub const ALL: [OAuthProvider; 2] = [OAuthProvider::Google, OAuthProvider::Microsoft];

    pub fn key(self) -> &'static str {
        match self {
            OAuthProvider::Google => "google",
            OAuthProvider::Microsoft => "microsoft",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            OAuthProvider::Google => "Google",
            OAuthProvider::Microsoft => "Microsoft",
        }
    }

    fn app_password_hint(self) -> &'static str {
        match self {
            OAuthProvider::Google => tr(
                "Gmail: bật xác minh 2 bước, tạo \"Mật khẩu ứng dụng\" trong tài khoản Google rồi dán vào ô mật khẩu.",
            ),
            OAuthProvider::Microsoft => tr(
                "Microsoft 365 / Outlook.com: nhiều tổ chức đã tắt đăng nhập bằng mật khẩu IMAP. Nếu quản trị viên cho phép, dùng mật khẩu ứng dụng; nếu không, cần OAuth (chưa có trong bản này).",
            ),
        }
    }
}

/// The row of mock buttons shown above the provider grid.
#[component]
pub fn OAuthMockButtons(
    selected: Option<OAuthProvider>,
    onpick: EventHandler<OAuthProvider>,
) -> Element {
    rsx! {
        div { class: "oauth-mock-row", "data-testid": "oauth-mock-buttons",
            for p in OAuthProvider::ALL {
                button { key: "{p.key()}", class: if selected == Some(p) { "btn oauth-btn chosen" } else { "btn oauth-btn" }, r#type: "button",
                    "data-testid": "oauth-mock-{p.key()}", "aria-pressed": if selected == Some(p) { "true" } else { "false" },
                    onclick: move |_| onpick.call(p),
                    Icon { name: "lock", size: 16 }
                    {bi(format!("Đăng nhập bằng {} ", p.name()), format!("Sign in with {} ", p.name()))}
                    span { class: "badge badge-warn", {tr("mock / chưa hoạt động thật")} }
                }
            }
        }
    }
}

#[component]
pub fn OAuthMockPanel(provider: OAuthProvider, onclose: EventHandler<()>) -> Element {
    let name = provider.name();
    rsx! {
        section { class: "alert alert-warn oauth-mock-panel", role: "region", "aria-label": bi(format!("Đăng nhập bằng {name} (mock)"), format!("Sign in with {name} (mock)")), "data-testid": "oauth-mock-panel", "data-provider": "{provider.key()}",
            Icon { name: "info" }
            div { class: "grow",
                p { strong { {tr("Mock / chưa hoạt động thật. ")} } {bi(format!("Đăng nhập bằng {name} (OAuth) chưa có trong bản này. Không có trình duyệt nào được mở, không có token được nhập hay lưu, và không có kết nối mạng nào được thực hiện."), format!("Sign-in with {name} (OAuth) is not in this version. No browser is opened, no token is entered or stored, and no network connection is made."))} }
                p { class: "small", strong { {tr("Cần gì để có bản thật: ")} } {bi(format!("một ứng dụng OAuth do chủ dự án đăng ký với {name} (client ID, màn hình đồng ý và xác minh ứng dụng) cùng cấu hình địa chỉ chuyển hướng."), format!("an OAuth app registered by the project owner with {name} (client ID, consent screen and app verification) plus redirect configuration."))} }
                p { class: "small", strong { {tr("Dùng được ngay: ")} } {tr("đăng nhập bằng mật khẩu ứng dụng. ")} "{provider.app_password_hint()}" }
                p { class: "small", {tr("Nhập địa chỉ email bên dưới, bật \"Tự động phát hiện cấu hình\" để MailBox đề xuất máy chủ IMAP/SMTP, rồi kiểm tra kết nối.")} }
                button { class: "btn btn-sm", r#type: "button", "data-testid": "oauth-mock-close", onclick: move |_| onclose.call(()), {tr("Đã hiểu, dùng mật khẩu ứng dụng")} }
            }
        }
    }
}
