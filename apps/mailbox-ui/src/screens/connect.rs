use crate::adapters::connect::{
    CheckFailure, CheckOk, REAL_CHECK, SmtpOk, check_imap, check_smtp, host_from_detection,
};
use crate::components::autoconfig::AutoconfigResult;
use crate::components::icon::{Icon, pick};
use crate::components::oauth_mock::{OAuthMockButtons, OAuthMockPanel, OAuthProvider};
use crate::i18n::{bi, tr};
use crate::state::connect::{ConnectForm, Field, Provider, Scenario};
use crate::state::{AppState, Screen};
use dioxus::prelude::*;

#[component]
pub fn ConnectScreen() -> Element {
    let app = use_context::<AppState>();
    let mut form = use_signal(ConnectForm::default);
    let mut step = use_signal(|| 1u8);
    let mut attempted = use_signal(|| false);
    let mut scenario = use_signal(|| None::<Scenario>);
    // Desktop only. The password lives here just until the check starts, then it is taken out.
    let mut password = use_signal(String::new);
    let mut password_missing = use_signal(|| false);
    let mut checking = use_signal(|| false);
    let mut live = use_signal(|| None::<Result<CheckOk, CheckFailure>>);
    let mut live_smtp = use_signal(|| None::<Result<SmtpOk, CheckFailure>>);
    // Kept only between a successful check and "save"; cleared on back, retry, save and unmount.
    let mut held_secret = use_signal(|| None::<String>);
    let mut saving = use_signal(|| false);
    let mut oauth_mock = use_signal(|| None::<OAuthProvider>);
    let mut detection = use_signal(|| None::<(String, mailbox_autoconfig::Detection)>);
    let mut detecting = use_signal(|| false);
    let mut saved = use_signal(|| None::<Result<(usize, usize), CheckFailure>>);
    use_drop(move || held_secret.set(None));
    let f = form.read().clone();
    let errors = if attempted() { f.errors() } else { Vec::new() };
    let err = |field: Field| errors.iter().find(|(k, _)| *k == field).map(|(_, m)| *m);
    let current = step();

    rsx! {
        div { class: "connect-layout", "data-testid": "screen-connect",
            aside { class: "connect-intro desktop-only", "aria-label": tr("Giới thiệu"),
                div { class: "intro-art", "aria-hidden": "true",
                    span { class: "intro-ring" }
                    span { class: "intro-logo", Icon { name: "mail", size: 30 } }
                    span { class: "mono m1", "G" } span { class: "mono m2", "M" } span { class: "mono m3", "cP" } span { class: "mono m4", Icon { name: "mail" } }
                }
                h2 { {tr("Kết nối nhiều tài khoản email vào MailBox")} }
                p { class: "muted", {tr("Tập trung hộp thư có sẵn — Gmail, Microsoft 365, Zoho hay email doanh nghiệp — trong một nơi. Máy chủ gốc vẫn là nơi lưu thư.")} }
                div { class: "benefit", Icon { name: "zap" } span { strong { {tr("Tập trung & đồng bộ")} } small { class: "muted", {tr("Xem và xử lý thư nhiều tài khoản cùng lúc.")} } } }
                div { class: "benefit", Icon { name: "layers" } span { strong { {tr("Làm việc hiệu quả hơn")} } small { class: "muted", {tr("Tìm kiếm, lọc, gắn sao trên một giao diện.")} } } }
                div { class: "benefit", Icon { name: "lock" } span { strong { {tr("An toàn & bảo mật")} } small { class: "muted", {tr("IMAP/SMTP qua TLS có kiểm tra chứng chỉ; mật khẩu không lưu dạng văn bản.")} } } }
                div { class: "alert alert-warn push-down",
                    Icon { name: "info" }
                    if REAL_CHECK {
                        span { strong { {tr("Bản Desktop: ")} } {tr("kết nối thật qua TLS. Sau khi kiểm tra, bạn có thể lưu tài khoản và đồng bộ hộp thư đến (chỉ đọc).")} }
                    } else {
                        span { strong { {tr("Bản Web: ")} } {tr("màn này không kết nối mạng và không nhận mật khẩu.")} }
                    }
                }
            }
            section { class: "connect-main", "aria-label": tr("Thêm tài khoản email"),
                h1 { {tr("Thêm tài khoản email")} }
                p { class: "muted", {tr("Kết nối một tài khoản email đã có để đưa hộp thư về MailBox.")} }
                ol { class: "stepper", "aria-label": tr("Tiến trình"),
                    for (n, label) in [(1u8, tr("Nhập thông tin")), (2, tr("Kiểm tra kết nối")), (3, tr("Hoàn tất"))] {
                        li { key: "{n}", class: if n == current { "step current" } else if n < current { "step done" } else { "step" },
                            "data-testid": "connect-step-{n}",
                            "aria-current": if n == current { "step" } else { "false" },
                            span { class: "step-num", if n < current { Icon { name: "check", size: 14 } } else { "{n}" } }
                            span { "{label}" }
                        }
                    }
                }
                if current == 1 {
                    OAuthMockButtons { selected: oauth_mock(), onpick: move |p| oauth_mock.set(Some(p)) }
                    if let Some(p) = oauth_mock() {
                        OAuthMockPanel { provider: p, onclose: move |_| oauth_mock.set(None) }
                    }
                    h2 { {tr("1. Chọn nhà cung cấp ")} span { class: "muted normal", {tr("(tùy chọn)")} } }
                    div { class: "provider-grid", role: "radiogroup", "aria-label": tr("Nhà cung cấp"),
                        for p in Provider::ALL {
                            button { key: "{p.key()}", class: if f.provider == p { "provider chosen" } else { "provider" }, r#type: "button", role: "radio",
                                "aria-checked": if f.provider == p { "true" } else { "false" },
                                "data-testid": "connect-provider-{p.key()}",
                                onclick: move |_| { let mut w = form.write(); w.provider = p; w.apply_suggestion(); },
                                span { class: "mono-tile", if p == Provider::Custom { Icon { name: "server" } } else { "{p.monogram()}" } }
                                span { "{p.label()}" }
                            }
                        }
                    }
                    div { class: "autodetect",
                        button { class: if f.autodetect { "switch on" } else { "switch" }, r#type: "button", role: "switch",
                            "aria-checked": if f.autodetect { "true" } else { "false" },
                            "aria-label": tr("Tự động phát hiện cấu hình"), "data-testid": "connect-autodetect",
                            onclick: move |_| { let v = !form.read().autodetect; form.write().autodetect = v; },
                            span { class: "knob" }
                        }
                        if REAL_CHECK && app.is_real() {
                            span { strong { {tr("Tự động phát hiện cấu hình")} } small { class: "muted", {tr("Bảng nhà cung cấp, DNS SRV, autoconfig qua HTTPS và tên máy chủ thông dụng. Chỉ tên miền được dùng; luôn cho phép sửa tay.")} } }
                        } else {
                            span { strong { {tr("Tự động phát hiện cấu hình")} } small { class: "muted", {tr("Gợi ý host/cổng theo quy tắc nhà cung cấp đã biết; luôn cho phép sửa tay. Không tải URL tùy ý.")} } }
                        }
                    }
                    if REAL_CHECK && app.is_real() && f.autodetect {
                        div { class: "autoconfig-actions",
                            button { class: "btn btn-sm", r#type: "button", "data-testid": "autoconfig-run", disabled: detecting() || mailbox_autoconfig::domain_of(&f.email).is_none(),
                                onclick: move |_| {
                                    let email = form.read().email.clone();
                                    let Some(domain) = mailbox_autoconfig::domain_of(&email) else { return };
                                    detecting.set(true);
                                    detection.set(None);
                                    spawn(async move {
                                        #[cfg(not(target_arch = "wasm32"))]
                                        if let Some(d) = crate::adapters::connect::detect_servers(&email).await {
                                            detection.set(Some((domain, d)));
                                        }
                                        #[cfg(target_arch = "wasm32")]
                                        let _ = (email, domain);
                                        detecting.set(false);
                                    });
                                },
                                Icon { name: "search", size: 14 } if detecting() { {tr("Đang dò cấu hình…")} } else { {tr("Dò cấu hình từ địa chỉ email")} }
                            }
                            small { class: "muted", {tr("Nhập email trước. Có thể mất vài giây.")} }
                        }
                        if let Some((domain, d)) = detection() {
                            AutoconfigResult { detection: d.clone(), domain: domain.clone(), onapply: move |_| {
                                if let Some((_, d)) = detection.peek().clone() { form.write().apply_detection(&d); }
                            } }
                        }
                    }
                    h2 { {tr("2. Thông tin kết nối")} }
                    div { class: "grid-2",
                        FieldInput { label: tr("Địa chỉ email *"), testid: "connect-email", value: f.email.clone(), error: err(Field::Email), error_key: Field::Email.key(), input_type: "email",
                            oninput: move |v: String| { let mut w = form.write(); w.email = v; if w.provider == Provider::Cpanel { w.apply_suggestion(); } } }
                        if REAL_CHECK {
                            label { class: "field",
                                span { {tr("Mật khẩu / Mật khẩu ứng dụng *")} }
                                span { class: if password_missing() { "input-icon invalid" } else { "input-icon" },
                                    Icon { name: "lock" }
                                    input { r#type: "password", "data-testid": "connect-password", autocomplete: "off",
                                        "aria-invalid": if password_missing() { "true" } else { "false" },
                                        placeholder: tr("Nhập mật khẩu hộp thư hoặc mật khẩu ứng dụng"),
                                        value: "{password}",
                                        oninput: move |e| { password.set(e.value()); password_missing.set(false); },
                                    }
                                }
                                if password_missing() {
                                    small { class: "field-error", role: "alert", "data-testid": "connect-error-password", {tr("Nhập mật khẩu để kiểm tra kết nối.")} }
                                } else {
                                    small { class: "muted", {tr("Chỉ dùng để kiểm tra kết nối qua TLS. Không lưu, không ghi log.")} }
                                }
                            }
                        } else {
                            label { class: "field",
                                span { {tr("Mật khẩu / Mật khẩu ứng dụng")} }
                                span { class: "input-icon disabled",
                                    Icon { name: "lock" }
                                    input { r#type: "password", "data-testid": "connect-password", disabled: true, autocomplete: "off",
                                        placeholder: tr("Bị khóa trên bản Web — không nhập mật khẩu thật") }
                                }
                                small { class: "muted", {tr("Bản Web chưa có đăng nhập và kho bí mật (MB-110/MB-113). Dùng bản Desktop để kiểm tra kết nối thật.")} }
                            }
                        }
                    }
                    div { class: "grid-2",
                        fieldset { class: "server-box",
                            legend { {tr("Máy chủ nhận thư (IMAP) *")} }
                            div { class: "host-port",
                                FieldInput { label: tr("Máy chủ"), testid: "connect-imap-host", value: f.imap_host.clone(), error: err(Field::ImapHost), error_key: Field::ImapHost.key(),
                                    oninput: move |v: String| form.write().imap_host = v }
                                FieldInput { label: tr("Cổng"), testid: "connect-imap-port", value: f.imap_port.clone(), error: err(Field::ImapPort), error_key: Field::ImapPort.key(), input_type: "text",
                                    oninput: move |v: String| form.write().imap_port = v }
                            }
                            p { class: "tls-line", Icon { name: "lock", size: 15 } {tr("SSL/TLS bắt buộc")} }
                        }
                        fieldset { class: "server-box",
                            legend { {tr("Máy chủ gửi thư (SMTP) *")} }
                            div { class: "host-port",
                                FieldInput { label: tr("Máy chủ"), testid: "connect-smtp-host", value: f.smtp_host.clone(), error: err(Field::SmtpHost), error_key: Field::SmtpHost.key(),
                                    oninput: move |v: String| form.write().smtp_host = v }
                                FieldInput { label: tr("Cổng"), testid: "connect-smtp-port", value: f.smtp_port.clone(), error: err(Field::SmtpPort), error_key: Field::SmtpPort.key(), input_type: "text",
                                    oninput: move |v: String| form.write().smtp_port = v }
                            }
                            p { class: "tls-line", Icon { name: "lock", size: 15 } {tr("SSL/TLS 465 hoặc STARTTLS 587")} }
                        }
                    }
                    div { class: "alert alert-info",
                        Icon { name: "shield" }
                        span { strong { {tr("Chỉ kết nối mã hóa. ")} } {tr("Chứng chỉ máy chủ luôn được kiểm tra; không có tùy chọn bỏ qua lỗi chứng chỉ.")} }
                    }
                    div { class: "form-actions",
                        button { class: "btn btn-ghost", r#type: "button", onclick: move |_| app.go(Screen::Inbox), {tr("Hủy")} }
                        button { class: "btn btn-primary", r#type: "button", "data-testid": "connect-submit",
                            onclick: move |_| {
                                attempted.set(true);
                                let f = form.read().clone();
                                if !f.errors().is_empty() { return; }
                                if !REAL_CHECK { step.set(2); scenario.set(None); return; }
                                if password.read().is_empty() { password_missing.set(true); return; }
                                // Take the password out of UI state before the check starts.
                                let secret = std::mem::take(&mut *password.write());
                                let imap_port = f.imap_port.trim().parse::<u16>().unwrap_or(0);
                                let smtp_port = f.smtp_port.trim().parse::<u16>().unwrap_or(0);
                                // Hosts proposed by autoconfig are checked through the SSRF guard (MB-304).
                                let found = detection.peek().as_ref().map(|(_, d)| d.clone());
                                let imap_guarded = host_from_detection(found.as_ref(), true, &f.imap_host);
                                let smtp_guarded = host_from_detection(found.as_ref(), false, &f.smtp_host);
                                live.set(None);
                                live_smtp.set(None);
                                saved.set(None);
                                held_secret.set(None);
                                checking.set(true);
                                step.set(2);
                                spawn(async move {
                                    // IMAP and SMTP are checked separately so one failing is not hidden by the other.
                                    let imap = check_imap(f.imap_host.trim(), imap_port, f.email.trim(), secret.clone(), imap_guarded).await;
                                    live.set(Some(imap));
                                    let imap_ok = matches!(*live.peek(), Some(Ok(_)));
                                    let smtp = check_smtp(f.smtp_host.trim(), smtp_port, f.email.trim(), secret.clone(), smtp_guarded).await;
                                    live_smtp.set(Some(smtp));
                                    held_secret.set(if imap_ok { Some(secret) } else { None });
                                    checking.set(false);
                                });
                            },
                            Icon { name: "link" } if REAL_CHECK { {tr("Kiểm tra kết nối")} } else { {tr("Kiểm tra kết nối (demo)")} }
                        }
                    }
                } else if current == 2 && REAL_CHECK {
                    h2 { {tr("Kiểm tra kết nối")} }
                    div { class: "summary-box",
                        span { Icon { name: "mail", size: 16 } "{f.email}" }
                        span { Icon { name: "server", size: 16 } "IMAP {f.imap_host}:{f.imap_port} · SSL/TLS" }
                        span { Icon { name: "send", size: 16 } "SMTP {f.smtp_host}:{f.smtp_port}" }
                    }
                    if checking() {
                        div { class: "alert alert-info", role: "status", "aria-live": "polite", "data-testid": "connect-live-status", "data-state": "checking",
                            Icon { name: "refresh" }
                            span { {tr("Đang kết nối, kiểm tra chứng chỉ và đăng nhập (IMAP rồi SMTP; có máy chủ mất hơn 10 giây)…")} }
                        }
                    }
                    if let Some(Ok(ok)) = live() {
                        {
                            let idle = ok.capabilities.iter().any(|c| c.eq_ignore_ascii_case("IDLE"));
                            let inbox = match ok.inbox_messages {
                                Some(n) => bi(format!("INBOX hiện có {n} thư."), format!("INBOX has {n} message{}.", crate::i18n::s(n as usize))),
                                None => tr("Chưa đọc được số thư trong INBOX.").to_string(),
                            };
                            rsx! {
                                div { class: "alert alert-success", role: "status", "aria-live": "polite", "data-testid": "connect-live-result", "data-state": "ok",
                                    Icon { name: "check" }
                                    span {
                                        strong { {tr("IMAP: chứng chỉ hợp lệ, đăng nhập thành công. ")} }
                                        span { "data-testid": "connect-inbox-count", "{inbox}" }
                                        if idle { {tr(" Máy chủ hỗ trợ IDLE (nhận thư tức thời).")} }
                                    }
                                }
                                p { class: "muted small", {tr("Chỉ đọc số lượng thư (EXAMINE, không thay đổi gì trên máy chủ).")} }
                            }
                        }
                    } else if let Some(Err(failure)) = live() {
                        div { class: "alert alert-danger", role: "alert", "data-testid": "connect-live-result", "data-state": "{failure.key()}",
                            Icon { name: "alert" }
                            span { "{failure.message()}" }
                        }
                    }
                    if let Some(Ok(smtp)) = live_smtp() {
                        div { class: "alert alert-success", role: "status", "aria-live": "polite", "data-testid": "connect-smtp-result", "data-state": "ok",
                            Icon { name: "check" }
                            span {
                                strong { {tr("SMTP: chứng chỉ hợp lệ, đăng nhập thành công. ")} }
                                if let Some(bytes) = smtp.max_size_bytes { {bi(format!("Thư tối đa {} MB. ", bytes / 1_048_576), format!("Messages up to {} MB. ", bytes / 1_048_576))} }
                                {tr("Chưa gửi thư nào.")}
                            }
                        }
                    } else if let Some(Err(failure)) = live_smtp() {
                        div { class: "alert alert-danger", role: "alert", "data-testid": "connect-smtp-result", "data-state": "{failure.key()}",
                            Icon { name: "alert" }
                            span { strong { {tr("SMTP (gửi thư): ")} } "{failure.message()}" }
                        }
                    }
                    if let Some(Err(failure)) = saved() {
                        div { class: "alert alert-danger", role: "alert", "data-testid": "connect-save-result", "data-state": "{failure.key()}",
                            Icon { name: "alert" }
                            span { strong { {tr("Chưa lưu được tài khoản: ")} } {bi(format!("{} Nhập lại mật khẩu để thử lại.", failure.message()), format!("{} Re-enter the password to try again.", failure.message()))} }
                        }
                    }
                    if matches!(live(), Some(Ok(_))) && !checking() {
                        p { class: "muted small", {tr("Khi lưu: mật khẩu vào Keychain của hệ điều hành, thông tin tài khoản và tiêu đề thư vào cơ sở dữ liệu trên máy này. MailBox chỉ đọc hộp thư, không thay đổi gì trên máy chủ.")} }
                    }
                    div { class: "form-actions",
                        button { class: "btn", r#type: "button", disabled: checking() || saving(), onclick: move |_| { held_secret.set(None); step.set(1); }, Icon { name: "arrow-left" } {tr("Quay lại")} }
                        button { class: "btn btn-primary", r#type: "button", "data-testid": "connect-finish",
                            disabled: !matches!(live(), Some(Ok(_))) || checking() || saving(),
                            onclick: move |_| {
                                let Some(secret) = held_secret.write().take() else { return };
                                let f = form.read().clone();
                                saving.set(true);
                                spawn(async move {
                                    let result = app
                                        .save_account(
                                            f.email.trim().to_string(),
                                            f.imap_host.trim().to_string(),
                                            f.imap_port.trim().parse().unwrap_or(0),
                                            f.smtp_host.trim().to_string(),
                                            f.smtp_port.trim().parse().unwrap_or(0),
                                            secret,
                                        )
                                        .await;
                                    let ok = result.is_ok();
                                    saved.set(Some(result));
                                    saving.set(false);
                                    if ok { step.set(3); }
                                });
                            },
                            if saving() { {tr("Đang lưu và đồng bộ…")} } else { {tr("Lưu tài khoản và đồng bộ")} }
                        }
                    }
                } else if current == 2 {
                    h2 { {tr("Kiểm tra kết nối — kịch bản minh họa")} }
                    p { class: "muted", {tr("Bản demo không mở kết nối mạng. Chọn một kết quả để xem giao diện cho từng trường hợp.")} }
                    div { class: "summary-box",
                        span { Icon { name: "mail", size: 16 } "{f.email}" }
                        span { Icon { name: "server", size: 16 } "IMAP {f.imap_host}:{f.imap_port}" }
                        span { Icon { name: "send", size: 16 } "SMTP {f.smtp_host}:{f.smtp_port}" }
                    }
                    div { class: "scenario-grid", role: "radiogroup", "aria-label": tr("Kịch bản kết quả"),
                        for s in Scenario::ALL {
                            button { key: "{s.key()}", class: if scenario() == Some(s) { "scenario chosen" } else { "scenario" }, r#type: "button", role: "radio",
                                "aria-checked": if scenario() == Some(s) { "true" } else { "false" },
                                "data-testid": "connect-scenario-{s.key()}",
                                onclick: move |_| scenario.set(Some(s)),
                                "{s.label()}"
                            }
                        }
                    }
                    if let Some(s) = scenario() {
                        {
                            let (tone, message) = s.outcome(&f.imap_host);
                            rsx! {
                                div { class: "alert alert-{tone}", role: "status", "aria-live": "polite", "data-testid": "connect-result", "data-tone": "{tone}",
                                    Icon { name: pick(tone == "danger", "alert", "info") }
                                    span { "{message}" }
                                }
                            }
                        }
                    }
                    div { class: "form-actions",
                        button { class: "btn", r#type: "button", onclick: move |_| step.set(1), Icon { name: "arrow-left" } {tr("Quay lại")} }
                        button { class: "btn btn-primary", r#type: "button", "data-testid": "connect-finish",
                            disabled: !scenario().is_some_and(Scenario::can_finish),
                            onclick: move |_| step.set(3),
                            {tr("Tiếp tục")}
                        }
                    }
                } else {
                    div { class: "done-box", "data-testid": "connect-done", "data-saved": if matches!(saved(), Some(Ok(_))) { "true" } else { "false" },
                        if REAL_CHECK {
                            span { class: "empty-icon ok", Icon { name: "check", size: 28 } }
                            h2 { {tr("Đã lưu tài khoản")} }
                            if let Some(Ok((folders, messages))) = saved() {
                                p { class: "muted", "data-testid": "connect-sync-summary", {bi(format!("Tìm thấy {folders} thư mục và đồng bộ {messages} thư từ hộp thư đến (tiêu đề, người gửi, thời gian). Mật khẩu nằm trong Keychain của hệ điều hành."), format!("Found {folders} folder{} and synced {messages} message{} from the inbox (subject, sender, time). The password is kept in the operating system keychain.", crate::i18n::s(folders), crate::i18n::s(messages)))} }
                            }
                            p { class: "muted small", {tr("Nội dung thư và các thư mục khác chưa được tải trong bản này (MB-130).")} }
                        } else {
                            span { class: "empty-icon warn", Icon { name: "info", size: 28 } }
                            h2 { {tr("Đã xem hết luồng kết nối (demo)")} }
                            p { class: "muted", {tr("Không có tài khoản, mật khẩu hay cấu hình nào được lưu. Bản Web chưa kiểm tra kết nối thật.")} }
                        }
                        div { class: "form-actions center",
                            button { class: "btn", r#type: "button", onclick: move |_| { form.set(ConnectForm::default()); attempted.set(false); live.set(None); live_smtp.set(None); saved.set(None); held_secret.set(None); step.set(1); }, {tr("Thêm tài khoản khác")} }
                            button { class: "btn btn-primary", r#type: "button", "data-testid": "connect-done-inbox", onclick: move |_| app.go(Screen::Inbox), {tr("Về hộp thư")} }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn FieldInput(
    label: &'static str,
    testid: &'static str,
    value: String,
    error: Option<&'static str>,
    error_key: &'static str,
    #[props(default = "text")] input_type: &'static str,
    oninput: EventHandler<String>,
) -> Element {
    let err_id = format!("connect-error-{error_key}");
    rsx! {
        label { class: "field",
            span { "{label}" }
            input {
                class: if error.is_some() { "input invalid" } else { "input" },
                r#type: "{input_type}",
                "data-testid": "{testid}",
                value: "{value}",
                autocomplete: "off",
                "aria-invalid": if error.is_some() { "true" } else { "false" },
                "aria-describedby": if error.is_some() { err_id.clone() } else { String::new() },
                oninput: move |e| oninput.call(e.value()),
            }
            if let Some(msg) = error {
                small { class: "field-error", id: "{err_id}", "data-testid": "connect-error-{error_key}", role: "alert", "{msg}" }
            }
        }
    }
}
