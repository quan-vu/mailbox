//! Message body in the reader and the thread view (MB-130, MB-131).
//!
//! Demo mode shows the fixture text exactly as before. Real mode loads the body through
//! `adapters::body` and shows sanitized HTML only inside a sandboxed frame: no scripts, no
//! same-origin access (so no path to the Desktop bridge) and a CSP that lets nothing load.
use crate::adapters::body::{
    AttachmentView, BodyState, BodyView, FRAME_SANDBOX, frame_document, load_body, save_attachment,
};
use crate::components::icon::Icon;
use crate::i18n::{bi, tr};
use crate::state::AppState;
use crate::state::compose::format_size;
use dioxus::prelude::*;
use mailbox_contracts::MessageSummary;

/// Demo message that carries sample links (the security notice).
const DEMO_LINK_MESSAGE: &str = "m06";

/// Sample links for the demo: a normal one, an unencrypted one, a look-alike name and a
/// deceptive address. They go through the same guard and sandbox as real mail.
const DEMO_LINKS_HTML: &str = "<ul>\
<li><a href=\"https://example.org/account/activity\">Xem hoạt động gần đây</a> (https)</li>\
<li><a href=\"http://example.org/help\">Trang trợ giúp</a> (http, không mã hóa)</li>\
<li><a href=\"https://xn--exmple-cua.org/login\">Tên miền giống thật</a> (xn--)</li>\
<li><a href=\"https://bank.example@evil.example/login\">Địa chỉ đánh lừa</a> (bị từ chối)</li>\
<li><a href=\"mailto:support@example.org\">support@example.org</a> (mailto)</li>\
</ul>";

#[component]
pub fn MessageBody(message: MessageSummary) -> Element {
    let app = use_context::<AppState>();
    if !app.is_real() {
        let links =
            (message.id == DEMO_LINK_MESSAGE).then(|| frame_document(DEMO_LINKS_HTML, false));
        return rsx! {
            div { class: "body-text", "{message.body_plain}" }
            if let Some(document) = links {
                p { class: "muted small", "data-testid": "demo-link-note",
                    {tr("Liên kết mẫu (demo) để thử hộp xác nhận trước khi mở:")}
                }
                iframe {
                    class: "mail-frame",
                    "data-testid": "demo-link-frame",
                    title: tr("Nội dung thư"),
                    "sandbox": FRAME_SANDBOX,
                    "referrerpolicy": "no-referrer",
                    "srcdoc": "{document}",
                }
            }
        };
    }
    rsx! { RealBody { id: message.id.clone() } }
}

#[component]
fn RealBody(id: ReadSignal<String>) -> Element {
    let mut attempt = use_signal(|| 0u32);
    // Per message: the ids for which the user asked for remote images / the plain-text view.
    let mut images_for = use_signal(|| None::<String>);
    let mut plain_for = use_signal(|| None::<String>);
    let body = use_resource(move || {
        let id = id();
        attempt();
        async move {
            let result = load_body(&id).await;
            (id, result)
        }
    });
    let current = id();
    // A result that belongs to another message is never shown under this one.
    let state = match &*body.read() {
        Some((loaded, result)) if *loaded == current => BodyState::from_result(result.clone()),
        _ => BodyState::Loading,
    };
    let show_images = images_for.read().as_deref() == Some(current.as_str());
    let plain_view = plain_for.read().as_deref() == Some(current.as_str());
    let (id_images, id_plain) = (current.clone(), current.clone());
    rsx! {
        BodyPane {
            id: current,
            state,
            show_images,
            plain_view,
            on_retry: move |_| attempt += 1,
            on_images: move |show: bool| images_for.set(show.then(|| id_images.clone())),
            on_plain: move |plain: bool| plain_for.set(plain.then(|| id_plain.clone())),
        }
    }
}

