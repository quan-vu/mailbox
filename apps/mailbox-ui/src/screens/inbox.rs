use crate::components::body::MessageBody;
use crate::components::common::{Avatar, EmptyState};
use crate::components::icon::{Icon, pick};
use crate::components::shell::account_icon;
use crate::i18n::{bi, tr, when};
use crate::state::compose::{Draft, ReplyKind, format_size};
use crate::state::mail::{Folder, ThreadRow};
use crate::state::{AppState, Screen};
use dioxus::prelude::*;
use mailbox_contracts::{AttachmentSummary, MessageSummary};

fn file_kind(a: &AttachmentSummary) -> (&'static str, &'static str) {
    if a.media_type.contains("pdf") {
        ("PDF", "ft-pdf")
    } else if a.media_type.contains("sheet") {
        ("XLS", "ft-xls")
    } else {
        ("FILE", "ft-generic")
    }
}

#[component]
pub fn AttachmentCard(attachment: AttachmentSummary) -> Element {
    let (kind, class) = file_kind(&attachment);
    let size = format_size(attachment.size_bytes);
    rsx! {
        div { class: "attachment", "data-testid": "attachment-{attachment.id}",
            span { class: "file-type {class}", "aria-hidden": "true", "{kind}" }
            span { class: "attachment-text",
                strong { title: "{attachment.filename}", "{attachment.filename}" }
                small { "{size}" }
            }
            button { class: "icon-btn", r#type: "button", disabled: true, "aria-label": bi(format!("Tải xuống {} (bị tắt trong bản demo)", attachment.filename), format!("Download {} (disabled in the demo)", attachment.filename)),
                Icon { name: "download" }
            }
        }
    }
}

