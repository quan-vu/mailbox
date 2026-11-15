use super::inbox::AttachmentCard;
use crate::components::body::MessageBody;
use crate::components::common::{Avatar, EmptyState};
use crate::components::icon::{Icon, pick};
use crate::components::shell::account_icon;
use crate::i18n::{bi, tr, when};
use crate::state::compose::{Draft, ReplyKind};
use crate::state::{AppState, Screen};
use dioxus::prelude::*;
use std::collections::BTreeSet;

#[component]
pub fn ThreadScreen(thread: String) -> Element {
    let app = use_context::<AppState>();
    let mail = app.mail;
    // Real mode (Desktop): the stored conversation across folders, oldest first (MB-201).
    let stored = crate::state::search::use_conversation(app, thread.clone());
    let messages = match stored {
        Some(list) if !list.is_empty() => list,
        _ => mail.read().thread(&thread),
    };
    // First message starts expanded; the rest are collapsed summaries.
    // The parent keys this component by thread id, so the hook re-seeds per conversation.
    let first_id = messages.first().map(|m| m.id.clone()).unwrap_or_default();
    let mut expanded = use_signal(move || BTreeSet::from([first_id]));
    let Some(first) = messages.first().cloned() else {
        return rsx! {
            section { class: "thread-screen", "data-testid": "screen-thread",
                EmptyState { icon: "mail", title: tr("Không tìm thấy hội thoại").to_string(), detail: tr("Hội thoại có thể đã bị xóa.").to_string() }
            }
        };
    };
    let last = messages.last().cloned().unwrap_or_else(|| first.clone());
    let account = mail.read().account(&first.account_id).cloned();
    let acc_name = account.map(|a| a.name).unwrap_or_default();
    let count = messages.len();
    let starred = messages.iter().any(|m| m.starred);
    let span = format!(
        "{} – {}",
        when(&first.received_at_full),
        when(&last.received_at_full)
    );
    let span_label = if app.is_real() {
        span.clone()
    } else {
        bi(format!("{span} (minh họa)"), format!("{span} (sample)"))
    };
    let all_expanded = expanded.read().len() == count;
    let ids: Vec<String> = messages.iter().map(|m| m.id.clone()).collect();
    let star_id = first.id.clone();
    let (k1, k2) = (thread.clone(), thread.clone());
    let reply_to = last.clone();

    rsx! {
        section { class: "thread-screen", "data-testid": "screen-thread", "aria-label": tr("Hội thoại"),
            div { class: "reader-toolbar", role: "toolbar", "aria-label": tr("Thao tác với hội thoại"),
                button { class: "icon-btn", r#type: "button", "data-testid": "thread-back", "aria-label": tr("Quay lại hộp thư"), onclick: move |_| app.go(Screen::Inbox),
                    Icon { name: "chevron-left", size: 20 }
                }
                span { class: "toolbar-sep", "aria-hidden": "true" }
                button { class: "text-btn", r#type: "button",
                    onclick: move |_| { crate::state::folders::archive(app, &k1, false); app.go(Screen::Inbox); },
                    Icon { name: "archive" } span { class: "label", {tr("Lưu trữ")} }
                }
                button { class: "text-btn", r#type: "button",
                    onclick: move |_| { crate::state::folders::trash(app, &k2, false); app.go(Screen::Inbox); },
                    Icon { name: "trash" } span { class: "label", {tr("Xóa")} }
                }
                button { class: "text-btn", r#type: "button", "aria-pressed": if starred { "true" } else { "false" },
                    onclick: move |_| crate::state::folders::toggle_star(app, &star_id),
                    Icon { name: "star", class: pick(starred, "starred", "") } span { class: "label", if starred { {tr("Bỏ gắn sao")} } else { {tr("Gắn sao")} } }
                }
                button { class: "text-btn", r#type: "button", disabled: true, title: tr("Tạm ẩn: sau MVP"),
                    Icon { name: "clock" } span { class: "label", {tr("Tạm ẩn")} }
                }
            }
            div { class: "thread-heading",
                h1 { "data-testid": "thread-subject", "{first.subject}" }
                span { class: "tag tag-blue", "data-testid": "thread-account", "data-account": "{first.account_id}",
                    title: tr("Hội thoại thuộc một tài khoản; MailBox không di chuyển thư chéo tài khoản"),
                    Icon { name: account_icon(&first.account_id), size: 15 } {bi(format!("Tài khoản: {acc_name}"), format!("Account: {acc_name}"))}
                }
                for label in first.labels.iter() {
                    span { key: "{label}", class: "tag tag-purple", "{label}" }
                }
            }
            div { class: "thread-meta",
                button { class: "icon-btn small-btn", r#type: "button",
                    "aria-label": if all_expanded { tr("Thu gọn tất cả")  } else { tr("Mở rộng tất cả")  },
                    onclick: move |_| {
                        if all_expanded { expanded.set(BTreeSet::new()) } else { expanded.set(ids.iter().cloned().collect()) }
                    },
                    Icon { name: pick(all_expanded, "chevron-up", "chevron-down"), size: 16 }
                }
                span { class: "muted", "data-testid": "thread-count", "data-count": "{count}", {bi(format!("{count} thư trong hội thoại"), format!("{count} message{} in the conversation", crate::i18n::s(count)))} }
                span { class: "muted small push", "data-testid": "thread-span", "{span_label}" }
            }
            div { class: "thread-list",
                for m in messages.iter() {
                    {
                        let open = expanded.read().contains(&m.id);
                        let id = m.id.clone();
                        rsx! {
                            article { key: "{m.id}", class: if open { "thread-msg open" } else { "thread-msg" }, "data-testid": "thread-message-{m.id}", "data-account": "{m.account_id}",
                                button {
                                    class: "thread-msg-head",
                                    r#type: "button",
                                    "data-testid": "thread-toggle-{m.id}",
                                    "aria-expanded": if open { "true" } else { "false" },
                                    "aria-controls": "thread-body-{m.id}",
                                    onclick: move |_| {
                                        let mut e = expanded.write();
                                        if !e.remove(&id) { e.insert(id.clone()); }
                                    },
                                    Icon { name: pick(open, "chevron-up", "chevron-down"), size: 16 }
                                    Avatar { name: m.from_name.clone() }
                                    span { class: "grow head-text",
                                        span { strong { "{m.from_name}" } " " span { class: "muted small", "{m.from_address}" } }
                                        if open {
                                            span { class: "muted small", {bi(format!("Đến: {}", m.to_line), format!("To: {}", m.to_line))} }
                                        } else {
                                            span { class: "muted small preview", "{m.preview}" }
                                        }
                                    }
                                    time { class: "muted small", "{when(&m.received_at_full)}" }
                                }
                                if open {
                                    div { class: "thread-msg-body", id: "thread-body-{m.id}", "data-testid": "thread-body-{m.id}",
                                        MessageBody { message: m.clone() }
                                        if !m.attachments.is_empty() {
                                            div { class: "attachments",
                                                for a in m.attachments.iter() {
                                                    AttachmentCard { key: "{a.id}", attachment: a.clone() }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "reader-actions",
                button { class: "btn btn-soft", r#type: "button", "data-testid": "thread-reply",
                    onclick: move |_| app.start_compose(Some(Draft::reply_to(&reply_to, ReplyKind::Reply))),
                    Icon { name: "reply" } {tr("Trả lời")}
                }
                button { class: "btn", r#type: "button",
                    onclick: move |_| app.start_compose(Some(Draft::reply_to(&last, ReplyKind::ReplyAll))),
                    Icon { name: "reply-all" } {tr("Trả lời tất cả")}
                }
            }
        }
    }
}