/// Presentation of one body state. Holds no state of its own, so every state is testable.
#[component]
pub fn BodyPane(
    id: String,
    state: BodyState,
    show_images: bool,
    plain_view: bool,
    on_retry: EventHandler<()>,
    on_images: EventHandler<bool>,
    on_plain: EventHandler<bool>,
) -> Element {
    let key = state.key();
    rsx! {
        div { class: "mail-body", "data-testid": "message-body", "data-state": key, "data-message": "{id}",
            match state {
                BodyState::Loading => rsx! {
                    div { class: "body-loading", "data-testid": "body-loading", role: "status", "aria-live": "polite",
                        span { class: "sr-only", {tr("Đang tải nội dung thư…")} }
                        span { class: "skeleton", "aria-hidden": "true" }
                        span { class: "skeleton", "aria-hidden": "true" }
                        span { class: "skeleton short", "aria-hidden": "true" }
                    }
                },
                BodyState::TooLarge => rsx! {
                    div { class: "alert alert-warn", "data-testid": "body-too-large", role: "status",
                        Icon { name: "alert" }
                        span { class: "grow", "{crate::adapters::body::BodyFailure::TooLarge.message()}" }
                    }
                },
                BodyState::Failed(failure) => rsx! {
                    div { class: "alert alert-danger", "data-testid": "body-error", "data-reason": failure.key(), role: "alert",
                        Icon { name: "alert" }
                        span { class: "grow", "{failure.message()}" }
                        button { class: "btn btn-sm", r#type: "button", "data-testid": "body-retry",
                            onclick: move |_| on_retry.call(()),
                            Icon { name: "refresh", size: 14 } {tr("Thử lại")}
                        }
                    }
                },
                BodyState::Loaded(view) => rsx! {
                    LoadedBody { id: id.clone(), view, show_images, plain_view, on_images, on_plain }
                },
            }
        }
    }
}

#[component]
fn LoadedBody(
    id: String,
    view: BodyView,
    show_images: bool,
    plain_view: bool,
    on_images: EventHandler<bool>,
    on_plain: EventHandler<bool>,
) -> Element {
    // Remote images are only ever shown from the dedicated variant, after the explicit action.
    let images = show_images && view.html_remote.is_some();
    let html = if images {
        view.html_remote.clone()
    } else {
        view.html.clone()
    };
    let has_html = html.is_some();
    let frame = html
        .filter(|_| !plain_view)
        .map(|h| (format!("{id}:{images}"), frame_document(&h, images)));
    let can_show = view.html_remote.is_some();
    let total: u64 = view.attachments.iter().map(|a| a.size_bytes).sum();
    let total_size = format_size(total);
    let count = view.attachments.len();
    rsx! {
        if view.truncated {
            div { class: "alert alert-info", "data-testid": "body-truncated", role: "status",
                Icon { name: "info" }
                span { class: "grow", {tr("Thư rất dài nên MailBox chỉ hiện một phần nội dung.")} }
            }
        }
        if view.remote_blocked && !plain_view {
            if images {
                div { class: "alert alert-warn remote-banner", "data-testid": "body-images-shown", role: "status",
                    Icon { name: "image" }
                    span { class: "grow", {tr("Đang hiện ảnh từ xa cho thư này. Người gửi có thể biết bạn đã mở thư.")} }
                    button { class: "btn btn-sm", r#type: "button", "data-testid": "body-hide-images",
                        onclick: move |_| on_images.call(false), {tr("Chặn lại")}
                    }
                }
            } else {
                div { class: "alert alert-info remote-banner", "data-testid": "body-remote-blocked", role: "status",
                    Icon { name: "shield" }
                    span { class: "grow", {tr("Đã chặn ảnh và nội dung từ xa để người gửi không biết bạn đã mở thư.")} }
                    if can_show {
                        button { class: "btn btn-sm", r#type: "button", "data-testid": "body-show-images",
                            title: tr("Chỉ áp dụng cho thư này"),
                            onclick: move |_| on_images.call(true), {tr("Hiện ảnh cho thư này")}
                        }
                    }
                }
            }
        }
        if has_html {
            div { class: "body-view-switch",
                button { class: "link-btn", r#type: "button", "data-testid": "body-view-toggle",
                    "aria-pressed": if plain_view { "true" } else { "false" },
                    onclick: move |_| on_plain.call(!plain_view),
                    if plain_view { {tr("Xem bản định dạng")} } else { {tr("Xem bản văn bản thuần")} }
                }
            }
        }
        if let Some((frame_key, document)) = frame {
            // A new element per message and mode, so the sandbox is in force before any content.
            for k in std::iter::once(frame_key) {
                iframe {
                    key: "{k}",
                    class: "mail-frame",
                    "data-testid": "body-html-frame",
                    "data-remote-images": if images { "shown" } else { "blocked" },
                    title: tr("Nội dung thư"),
                    "sandbox": FRAME_SANDBOX,
                    "referrerpolicy": "no-referrer",
                    "srcdoc": "{document}",
                }
            }
        } else {
            div { class: "body-text", "data-testid": "body-text", "{view.text}" }
        }
        if count > 0 {
            div { class: "attachments-head", "data-testid": "body-attachments", "data-count": "{count}",
                Icon { name: "clip" }
                strong { {bi(format!("{count} tệp đính kèm"), format!("{count} attachment{}", crate::i18n::s(count)))} }
                span { class: "muted", "({total_size})" }
                span { class: "muted small push", {tr("Tệp được lưu vào thư mục Tải về, không tự mở")} }
            }
            div { class: "attachments",
                for a in view.attachments.iter() {
                    RealAttachment { key: "{a.part}", message_id: id.clone(), attachment: a.clone() }
                }
            }
        }
    }
}