#[component]
pub fn InboxScreen() -> Element {
    let app = use_context::<AppState>();
    let mut filter = app.filter;
    let selected = app.selected;
    let mut search_open = app.mobile_search;
    let f = filter.read().clone();
    let mail = app.mail.read();
    let rows = crate::state::search::list_rows(app, &f, &mail);
    let real = app.is_real();
    // Real mode: a query is a local search in the store (MB-200), driven by this effect.
    use_effect(move || crate::state::search::drive_search(app));
    let searching = real && crate::state::search::searching(&f);
    let view_key = crate::state::folders::RealState::get().map(|r| format!("{:?}", r.view.read()));
    use_effect(use_reactive!(|view_key| {
        let _ = view_key;
        crate::state::search::reset_older();
    }));
    let unread = if real {
        rows.iter().filter(|r| r.unread).count()
    } else {
        mail.unread_count(f.account.as_deref())
    };
    let accounts = mail.accounts.clone();
    let title = match (&f.account, f.folder) {
        (Some(id), Folder::Inbox) => mail.account(id).map(|a| a.name.clone()).unwrap_or_default(),
        _ => f.folder.label().to_string(),
    };
    let title = crate::state::folders::view_title(app, &f).unwrap_or(title);
    let scope = match &f.account {
        Some(id) => mail
            .account(id)
            .map(|a| a.address.clone())
            .unwrap_or_default(),
        None => tr("Tất cả tài khoản").to_string(),
    };
    let sel_id = selected.read().clone();
    let current = sel_id
        .as_ref()
        .and_then(|id| {
            rows.iter().find(|r| {
                &r.head.id == id || mail.thread(r.head.thread_key()).iter().any(|m| &m.id == id)
            })
        })
        .map(|r| {
            let id = sel_id.as_deref().unwrap_or_default();
            // A search hit may lie outside the loaded list: the row carries the message itself.
            let message = mail
                .message(id)
                .cloned()
                .or_else(|| (r.head.id == id).then(|| r.head.clone()));
            (r.clone(), message)
        });
    drop(mail);
    let reader_open = *app.reader_open.read();
    let account_value = f.account.clone().unwrap_or_else(|| "all".into());
    let narrowing = f.has_narrowing();
    let (empty_title, empty_detail) = match f.folder {
        _ if real && !narrowing => (
            tr("Chưa có thư thật để hiển thị").to_string(),
            tr("Chế độ thật không dùng dữ liệu minh họa. Thêm tài khoản để đồng bộ hộp thư đến, hoặc bấm nút làm mới để tải thư mới.").to_string(),
        ),
        Folder::Sent | Folder::Drafts | Folder::Scheduled => (
            bi(format!("Chưa có thư trong \"{}\"", f.folder.label()), format!("No messages in \"{}\"", f.folder.label())),
            tr("Bản demo chưa có dữ liệu cho thư mục này. Thư thật sẽ đồng bộ từ máy chủ sau khi kết nối.").to_string(),
        ),
        _ if searching => (
            tr("Không tìm thấy thư phù hợp").to_string(),
            tr("Không có thư nào trong bộ nhớ đệm khớp với từ khóa và bộ lọc. Thử phạm vi rộng hơn, từ khóa khác, hoặc tải thêm thư cũ.").to_string(),
        ),
        _ if narrowing => (
            tr("Không tìm thấy thư phù hợp").to_string(),
            tr("Thử từ khóa khác hoặc bỏ bớt bộ lọc.").to_string(),
        ),
        _ => (tr("Không còn thư nào ở đây").to_string(), tr("Thư mới của các tài khoản sẽ xuất hiện tại đây.").to_string()),
    };

    rsx! {
        div { class: "inbox-layout", "data-testid": "screen-inbox", "data-reader-open": if reader_open { "true" } else { "false" },
            section { class: "list-pane", "aria-label": tr("Danh sách thư"), "data-testid": "message-list",
                div { class: "pane-header",
                    div { class: "grow",
                        h1 { "data-testid": "inbox-title", "{title}" }
                        p { class: "muted", "data-testid": "inbox-subtitle", {bi(format!("{scope} · {unread} thư chưa đọc"), format!("{scope} · {unread} unread"))} }
                    }
                    button { class: "icon-btn", r#type: "button", "data-testid": "inbox-refresh",
                        "aria-label": if real { tr("Đồng bộ thư mới")  } else { tr("Làm mới (demo — không có máy chủ)")  },
                        title: if real { tr("Tải thư mới từ máy chủ")  } else { tr("Bản demo không đồng bộ")  },
                        onclick: move |_| {
                            if app.is_real() { spawn(app.sync_now()); } else { app.notify(tr("Bản demo không kết nối máy chủ nên không có thư mới."), false); }
                        },
                        Icon { name: "refresh" }
                    }
                }
                if search_open() {
                    label { class: "mobile-search mobile-only",
                        Icon { name: "search" }
                        input { r#type: "search", "data-testid": "mobile-search-input", "aria-label": tr("Tìm kiếm thư"), placeholder: tr("Tìm kiếm thư"),
                            value: "{f.query}",
                            oninput: move |e| filter.write().query = e.value(),
                        }
                        button { class: "icon-btn", r#type: "button", "aria-label": tr("Đóng tìm kiếm"), onclick: move |_| { search_open.set(false); filter.write().query.clear(); },
                            Icon { name: "x" }
                        }
                    }
                }
                div { class: "chips", role: "toolbar", "aria-label": tr("Bộ lọc"),
                    button { class: if f.unread_only { "chip on" } else { "chip" }, r#type: "button", "data-testid": "filter-unread",
                        "aria-pressed": if f.unread_only { "true" } else { "false" },
                        onclick: move |_| { let v = !filter.read().unread_only; filter.write().unread_only = v; },
                        span { class: "dot dot-blue" } {tr("Chưa đọc")}
                    }
                    button { class: if f.attachments_only { "chip on" } else { "chip" }, r#type: "button", "data-testid": "filter-attachments",
                        "aria-pressed": if f.attachments_only { "true" } else { "false" },
                        onclick: move |_| { let v = !filter.read().attachments_only; filter.write().attachments_only = v; },
                        Icon { name: "clip", size: 15 } {tr("Có tệp đính kèm")}
                    }
                    button { class: if f.today_only { "chip on" } else { "chip" }, r#type: "button", "data-testid": "filter-today",
                        "aria-pressed": if f.today_only { "true" } else { "false" },
                        onclick: move |_| { let v = !filter.read().today_only; filter.write().today_only = v; },
                        Icon { name: "calendar", size: 15 } {tr("Hôm nay")}
                    }
                    label { class: "chip chip-select desktop-only",
                        span { class: "sr-only", {tr("Lọc theo tài khoản")} }
                        select { "data-testid": "filter-account", value: "{account_value}",
                            onchange: move |e| { let v = e.value(); filter.write().account = if v == "all" { None } else { Some(v) }; },
                            option { value: "all", selected: account_value == "all", {tr("Tất cả tài khoản")} }
                            for a in accounts.iter() {
                                option { key: "{a.id}", value: "{a.id}", selected: account_value == a.id, "{a.name}" }
                            }
                        }
                    }
                }
                if searching {
                    crate::components::search::SearchBar {}
                }
                div { class: "message-list", "data-search": if searching { "true" } else { "false" },
                    if rows.is_empty() && searching && crate::state::search::search_pending(&f) {
                        div { class: "list-footer muted small", role: "status", "data-testid": "search-pending", {tr("Đang tìm trong thư đã lưu…")} }
                    } else if rows.is_empty() {
                        EmptyState {
                            icon: pick(narrowing, "search", "inbox"),
                            title: empty_title,
                            detail: empty_detail,
                            clear: if narrowing { Some(EventHandler::new(move |_| filter.write().clear_narrowing())) } else { None },
                        }
                    }
                    if searching {
                        for row in rows.iter() {
                            crate::components::search::SearchRow { key: "{row.head.id}", row: row.clone(), current: current.as_ref().is_some_and(|(r, _)| r.head.id == row.head.id), query: f.query.clone() }
                        }
                    } else {
                        for row in rows.iter() {
                            MessageRow { key: "{row.head.id}", row: row.clone(), current: current.as_ref().is_some_and(|(r, _)| r.head.id == row.head.id) }
                        }
                    }
                    if real && !searching && f.folder != Folder::Starred && !rows.is_empty() {
                        crate::components::search::LoadOlder {}
                    }
                }
            }
            section { class: "reader-pane", "aria-label": tr("Nội dung thư"), "data-testid": "reader",
                if let Some((row, Some(message))) = current {
                    Reader { row, message }
                } else {
                    div { class: "reader-empty",
                        EmptyState { icon: "mail", title: tr("Chọn một thư để đọc").to_string(), detail: tr("Nội dung thư sẽ hiển thị tại đây.").to_string(), testid: "reader-empty" }
                    }
                }
            }
        }
    }
}

