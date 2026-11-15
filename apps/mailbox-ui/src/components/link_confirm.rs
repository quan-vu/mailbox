//! Confirmation before a link from a message is opened (AUDIT S-09).
//!
//! Shows the real host and the full, parsed address. What is shown is exactly what is opened.
//! A deceptive or unsupported address gets an explanation and no way to open it.
use crate::components::icon::Icon;
use crate::i18n::tr;
use crate::state::link::{PENDING, dismiss};
use dioxus::prelude::*;
use mailbox_core::link::{LinkKind, LinkRefusal, LinkTarget};

/// Hand the confirmed address to the operating system. The Web build has no mail links to open.
fn open(target: &LinkTarget) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    return crate::adapters::link::open(target);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = target;
        false
    }
}

#[component]
pub fn LinkConfirmHost() -> Element {
    let mut failed = use_signal(|| false);
    let pending = PENDING.read().clone();
    let mut close = move || {
        failed.set(false);
        dismiss();
    };
    let Some(decision) = pending else {
        return rsx! {};
    };
    rsx! {
        div { class: "overlay",
            onclick: move |_| close(),
            onkeydown: move |e| {
                if e.key() == Key::Escape {
                    e.stop_propagation();
                    close();
                }
            },
            match decision {
                Ok(target) => {
                    let mail = target.kind == LinkKind::Mail;
                    let confirmed = target.clone();
                    rsx! {
                        div {
                            class: "dialog link-dialog",
                            role: "dialog",
                            "aria-modal": "true",
                            "aria-labelledby": "link-title",
                            "data-testid": "link-confirm",
                            "data-kind": if mail { "mail" } else { "web" },
                            onclick: move |e| e.stop_propagation(),
                            h2 { id: "link-title", {tr("Mở liên kết này?")} }
                            p { {tr("Liên kết này nằm trong thư và sẽ mở bên ngoài MailBox. Hãy kiểm tra địa chỉ thật bên dưới: chữ hiển thị trong thư có thể khác.")} }
                            dl { class: "link-facts",
                                dt { if mail { {tr("Người nhận")} } else { {tr("Trang web")} } }
                                dd { class: "link-host", "data-testid": "link-confirm-host", "{target.host}" }
                                dt { {tr("Địa chỉ đầy đủ")} }
                                dd { class: "link-url", "data-testid": "link-confirm-url", "{target.url}" }
                            }
                            if !target.secure {
                                div { class: "alert alert-warn", "data-testid": "link-confirm-insecure",
                                    Icon { name: "alert" }
                                    span { {tr("Kết nối không mã hóa (http): người khác trên mạng có thể đọc hoặc sửa nội dung.")} }
                                }
                            }
                            if target.punycode {
                                div { class: "alert alert-warn", "data-testid": "link-confirm-lookalike",
                                    Icon { name: "alert" }
                                    span { {tr("Tên miền này dùng ký tự quốc tế (dạng xn--), cách thường được dùng để bắt chước một tên quen thuộc.")} }
                                }
                            }
                            if failed() {
                                div { class: "alert alert-danger", role: "alert", "data-testid": "link-confirm-failed",
                                    Icon { name: "alert" }
                                    span { {tr("Không mở được. Bạn có thể chép địa chỉ ở trên và tự mở.")} }
                                }
                            }
                            div { class: "dialog-actions",
                                button { class: "btn", r#type: "button", "data-testid": "link-confirm-cancel",
                                    onmounted: move |e| async move { let _ = e.set_focus(true).await; },
                                    onclick: move |_| close(), {tr("Hủy")}
                                }
                                button { class: "btn btn-primary", r#type: "button", "data-testid": "link-confirm-open",
                                    onclick: move |_| {
                                        if open(&confirmed) {
                                            close();
                                        } else {
                                            failed.set(true);
                                        }
                                    },
                                    if mail { {tr("Mở ứng dụng thư")} } else { {tr("Mở trong trình duyệt")} }
                                }
                            }
                        }
                    }
                }
                Err(why) => {
                    let (reason, text) = match why {
                        LinkRefusal::Credentials => (
                            "credentials",
                            tr("Địa chỉ có tên đăng nhập đứng trước dấu @ của tên miền. Đây là cách thường dùng để giả mạo tên trang web, nên MailBox không mở."),
                        ),
                        LinkRefusal::Scheme => (
                            "unsupported",
                            tr("MailBox chỉ mở liên kết http, https và mailto. Liên kết này thuộc loại khác."),
                        ),
                        LinkRefusal::TooLong => (
                            "too-long",
                            tr("Địa chỉ của liên kết quá dài nên MailBox không mở."),
                        ),
                        LinkRefusal::Malformed => (
                            "malformed",
                            tr("Địa chỉ của liên kết không hợp lệ."),
                        ),
                    };
                    rsx! {
                        div {
                            class: "dialog link-dialog",
                            role: "alertdialog",
                            "aria-modal": "true",
                            "aria-labelledby": "link-title",
                            "data-testid": "link-refused",
                            "data-reason": reason,
                            onclick: move |e| e.stop_propagation(),
                            h2 { id: "link-title", {tr("MailBox không mở liên kết này")} }
                            p { "{text}" }
                            div { class: "dialog-actions",
                                button { class: "btn btn-primary", r#type: "button", "data-testid": "link-refused-close",
                                    onmounted: move |e| async move { let _ = e.set_focus(true).await; },
                                    onclick: move |_| close(), {tr("Đóng")}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