fn file_kind(media_type: &str) -> (&'static str, &'static str) {
    if media_type.contains("pdf") {
        ("PDF", "ft-pdf")
    } else if media_type.contains("sheet") || media_type.contains("excel") {
        ("XLS", "ft-xls")
    } else if media_type.starts_with("image/") {
        ("IMG", "ft-generic")
    } else {
        ("FILE", "ft-generic")
    }
}

#[component]
fn RealAttachment(message_id: String, attachment: AttachmentView) -> Element {
    let app = use_context::<AppState>();
    let mut saving = use_signal(|| false);
    let (kind, class) = file_kind(&attachment.media_type);
    let size = format_size(attachment.size_bytes);
    let part = attachment.part;
    let busy = saving();
    rsx! {
        div { class: "attachment", "data-testid": "attachment-real-{part}", "data-inline": if attachment.inline { "true" } else { "false" },
            span { class: "file-type {class}", "aria-hidden": "true", "{kind}" }
            span { class: "attachment-text",
                strong { title: "{attachment.filename}", "{attachment.filename}" }
                small { if attachment.inline { {bi(format!("{size} · ảnh trong thư"), format!("{size} · inline image"))} } else { "{size}" } }
            }
            button { class: "icon-btn", r#type: "button", "data-testid": "attachment-save-{part}", disabled: busy,
                "aria-label": bi(format!("Lưu {} vào thư mục Tải về", attachment.filename), format!("Save {} to the Downloads folder", attachment.filename)),
                title: tr("Lưu vào thư mục Tải về"),
                onclick: move |_| {
                    if saving() { return; }
                    saving.set(true);
                    let id = message_id.clone();
                    spawn(async move {
                        match save_attachment(&id, part).await {
                            Ok(name) => app.notify(bi(format!("Đã lưu \"{name}\" vào thư mục Tải về. MailBox không tự mở tệp."), format!("Saved \"{name}\" to the Downloads folder. MailBox does not open files by itself.")), false),
                            Err(e) => app.notify(e.message(), false),
                        }
                        saving.set(false);
                    });
                },
                Icon { name: "download" }
            }
        }
    }
}
