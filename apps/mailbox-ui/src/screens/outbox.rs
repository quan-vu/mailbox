//! Local drafts and outbox (MB-133, MB-151), shown for the Drafts entry in Real mode.
//! Every row shows the stored state; "may have been sent" is its own state, never "failed".
use crate::components::common::EmptyState;
use crate::components::icon::Icon;
use crate::i18n::{bi, tr};
use crate::state::AppState;
use crate::state::compose::parse_recipient;
use crate::state::outbox::{self, OUTBOX, state_label};
use dioxus::prelude::*;
use mailbox_core::outbox::OutboxState;

static COMPOSE_CSS: Asset = asset!("/assets/compose.css");

#[component]
pub fn OutboxScreen() -> Element {
    let app = use_context::<AppState>();
    let ui = OUTBOX.read().clone();
    let mut pending_delete = use_signal(|| None::<String>);
    use_future(move || async move {
        if !OUTBOX.peek().items_loaded {
            outbox::refresh_items().await;
        }
    });
    rsx! {
        document::Stylesheet { href: COMPOSE_CSS }
        section { class: "outbox", "data-testid": "screen-outbox", "aria-label": tr("Bản nháp và thư đang gửi"),
            header { class: "outbox-head",
                h1 { {tr("Bản nháp và thư đang gửi")} }
                p { class: "muted small", {tr("Bản nháp chỉ lưu trên máy này; chưa đồng bộ lên thư mục Nháp của máy chủ.")} }
                crate::components::schedule::OutboxScheduleNote {}
                button { class: "btn btn-sm push", r#type: "button", "data-testid": "outbox-refresh",
                    onclick: move |_| { spawn(outbox::refresh_items()); },
                    Icon { name: "refresh", size: 14 } {tr("Tải lại")} }
            }
            if ui.items_error {
                div { class: "alert alert-danger", role: "alert", "data-testid": "outbox-error",
                    Icon { name: "alert" }
                    span { {tr("Không đọc được bản nháp trên máy này. Thử tải lại.")} }
                }
            }
            if ui.items.is_empty() && ui.items_loaded && !ui.items_error {
                EmptyState { icon: "pen", title: tr("Chưa có bản nháp nào").to_string(), detail: tr("Thư bạn đang viết sẽ tự lưu ở đây và vẫn còn sau khi mở lại ứng dụng.").to_string(), testid: "outbox-empty" }
            }
            ul { class: "outbox-list",
                for item in ui.items.iter() {
                    {
                        let (label, key) = state_label(item.state);
                        let id = item.id.clone();
                        let record = item.clone();
                        let subject = if item.subject.trim().is_empty() { {tr("(Không có tiêu đề)")}.to_string() } else { item.subject.clone() };
                        let recipients = item
                            .to
                            .iter()
                            .chain(&item.cc)
                            .chain(&item.bcc)
                            .filter_map(|e| parse_recipient(e))
                            .map(|r| r.display)
                            .collect::<Vec<_>>()
                            .join(", ");
                        let recipients = if recipients.is_empty() { {tr("(Chưa có người nhận)")}.to_string() } else { recipients };
                        let in_flight = matches!(item.state, OutboxState::Queued | OutboxState::Sending);
                        let delete_id = item.id.clone();
                        rsx! {
                            li { key: "{id}", class: "outbox-item", "data-testid": "outbox-item-{id}", "data-state": key,
                                span { class: "badge outbox-badge state-{key}", "{label}" }
                                span { class: "outbox-text",
                                    strong { "{subject}" }
                                    small { class: "muted", {bi(format!("Đến: {recipients}"), format!("To: {recipients}"))} }
                                }
                                if item.state == OutboxState::Scheduled {
                                    crate::components::schedule::OutboxScheduleInfo { id: id.clone(), at: item.scheduled_at }
                                }
                                button { class: "btn btn-sm", r#type: "button", "data-testid": "outbox-open-{id}",
                                    onclick: move |_| outbox::open_item(app, &record), {tr("Mở")} }
                                button { class: "icon-btn", r#type: "button", "data-testid": "outbox-delete-{id}", disabled: in_flight, "aria-label": bi(format!("Xóa {} khỏi máy này", subject), format!("Delete {} from this computer", subject)),
                                    onclick: move |_| pending_delete.set(Some(delete_id.clone())),
                                    Icon { name: "trash", size: 16 }
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some(id) = pending_delete() {
            div { class: "overlay", onclick: move |_| pending_delete.set(None),
                div { class: "dialog", role: "alertdialog", "aria-modal": "true", "aria-labelledby": "outbox-delete-title", "data-testid": "outbox-delete-dialog",
                    onclick: move |e| e.stop_propagation(),
                    h2 { id: "outbox-delete-title", {tr("Xóa mục này khỏi máy?")} }
                    p { {tr("Bản lưu trên máy này sẽ bị xóa và không khôi phục được. Thư đã gửi và thư trên máy chủ không bị ảnh hưởng.")} }
                    div { class: "dialog-actions",
                        button { class: "btn", r#type: "button", "data-testid": "outbox-delete-cancel",
                            onmounted: move |e| async move { let _ = e.set_focus(true).await; },
                            onclick: move |_| pending_delete.set(None), {tr("Giữ lại")} }
                        button { class: "btn btn-danger", r#type: "button", "data-testid": "outbox-delete-confirm",
                            onclick: move |_| {
                                let id = id.clone();
                                pending_delete.set(None);
                                spawn(async move {
                                    if !outbox::discard(app, id).await {
                                        app.notify(tr("Chưa xóa được. Thư đang gửi không thể xóa."), false);
                                    }
                                });
                            },
                            {tr("Xóa")} }
                    }
                }
            }
        }
    }
}