#[component]
fn MessageRow(row: ThreadRow, current: bool) -> Element {
    let app = use_context::<AppState>();
    let id = row.head.id.clone();
    let star_id = row.head.id.clone();
    let was_unread = row.unread;
    let head = row.head.clone();
    let account = app.mail.read().account(&head.account_id).cloned();
    let (acc_name, acc_color) = account.map(|a| (a.name, a.color)).unwrap_or_default();
    let class = match (current, row.unread) {
        (true, _) => "row selected",
        (false, true) => "row unread",
        _ => "row",
    };
    rsx! {
        div { class: "row-wrap",
            button {
                class: "{class}",
                r#type: "button",
                "data-testid": "message-row-{head.id}",
                "data-row": "true",
                "data-unread": if row.unread { "true" } else { "false" },
                "data-attachments": if row.has_attachments { "true" } else { "false" },
                "data-thread-count": "{row.count}",
                "data-op": crate::state::folders::row_op_key(&head.id),
                "aria-current": if current { "true" } else { "false" },
                onclick: move |_| { app.open_message(&id); crate::state::folders::opened(app, &id, was_unread); },
                span { class: if row.unread { "unread-dot" } else { "unread-dot hidden" }, "aria-hidden": "true" }
                Avatar { name: head.from_name.clone() }
                span { class: "row-main",
                    span { class: "row-top",
                        span { class: "sender", "{head.from_name}" }
                        if row.has_attachments { span { class: "muted", title: tr("Có tệp đính kèm"), Icon { name: "clip", size: 15 } } }
                        time { class: "muted small", title: "{when(&head.received_at_full)}", "{when(&head.received_at)}" }
                    }
                    span { class: "subject",
                        "{head.subject}"
                        if row.count > 1 { span { class: "thread-count", "{row.count}" } }
                    }
                    span { class: "preview", "{head.preview}" }
                    span { class: "row-account", "data-testid": "message-row-account", "data-account": "{head.account_id}",
                        span { class: "dot", style: "background:{acc_color}" }
                        "{acc_name}"
                    }
                    if row.unread { span { class: "sr-only", {tr("Chưa đọc")} } }
                }
            }
            button {
                class: if row.starred { "row-star on" } else { "row-star" },
                r#type: "button",
                "data-testid": "message-row-star-{star_id}",
                "aria-pressed": if row.starred { "true" } else { "false" },
                "aria-label": if row.starred { tr("Bỏ gắn sao")  } else { tr("Gắn sao")  },
                onclick: move |_| crate::state::folders::toggle_star(app, &star_id),
                Icon { name: "star", size: 18 }
            }
            crate::components::folders::RowOpBadge { id: head.id.clone() }
        }
    }
}

