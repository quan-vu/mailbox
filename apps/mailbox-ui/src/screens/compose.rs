//! Composer (MB-150). Demo mode keeps the "nothing is sent" dialog. Real mode on Desktop saves
//! the draft locally (truthful autosave indicator) and drives the outbox: sending, sent,
//! failed with reason, and "may have been sent", which is never a one-click retry.
use crate::components::common::Avatar;
use crate::components::icon::Icon;
use crate::components::shell::account_icon;
use crate::i18n::{bi, tr};
use crate::state::compose::{
    ATTACHMENT_LIMIT_BYTES, DraftAttachment, RecipientField, SendAction, TEMPLATES, format_size,
    send_action,
};
use crate::state::outbox::{self, CAN_SEND, OUTBOX, SendPhase, fail_message, sent_copy_message};
use crate::state::{AppState, Confirm, ConfirmAction, Screen, demo};
use dioxus::prelude::*;
use mailbox_core::outbox::{FailReason, SendIntent};

static COMPOSE_CSS: Asset = asset!("/assets/compose.css");

#[component]
pub fn ComposeScreen() -> Element {
    let app = use_context::<AppState>();
    crate::state::prefs::use_signature_sync(app);
    let mut draft = app.draft;
    let mut confirm = app.confirm;
    let mut send_dialog = app.send_dialog;
    let mut show_ccbcc = use_signal(|| false);
    let mut tried_send = use_signal(|| false);
    let mut discard_open = use_signal(|| false);
    let mut resend_open = use_signal(|| false);
    let mut generation = use_signal(|| 0u64);
    let real = app.is_real();
    let user_templates = crate::state::prefs::USER_TEMPLATES.read().clone();
    let can_send = real && CAN_SEND;
    let d = draft.read().clone();
    let ui = OUTBOX.read().clone();
    let phase = if real && !d.id.is_empty() && ui.draft_id == d.id {
        ui.phase.clone()
    } else {
        SendPhase::Idle
    };
    let locked = phase.locks_editing();
    let accounts = app.mail.read().accounts.clone();
    let detached = app.detached.read().clone();
    let identities: Vec<_> = accounts
        .into_iter()
        .filter(|a| !detached.contains(&a.id))
        .collect();
    let blockers = if real {
        d.real_send_blockers()
    } else {
        d.send_blockers()
    };
    let any_invalid = d.to.iter().chain(&d.cc).chain(&d.bcc).any(|r| !r.valid);
    let cc_visible = show_ccbcc() || !d.cc.is_empty() || !d.bcc.is_empty();
    let total = format_size(d.total_attachment_bytes());
    let limit = format_size(ATTACHMENT_LIMIT_BYTES);
    let reply_name = d
        .reply_account
        .as_ref()
        .and_then(|id| identities.iter().find(|a| &a.id == id))
        .map(|a| a.name.clone());

    // Reply-all: never copy the user's own addresses back to themselves (real accounts only).
    use_effect(move || {
        if app.is_real() {
            let own: Vec<String> = app
                .mail
                .peek()
                .accounts
                .iter()
                .map(|a| a.address.clone())
                .collect();
            let before = draft.peek().clone();
            if before.reply_source.is_some() && before.id.is_empty() {
                let mut after = before.clone();
                after.drop_own_addresses(&own);
                if after != before {
                    draft.set(after);
                }
            }
        }
    });
    // Autosave (Real mode, Desktop): every change schedules a save after a pause in typing.
    use_effect(move || {
        let _subscribe = draft.read();
        if outbox::note_change(&app) {
            let mine = *generation.peek() + 1;
            generation.set(mine);
            spawn(outbox::autosave_after_pause(app, generation, mine));
        }
    });

    let mode_key = if real { "real" } else { "demo" };
    let phase_key = phase.key();
    let (reason_key, retryable, rejected) = match &phase {
        SendPhase::Failed {
            reason,
            retryable,
            rejected,
        } => (reason.key(), *retryable, rejected.clone()),
        _ => ("", false, Vec::new()),
    };
    let fail_text = match &phase {
        SendPhase::Failed { reason, .. } => fail_message(*reason),
        _ => "",
    };
    let auth_failed = matches!(
        &phase,
        SendPhase::Failed {
            reason: FailReason::AuthFailed,
            ..
        }
    );
    let (copy_key, copy_text) = match &phase {
        SendPhase::Sent { copy } => (copy.key(), sent_copy_message(*copy)),
        _ => ("", ""),
    };
    let checked = match &phase {
        SendPhase::Unknown { checked } => *checked,
        _ => None,
    };
    let busy = ui.busy;
    let save_key = ui.save.key();
    let save_label = ui.save.label();
    let draft_id = d.id.clone();
    let send_label = if real {
        tr("Gửi")
    } else {
        tr("Gửi (demo)")
    };
    let format_note = if real {
        tr("Văn bản thuần")
    } else {
        tr("Văn bản thuần trong bản demo")
    };

    rsx! {
        document::Stylesheet { href: COMPOSE_CSS }
        div { class: "compose-layout", "data-testid": "screen-compose", "data-mode": mode_key,
            section { class: "compose-main", "aria-label": tr("Soạn thư mới"),
                div { class: "compose-head",
                    button { class: "icon-btn", r#type: "button", "data-testid": "compose-close", "aria-label": tr("Đóng soạn thư"),
                        onclick: move |_| {
                            if app.is_real() {
                                // The draft is kept: closing saves it, it does not discard it.
                                spawn(async move {
                                    let sent = matches!(outbox::phase_for(&app.draft.peek().id), SendPhase::Sent { .. });
                                    if sent {
                                        outbox::new_message(app);
                                    } else if !app.draft.peek().is_empty() {
                                        outbox::save_now(app).await;
                                    }
                                    app.go(Screen::Inbox);
                                });
                            } else if draft.read().is_empty() {
                                app.go(Screen::Inbox);
                            } else {
                                confirm.set(Some(Confirm {
                                    title: tr("Bỏ bản nháp này?").into(),
                                    body: tr("Bản nháp chỉ lưu trong phiên demo. Nếu bỏ, nội dung sẽ mất.").into(),
                                    ok_label: tr("Bỏ bản nháp"),
                                    danger: true,
                                    action: ConfirmAction::DiscardDraft,
                                }));
                            }
                        },
                        Icon { name: "arrow-left", size: 22 }
                    }
                    h1 { {tr("Soạn thư mới")} }
                    if real && CAN_SEND {
                        span { class: "draft-status push", role: "status", "aria-live": "polite", "data-testid": "compose-draft-status", "data-state": save_key,
                            Icon { name: "info", size: 15 }
                            "{save_label}"
                        }
                    } else if real {
                        span { class: "draft-status push", role: "status", "data-testid": "compose-draft-status", "data-state": "not-saved",
                            Icon { name: "info", size: 15 }
                            {tr("Bản Web chưa lưu bản nháp và chưa gửi được thư")}
                        }
                    } else {
                        span { class: "draft-status push", role: "status", "data-testid": "compose-draft-status", "data-state": "local-demo",
                            Icon { name: "info", size: 15 }
                            {tr("Bản nháp cục bộ (demo) — mất khi tải lại")}
                        }
                    }
                }
                if phase != SendPhase::Idle {
                    div { class: "send-state send-{phase_key}", role: "alert", "data-testid": "compose-send-state",
                        "data-state": phase_key, "data-reason": reason_key, "data-sent-copy": copy_key,
                        match &phase {
                            SendPhase::Sending => rsx! {
                                Icon { name: "refresh" }
                                div { class: "grow",
                                    strong { {tr("Đang gửi thư…")} }
                                    p { {tr("Giữ ứng dụng mở cho tới khi có kết quả. Nội dung thư đang được khóa.")} }
                                }
                            },
                            SendPhase::Sent { .. } => rsx! {
                                Icon { name: "check" }
                                div { class: "grow",
                                    strong { {tr("Đã gửi thư")} }
                                    p { {tr("Máy chủ gửi thư đã nhận thư này. ")} "{copy_text}" }
                                    div { class: "send-actions",
                                        button { class: "btn btn-primary btn-sm", r#type: "button", "data-testid": "compose-new-message",
                                            onclick: move |_| outbox::new_message(app), {tr("Soạn thư mới")} }
                                        button { class: "btn btn-sm", r#type: "button", "data-testid": "compose-back-inbox",
                                            onclick: move |_| { outbox::new_message(app); app.go(Screen::Inbox); }, {tr("Về hộp thư")} }
                                    }
                                }
                            },
                            SendPhase::Failed { .. } => rsx! {
                                Icon { name: "alert" }
                                div { class: "grow",
                                    strong { {tr("Chưa gửi được thư")} }
                                    p { "{fail_text}" }
                                    if !rejected.is_empty() {
                                        ul { class: "rejected-list", "aria-label": tr("Địa chỉ bị máy chủ từ chối"),
                                            for address in rejected.iter() {
                                                li { key: "{address}", "data-testid": "compose-rejected-recipient", "data-address": "{address}", "{address}" }
                                            }
                                        }
                                    }
                                    div { class: "send-actions",
                                        if retryable {
                                            button { class: "btn btn-primary btn-sm", r#type: "button", "data-testid": "compose-send-retry",
                                                onclick: move |_| { spawn(outbox::send(app, SendIntent::Retry)); }, {tr("Thử lại")} }
                                        }
                                        if auth_failed {
                                            button { class: "btn btn-sm", r#type: "button", "data-testid": "compose-smtp-password-open",
                                                onclick: move |_| OUTBOX.write().ask_smtp_password = true, {tr("Nhập mật khẩu gửi thư")} }
                                        }
                                        button { class: "btn btn-sm", r#type: "button", "data-testid": "compose-send-edit",
                                            onclick: move |_| { spawn(outbox::edit_again(app)); }, {tr("Sửa thư")} }
                                    }
                                }
                            },
                            SendPhase::Unknown { .. } => rsx! {
                                Icon { name: "alert" }
                                div { class: "grow",
                                    strong { {tr("Thư có thể đã được gửi")} }
                                    p { {tr("Kết nối bị ngắt sau khi thư được chuyển cho máy chủ, nên MailBox không biết thư đã đi hay chưa. MailBox sẽ không tự gửi lại. Hãy kiểm tra thư mục Đã gửi hoặc hỏi người nhận trước khi quyết định.")} }
                                    if let Some(found) = checked {
                                        p { class: "check-result", "data-testid": "compose-unknown-check-result", "data-found": if found { "true" } else { "false" },
                                            {tr("Chưa thấy thư này trong thư mục Đã gửi trên máy chủ. Điều đó không chứng minh được là thư chưa gửi: nhiều máy chủ không tự lưu thư đã gửi.")}
                                        }
                                    }
                                    div { class: "send-actions",
                                        button { class: "btn btn-primary btn-sm", r#type: "button", "data-testid": "compose-unknown-check", disabled: busy,
                                            onclick: move |_| { spawn(async move {
                                                outbox::check_sent_folder(app).await;
                                                if matches!(outbox::phase_for(&app.draft.peek().id), SendPhase::Unknown { checked: None }) {
                                                    app.notify(tr("Chưa kiểm tra được thư mục Đã gửi. Kiểm tra mạng rồi thử lại."), false);
                                                }
                                            }); },
                                            {tr("Kiểm tra thư mục Đã gửi")} }
                                        button { class: "btn btn-sm", r#type: "button", "data-testid": "compose-unknown-mark-sent",
                                            onclick: move |_| { spawn(outbox::mark_sent(app)); }, {tr("Thư đã tới — đánh dấu đã gửi")} }
                                        button { class: "btn btn-sm", r#type: "button", "data-testid": "compose-send-edit",
                                            onclick: move |_| { spawn(outbox::edit_again(app)); }, {tr("Sửa thư")} }
                                        button { class: "btn btn-sm btn-danger", r#type: "button", "data-testid": "compose-unknown-resend",
                                            onclick: move |_| resend_open.set(true), {tr("Vẫn gửi lại…")} }
                                    }
                                    p { class: "muted small", {tr("Nếu gửi lại, người nhận có thể nhận hai thư.")} }
                                }
                            },
                            SendPhase::Scheduled { at } => rsx! { crate::components::schedule::ScheduledState { at: *at } },
                            SendPhase::Idle => rsx! {},
                        }
                    }
                }
                fieldset { class: "compose-form", "data-testid": "compose-form", disabled: locked,
                    div { class: "form-row",
                        label { class: "form-label", r#for: "compose-from", {tr("Từ:")} }
                        div { class: "select-wrap",
                            Icon { name: account_icon(&d.from_account) }
                            select { id: "compose-from", "data-testid": "compose-from", value: "{d.from_account}",
                                onchange: move |e| draft.write().from_account = e.value(),
                                for a in identities.iter() {
                                    option { key: "{a.id}", value: "{a.id}", selected: a.id == d.from_account, "{a.name} – {a.address}" }
                                }
                            }
                        }
                        span { class: "muted small", {tr("Danh tính gửi luôn hiển thị.")} }
                    }
                    if d.identity_mismatch() {
                        div { class: "alert alert-warn", role: "alert", "data-testid": "compose-identity-warning",
                            Icon { name: "alert" }
                            span { {tr("Thư gốc được nhận qua tài khoản ")} strong { "{reply_name.clone().unwrap_or_default()}" } {tr(". Bạn đang trả lời bằng danh tính khác — người nhận sẽ thấy địa chỉ gửi khác.")} }
                        }
                    }
                    RecipientRow { field: RecipientField::To, label: tr("Đến:") }
                    if !cc_visible {
                        div { class: "form-row",
                            span { class: "form-label" }
                            button { class: "link-btn", r#type: "button", "data-testid": "compose-ccbcc-toggle", onclick: move |_| show_ccbcc.set(true), "+ Cc / Bcc" }
                        }
                    } else {
                        RecipientRow { field: RecipientField::Cc, label: "Cc:" }
                        RecipientRow { field: RecipientField::Bcc, label: "Bcc:" }
                        div { class: "form-row",
                            span { class: "form-label" }
                            span { class: "muted small", {tr("Người nhận Bcc được ẩn và không bị lộ khi Trả lời tất cả.")} }
                        }
                    }
                    if any_invalid {
                        div { class: "form-row",
                            span { class: "form-label" }
                            span { class: "field-error", role: "alert", "data-testid": "compose-recipient-error", {tr("Có địa chỉ email không hợp lệ (đánh dấu đỏ). Sửa hoặc xóa trước khi gửi.")} }
                        }
                    }
                    div { class: "form-row",
                        label { class: "form-label", r#for: "compose-subject", {tr("Chủ đề:")} }
                        input { id: "compose-subject", class: "input grow", "data-testid": "compose-subject", value: "{d.subject}", placeholder: tr("Tiêu đề email"),
                            oninput: move |e| draft.write().subject = e.value(),
                        }
                    }
                    div { class: "editor",
                        div { class: "editor-toolbar", role: "toolbar", "aria-label": tr("Định dạng (chưa hỗ trợ)"),
                            for (icon, label) in [("undo", tr("Hoàn tác")), ("redo", tr("Làm lại")), ("list", tr("Danh sách")), ("link", tr("Chèn liên kết"))] {
                                button { key: "{icon}", class: "icon-btn", r#type: "button", disabled: true, "aria-label": "{label}", Icon { name: icon } }
                            }
                            span { class: "muted small push", "{format_note}" }
                        }
                        textarea { class: "editor-body", "data-testid": "compose-body", "aria-label": tr("Nội dung email"), placeholder: tr("Viết email của bạn tại đây…"), value: "{d.body}",
                            oninput: move |e| draft.write().body = e.value(),
                        }
                    }
                    div { class: "attach-head",
                        Icon { name: "clip" }
                        strong { {bi(format!("Tệp đính kèm ({})", d.attachments.len()), format!("Attachments ({})", d.attachments.len()))} }
                        span { class: "muted small", {bi(format!("Tổng: {total} · giới hạn {limit}"), format!("Total: {total} · limit {limit}"))} }
                        if can_send {
                            // Desktop opens the system file dialog; only the path is kept, the file
                            // is read when the message is sent.
                            label { class: "dropzone push attach-picker",
                                Icon { name: "upload" }
                                {tr("Đính kèm tệp từ máy")}
                                input { class: "sr-only", r#type: "file", multiple: true, "data-testid": "compose-attach-input", "aria-label": tr("Chọn tệp đính kèm"),
                                    onchange: move |e| {
                                        let mut d = draft.write();
                                        for file in e.files() {
                                            let path = file.path().to_string_lossy().into_owned();
                                            if path.is_empty() || d.attachments.iter().any(|a| a.path.as_deref() == Some(path.as_str())) {
                                                continue;
                                            }
                                            d.attachments.push(DraftAttachment { name: file.name(), size: file.size(), path: Some(path), forwarded: None });
                                        }
                                    },
                                }
                            }
                        } else if !real {
                            button { class: "dropzone push", r#type: "button", "data-testid": "compose-add-attachment",
                                onclick: move |_| draft.write().attachments.push(DraftAttachment { name: "Tai-lieu-minh-hoa.pdf".into(), size: 1_250_000, path: None, forwarded: None }),
                                Icon { name: "upload" } {tr("Thêm tệp minh họa (không tải lên)")}
                            }
                        }
                    }
                    crate::components::forward::ForwardFilesNote {}
                    if !d.attachments.is_empty() {
                        div { class: "attachments",
                            for (i, a) in d.attachments.iter().enumerate() {
                                div { key: "{i}-{a.name}", class: "attachment", "data-testid": "compose-attachment", "data-local": if a.path.is_some() { "true" } else { "false" }, "data-forwarded": if a.forwarded.is_some() { "true" } else { "false" },
                                    span { class: "file-type ft-generic", "aria-hidden": "true", "FILE" }
                                    span { class: "attachment-text",
                                        strong { "{a.name}" }
                                        small { "{format_size(a.size)}" }
                                        if real && a.forwarded.is_some() {
                                            small { class: "muted", {tr("Chuyển tiếp từ thư gốc — tải lại từ máy chủ khi gửi")} }
                                        } else if real && a.path.is_none() {
                                            small { class: "field-error", {tr("Không có trên máy này — gỡ hoặc đính kèm lại")} }
                                        }
                                    }
                                    button { class: "icon-btn", r#type: "button", "aria-label": bi(format!("Gỡ {}", a.name), format!("Remove {}", a.name)), onclick: move |_| { draft.write().attachments.remove(i); },
                                        Icon { name: "x", size: 16 }
                                    }
                                }
                            }
                        }
                    }
                }
                if tried_send() && !blockers.is_empty() && !locked {
                    div { class: "alert alert-warn", role: "alert", "data-testid": "compose-send-blockers",
                        Icon { name: "alert" }
                        ul { for b in blockers.iter() { li { key: "{b}", "{b}" } } }
                    }
                }
                div { class: "compose-footer",
                    crate::components::schedule::ScheduleControl { locked }
                    if real {
                        if can_send && !locked && (!d.is_empty() || !draft_id.is_empty()) {
                            button { class: "btn", r#type: "button", "data-testid": "compose-discard",
                                onclick: move |_| discard_open.set(true), Icon { name: "trash" } {tr("Bỏ bản nháp")} }
                        }
                    } else {
                        span { class: "muted small", {tr("Chữ ký: Công ty (mặc định)")} }
                    }
                    button { class: "btn btn-primary push", r#type: "button", "data-testid": "compose-send", disabled: locked,
                        onclick: move |_| {
                            tried_send.set(true);
                            let real = app.is_real();
                            let blockers = if real { draft.read().real_send_blockers().len() } else { draft.read().send_blockers().len() };
                            match send_action(real, CAN_SEND, blockers) {
                                SendAction::ShowBlockers => {}
                                SendAction::DemoNotice => send_dialog.set(true),
                                SendAction::Submit => { spawn(outbox::send(app, SendIntent::Send)); }
                            }
                        },
                        Icon { name: "send" } "{send_label}"
                    }
                }
            }
            aside { class: "compose-side desktop-only", "aria-label": tr("Mẫu thư và liên hệ"),
                if real {
                    h2 { {tr("Mẫu thư của bạn")} }
                    p { class: "muted small", {tr("Điền {{name}} và {{email}} từ người nhận đầu tiên. Áp dụng mẫu sẽ hỏi trước khi ghi đè nội dung.")} }
                    if user_templates.is_empty() {
                        div { class: "empty-inline", "data-testid": "compose-templates-empty",
                            small { class: "muted", {tr("Chưa có mẫu nào. Tạo mẫu trong Cài đặt › Chữ ký.")} }
                            button { class: "btn btn-sm", r#type: "button", onclick: move |_| app.go(Screen::Settings(crate::state::SettingsTab::Signature)), {tr("Tạo mẫu")} }
                        }
                    }
                    for t in user_templates.iter() {
                        {
                            let id = t.id.clone();
                            let title = t.name.clone();
                            rsx! {
                                button { key: "{t.id}", class: "template", r#type: "button", "data-testid": "compose-template-{t.id}", disabled: locked,
                                    onclick: move |_| {
                                        let d = draft.read();
                                        let occupied = !d.subject.trim().is_empty() || !crate::state::compose::body_is_blank(&d.body);
                                        drop(d);
                                        if occupied {
                                            confirm.set(Some(Confirm {
                                                title: tr("Ghi đè nội dung?").into(),
                                                body: bi(format!("Áp dụng mẫu \"{title}\" sẽ thay chủ đề và nội dung hiện tại."), format!("Applying the template \"{title}\" will replace the current subject and content.")),
                                                ok_label: tr("Áp dụng mẫu"),
                                                danger: false,
                                                action: ConfirmAction::ApplyUserTemplate(id.clone()),
                                            }));
                                        } else {
                                            app.apply_user_template(&id);
                                        }
                                    },
                                    Icon { name: "file" }
                                    span { class: "grow", strong { "{t.name}" } small { class: "muted", "{t.subject}" } }
                                }
                            }
                        }
                    }
                } else {
                    h2 { {tr("Mẫu thư đề xuất")} }
                    p { class: "muted small", {tr("Áp dụng mẫu sẽ hỏi trước khi ghi đè nội dung.")} }
                    for t in TEMPLATES.iter() {
                        {
                            let key = t.key;
                            let title = tr(t.title);
                            rsx! {
                                button { key: "{key}", class: "template", r#type: "button", "data-testid": "compose-template-{key}", disabled: locked,
                                    onclick: move |_| {
                                        let d = draft.read();
                                        let occupied = !d.subject.trim().is_empty() || !d.body.trim().is_empty();
                                        drop(d);
                                        if occupied {
                                            confirm.set(Some(Confirm {
                                                title: tr("Ghi đè nội dung?").into(),
                                                body: bi(format!("Áp dụng mẫu \"{title}\" sẽ thay chủ đề và nội dung hiện tại."), format!("Applying the template \"{title}\" will replace the current subject and content.")),
                                                ok_label: tr("Áp dụng mẫu"),
                                                danger: false,
                                                action: ConfirmAction::ApplyTemplate(key),
                                            }));
                                        } else {
                                            app.apply_template(key);
                                        }
                                    },
                                    Icon { name: t.icon }
                                    span { class: "grow", strong { {tr(t.title)} } small { class: "muted", {tr(t.description)} } }
                                }
                            }
                        }
                    }
                }
                if !real {
                    h2 { {tr("Gợi ý người liên hệ")} }
                    p { class: "muted small", {tr("Từ dữ liệu minh họa.")} }
                    for c in demo::CONTACTS.iter() {
                        {
                            let address = c.address;
                            rsx! {
                                div { key: "{address}", class: "contact",
                                    Avatar { name: c.name.to_string() }
                                    span { class: "grow", strong { "{c.name}" } small { class: "muted", "{c.address}" } }
                                    button { class: "icon-btn bordered", r#type: "button", "aria-label": bi(format!("Thêm {} vào người nhận", c.name), format!("Add {} to recipients", c.name)),
                                        onclick: move |_| { draft.write().add_recipient(RecipientField::To, address); },
                                        Icon { name: "plus", size: 16 }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if send_dialog() {
            div { class: "overlay", onclick: move |_| send_dialog.set(false),
                div { class: "dialog", role: "dialog", "aria-modal": "true", "aria-labelledby": "send-title", "data-testid": "compose-send-dialog", "data-sent": "false",
                    onclick: move |e| e.stop_propagation(),
                    if real {
                        h2 { id: "send-title", {tr("Bản này chưa gửi được thư")} }
                        p { {tr("Gửi thư thật hiện chỉ có trên bản Desktop. Thư này ")} strong { {tr("sẽ không được gửi")} } {tr(" tới ai.")} }
                    } else {
                        h2 { id: "send-title", {tr("Bản demo không gửi thư")} }
                        p { {tr("Chế độ demo không kết nối SMTP. Thư này ")} strong { {tr("sẽ không được gửi")} } {tr(" tới ai. Bản nháp vẫn ở đây để bạn tiếp tục chỉnh.")} }
                    }
                    div { class: "dialog-actions",
                        button { class: "btn btn-primary", r#type: "button", "data-testid": "compose-send-dialog-ok",
                            onmounted: move |e| async move { let _ = e.set_focus(true).await; },
                            onclick: move |_| send_dialog.set(false), {tr("Đã hiểu")}
                        }
                    }
                }
            }
        }
        if discard_open() {
            div { class: "overlay", onclick: move |_| discard_open.set(false),
                div { class: "dialog", role: "alertdialog", "aria-modal": "true", "aria-labelledby": "discard-title", "data-testid": "compose-discard-dialog",
                    onclick: move |e| e.stop_propagation(),
                    h2 { id: "discard-title", {tr("Bỏ bản nháp này?")} }
                    p { {tr("Bản nháp sẽ bị xóa khỏi máy này và không khôi phục được. Thư trên máy chủ không bị ảnh hưởng.")} }
                    div { class: "dialog-actions",
                        button { class: "btn", r#type: "button", "data-testid": "compose-discard-cancel",
                            onmounted: move |e| async move { let _ = e.set_focus(true).await; },
                            onclick: move |_| discard_open.set(false), {tr("Giữ bản nháp")} }
                        button { class: "btn btn-danger", r#type: "button", "data-testid": "compose-discard-confirm",
                            onclick: move |_| {
                                discard_open.set(false);
                                spawn(async move {
                                    let id = app.draft.peek().id.clone();
                                    if outbox::discard(app, id).await {
                                        app.go(Screen::Inbox);
                                    } else {
                                        app.notify(tr("Chưa xóa được bản nháp. Thử lại."), false);
                                    }
                                });
                            },
                            {tr("Bỏ bản nháp")} }
                    }
                }
            }
        }
        if resend_open() {
            div { class: "overlay", onclick: move |_| resend_open.set(false),
                div { class: "dialog", role: "alertdialog", "aria-modal": "true", "aria-labelledby": "resend-title", "data-testid": "compose-resend-dialog",
                    onclick: move |e| e.stop_propagation(),
                    h2 { id: "resend-title", {tr("Gửi lại dù thư có thể đã đi?")} }
                    p { {tr("Lần gửi trước có thể đã thành công. Nếu gửi lại, người nhận có thể nhận hai thư giống nhau. Chỉ gửi lại khi bạn đã kiểm tra thư mục Đã gửi hoặc hỏi người nhận.")} }
                    div { class: "dialog-actions",
                        button { class: "btn", r#type: "button", "data-testid": "compose-resend-cancel",
                            onmounted: move |e| async move { let _ = e.set_focus(true).await; },
                            onclick: move |_| resend_open.set(false), {tr("Không gửi lại")} }
                        button { class: "btn btn-danger", r#type: "button", "data-testid": "compose-resend-confirm",
                            onclick: move |_| {
                                resend_open.set(false);
                                spawn(outbox::send(app, SendIntent::ResendAfterUnknown));
                            },
                            {tr("Tôi hiểu, gửi lại")} }
                    }
                }
            }
        }
        if real && ui.ask_smtp_password {
            SmtpPasswordDialog {}
        }
    }
}

/// Asked once, when the SMTP server refuses the account password. The value goes straight to
/// the keychain after the server accepted it; it is never kept in UI state after submit.
#[component]
fn SmtpPasswordDialog() -> Element {
    let app = use_context::<AppState>();
    let mut password = use_signal(String::new);
    let ui = OUTBOX.read().clone();
    let error = ui.smtp_password_error;
    let busy = ui.busy;
    rsx! {
        div { class: "overlay",
            div { class: "dialog", role: "dialog", "aria-modal": "true", "aria-labelledby": "smtp-pw-title", "data-testid": "compose-smtp-password-dialog",
                h2 { id: "smtp-pw-title", {tr("Mật khẩu gửi thư (SMTP)")} }
                p { {tr("Máy chủ gửi thư không chấp nhận mật khẩu của tài khoản. Nếu nhà cung cấp dùng mật khẩu riêng cho việc gửi thư (hoặc mật khẩu ứng dụng), nhập vào đây. MailBox kiểm tra với máy chủ qua kết nối mã hóa rồi lưu trong Keychain của hệ điều hành — không lưu trong cơ sở dữ liệu hay tệp cài đặt.")} }
                label { class: "form-label", r#for: "compose-smtp-password", {tr("Mật khẩu SMTP")} }
                input { id: "compose-smtp-password", class: "input", r#type: "password", autocomplete: "off", "data-testid": "compose-smtp-password", value: "{password}",
                    oninput: move |e| password.set(e.value()),
                }
                if let Some(message) = error {
                    span { class: "field-error", role: "alert", "data-testid": "compose-smtp-password-error", "{message}" }
                }
                div { class: "dialog-actions",
                    button { class: "btn", r#type: "button", "data-testid": "compose-smtp-password-cancel",
                        onclick: move |_| {
                            password.set(String::new());
                            let mut ui = OUTBOX.write();
                            ui.ask_smtp_password = false;
                            ui.smtp_password_error = None;
                        },
                        {tr("Hủy")} }
                    button { class: "btn btn-primary", r#type: "button", "data-testid": "compose-smtp-password-save", disabled: busy,
                        onclick: move |_| {
                            // Taken out of UI state before the request starts.
                            let value = std::mem::take(&mut *password.write());
                            spawn(outbox::save_smtp_password_and_retry(app, value));
                        },
                        {tr("Lưu và gửi lại")} }
                }
            }
        }
    }
}

#[component]
fn RecipientRow(field: RecipientField, label: &'static str) -> Element {
    let app = use_context::<AppState>();
    let mut draft = app.draft;
    let mut pending = use_signal(String::new);
    let list = match field {
        RecipientField::To => draft.read().to.clone(),
        RecipientField::Cc => draft.read().cc.clone(),
        RecipientField::Bcc => draft.read().bcc.clone(),
    };
    let key = field.key();
    let mut commit = move || {
        let value = pending.read().clone();
        if !value.trim().is_empty() {
            draft.write().add_recipient(field, &value);
        }
        pending.set(String::new());
    };
    rsx! {
        div { class: "form-row",
            label { class: "form-label", r#for: "compose-{key}-input", "{label}" }
            div { class: "recipient-box grow", "data-testid": "compose-{key}-field",
                for (i, r) in list.iter().enumerate() {
                    span { key: "{r.address}", class: if r.valid { "chip-recipient" } else { "chip-recipient invalid" },
                        "data-testid": "compose-chip", "data-address": "{r.address}", "data-valid": if r.valid { "true" } else { "false" },
                        title: if r.valid { "{r.address}" } else { tr("Địa chỉ không hợp lệ").to_string() },
                        if r.valid { Avatar { name: r.display.clone(), size: "xs" } } else { Icon { name: "alert", size: 14 } }
                        "{r.display}"
                        button { class: "chip-x", r#type: "button", "data-testid": "compose-chip-remove", "aria-label": bi(format!("Xóa {}", r.address), format!("Remove {}", r.address)),
                            onclick: move |_| draft.write().remove_recipient(field, i),
                            Icon { name: "x", size: 12 }
                        }
                    }
                }
                input { id: "compose-{key}-input", class: "chip-input", "data-testid": "compose-{key}-input", r#type: "email", autocomplete: "off",
                    "aria-label": bi(format!("Thêm người nhận ({label})"), format!("Add recipient ({label})")),
                    placeholder: if list.is_empty() { tr("Nhập email, nhấn Enter") } else { "" },
                    value: "{pending}",
                    oninput: move |e| {
                        let v = e.value();
                        if v.ends_with(',') || v.ends_with(';') { pending.set(v); commit(); } else { pending.set(v); }
                    },
                    onkeydown: move |e| {
                        if e.key() == Key::Enter { e.prevent_default(); commit(); }
                    },
                    onblur: move |_| commit(),
                }
            }
        }
    }
}