#[component]
fn Reader(row: ThreadRow, message: MessageSummary) -> Element {
    let app = use_context::<AppState>();
    let mut reader_open = app.reader_open;
    let key = message.thread_key().to_string();
    let account = app.mail.read().account(&message.account_id).cloned();
    let acc_name = account.as_ref().map(|a| a.name.clone()).unwrap_or_default();
    let (k1, k2, k3) = (key.clone(), key.clone(), key.clone());
    let star_id = message.id.clone();
    let open_id = message.id.clone();
    let (m_reply, m_all, m_fwd) = (message.clone(), message.clone(), message.clone());
    let was_unread = row.unread;
    let total: u64 = message.attachments.iter().map(|a| a.size_bytes).sum();
    let total_size = format_size(total);
    rsx! {
        div { class: "reader",
            div { class: "reader-toolbar", role: "toolbar", "aria-label": tr("Thao tác với thư"),
                button { class: "icon-btn mobile-only", r#type: "button", "data-testid": "reader-back", "aria-label": tr("Quay lại danh sách"),
                    onclick: move |_| reader_open.set(false),
                    Icon { name: "arrow-left", size: 20 }
                }
                button { class: "text-btn", r#type: "button", "data-testid": "reader-archive",
                    onclick: move |_| crate::state::folders::archive(app, &k1, true),
                    Icon { name: "archive" } span { class: "label", {tr("Lưu trữ")} }
                }
                button { class: "text-btn", r#type: "button", "data-testid": "reader-delete",
                    onclick: move |_| crate::state::folders::trash(app, &k2, true),
                    Icon { name: "trash" } span { class: "label", {tr("Xóa")} }
                }
                button { class: "text-btn", r#type: "button", "data-testid": "reader-toggle-unread",
                    onclick: move |_| crate::state::folders::set_unread(app, &k3, !was_unread),
                    Icon { name: "mail" } span { class: "label", if row.unread { {tr("Đánh dấu đã đọc")} } else { {tr("Đánh dấu chưa đọc")} } }
                }
                button { class: "text-btn", r#type: "button", "data-testid": "reader-star",
                    "aria-pressed": if message.starred { "true" } else { "false" },
                    onclick: move |_| crate::state::folders::toggle_star(app, &star_id),
                    Icon { name: "star", class: pick(message.starred, "starred", "") }
                    span { class: "label", if message.starred { {tr("Bỏ gắn sao")} } else { {tr("Gắn sao")} } }
                }
                crate::components::folders::MoveMenu { message: message.clone() }
            }
            h2 { class: "reader-subject", "data-testid": "reader-subject", "{message.subject}" }
            div { class: "tag-row",
                span { class: "tag tag-blue", "data-testid": "reader-account-chip", "data-account": "{message.account_id}",
                    Icon { name: account_icon(&message.account_id), size: 15 } "{acc_name}"
                }
                for label in message.labels.iter() {
                    span { key: "{label}", class: "tag tag-purple", Icon { name: "tag", size: 15 } "{label}" }
                }
            }
            article { class: "message-card",
                div { class: "sender-line",
                    Avatar { name: message.from_name.clone(), size: "lg" }
                    div { class: "grow",
                        strong { "{message.from_name}" }
                        small { class: "muted", "{message.from_address}" }
                    }
                    time { class: "muted small", title: bi(format!("{} (minh họa)", message.received_at_full), format!("{} (sample)", when(&message.received_at_full))), "{when(&message.received_at_full)}" }
                }
                p { class: "muted small recipients", {bi(format!("Đến: {} · Nhận qua tài khoản ", message.to_line), format!("To: {} · Received via account ", message.to_line))} strong { "{acc_name}" } }
                MessageBody { message: message.clone() }
                if !message.attachments.is_empty() {
                    div { class: "attachments-head",
                        Icon { name: "clip" }
                        strong { {bi(format!("{} tệp đính kèm", message.attachments.len()), format!("{} attachment{}", message.attachments.len(), crate::i18n::s(message.attachments.len())))} }
                        span { class: "muted", "({total_size})" }
                        span { class: "muted small push", {tr("Tải xuống bị tắt trong bản demo")} }
                    }
                    div { class: "attachments",
                        for a in message.attachments.iter() {
                            AttachmentCard { key: "{a.id}", attachment: a.clone() }
                        }
                    }
                }
            }
            div { class: "reader-actions",
                button { class: "btn btn-soft", r#type: "button", "data-testid": "reader-reply",
                    onclick: move |_| app.start_compose(Some(Draft::reply_to(&m_reply, ReplyKind::Reply))),
                    Icon { name: "reply" } {tr("Trả lời")}
                }
                button { class: "btn", r#type: "button", "data-testid": "reader-reply-all",
                    onclick: move |_| app.start_compose(Some(Draft::reply_to(&m_all, ReplyKind::ReplyAll))),
                    Icon { name: "reply-all" } {tr("Trả lời tất cả")}
                }
                button { class: "btn", r#type: "button", "data-testid": "reader-forward",
                    onclick: move |_| app.start_compose(Some(Draft::reply_to(&m_fwd, ReplyKind::Forward))),
                    Icon { name: "forward" } {tr("Chuyển tiếp")}
                }
                button { class: "link-btn push", r#type: "button", "data-testid": "reader-open-thread",
                    onclick: move |_| app.go(Screen::Thread(open_id.clone())),
                    {bi(format!("Xem cả hội thoại ({}) →", row.count), format!("View the whole conversation ({}) →", row.count))}
                }
            }
        }
    }
}
