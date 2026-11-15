//! MailBox UI: one Rust/Dioxus codebase for Web (WASM) and Desktop (native WebView).
//! All email data is deterministic fixture data. No real IMAP/SMTP is connected.
mod adapters;
mod app;
mod components;
mod i18n;
mod routes;
mod screens;
mod state;

#[cfg(all(feature = "web", feature = "desktop"))]
compile_error!("Select one platform: --features web OR --features desktop");

/// Default Desktop window: 1280×700 logical pixels (fits a 1366×768 laptop screen with the
/// menu bar/taskbar), resizable down to the icon-rail layout.
#[cfg(feature = "desktop")]
fn main() {
    use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
    let window = WindowBuilder::new()
        .with_title("MailBox")
        .with_inner_size(LogicalSize::new(1280.0, 700.0))
        .with_min_inner_size(LogicalSize::new(800.0, 560.0));
    dioxus::LaunchBuilder::new()
        // Mail links never navigate or open by themselves; see `adapters/link.rs` (S-09).
        .with_cfg(
            Config::new()
                .with_window(window)
                .with_navigation_handler(adapters::link::gate),
        )
        .launch(app::App);
}

#[cfg(not(feature = "desktop"))]
fn main() {
    dioxus::launch(app::App);
}

/// Renderer-independent UI tests. They render the shared component tree to HTML
/// with `dioxus-ssr`, so they cover the code compiled for both Web and Desktop.
#[cfg(test)]
mod ui_tests {
    use dioxus::prelude::*;

    fn render(root: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(root);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    fn has(html: &str, id: &str) -> bool {
        html.contains(&format!("data-testid=\"{id}\""))
    }

    #[test]
    fn shell_exposes_navigation_test_ids() {
        let html = render(crate::app::App);
        for id in [
            "app-shell",
            "topbar-demo-status",
            "topbar-settings",
            "topbar-theme-toggle",
            "nav-compose",
            "nav-inbox",
            "nav-folder-starred",
            "nav-account-work",
            "nav-add-account",
            "bottom-nav",
            "screen-inbox",
            "message-row-m01",
            "reader-open-thread",
        ] {
            assert!(has(&html, id), "missing test id {id}");
        }
    }

    #[test]
    fn shell_never_claims_real_sync() {
        let html = render(crate::app::App);
        assert!(html.contains("Chế độ demo"), "demo status must be visible");
        assert!(html.contains("data-status=\"demo\""));
        assert!(
            !html.contains("Đồng bộ hoàn tất"),
            "must not claim a real completed sync"
        );
    }

    #[test]
    fn rail_buttons_keep_accessible_names_without_visible_labels() {
        let html = render(crate::app::App);
        for label in [
            "aria-label=\"Thư gắn sao\"",
            "aria-label=\"Hộp thư hợp nhất, 5 chưa đọc\"",
            "aria-label=\"Tài khoản Work, minh@company.vn, 4 chưa đọc\"",
        ] {
            assert!(html.contains(label), "missing {label}");
        }
    }

    #[test]
    fn unified_inbox_renders_one_row_per_conversation() {
        let html = render(crate::app::App);
        assert_eq!(html.matches("data-row=\"true\"").count(), 9);
        assert!(html.contains("data-thread-count=\"5\""));
    }

    /// Render the app at a URL using an in-memory history (same router code as Web/Desktop).
    fn render_at(path: &'static str) -> String {
        thread_local! { static PATH: std::cell::Cell<&'static str> = const { std::cell::Cell::new("/") }; }
        PATH.with(|p| p.set(path));
        fn root() -> Element {
            use_hook(|| {
                let path = PATH.with(|p| p.get());
                dioxus::history::provide_history_context(std::rc::Rc::new(
                    dioxus::history::MemoryHistory::with_initial_path(path),
                ));
            });
            rsx! { crate::app::App {} }
        }
        render(root)
    }

    #[test]
    fn every_route_renders_its_screen_on_shared_code() {
        for (path, id) in [
            ("/", "screen-inbox"),
            ("/inbox", "screen-inbox"),
            ("/messages/m11/thread", "screen-thread"),
            ("/compose", "screen-compose"),
            ("/accounts/connect", "screen-connect"),
            ("/settings", "screen-settings"),
            ("/settings/notifications", "settings-panel-notifications"),
            ("/messages/nope/thread", "not-found"),
            ("/settings/nope", "not-found"),
            ("/no/such/page", "not-found"),
        ] {
            let html = render_at(path);
            assert!(has(&html, id), "{path} did not render {id}");
            assert!(has(&html, "app-shell"), "{path} lost the shell");
        }
    }

    /// Real mode must never show fixture mail, accounts or a demo label.
    #[test]
    fn real_mode_shows_no_demo_data() {
        use crate::state::AppState;
        fn root() -> Element {
            let mut app = use_context_provider(AppState::init);
            use_hook(move || {
                // No saved account: the state real mode has when the local store is empty.
                app.enter_real_with(mailbox_contracts::BootstrapResponse {
                    mode: "real".into(),
                    accounts: Vec::new(),
                    messages: Vec::new(),
                });
            });
            rsx! { Router::<crate::routes::Route> {} }
        }
        let html = render(root);
        assert!(has(&html, "topbar-real-status") && !has(&html, "topbar-demo-status"));
        assert!(has(&html, "topbar-mode-toggle"));
        assert_eq!(
            html.matches("data-row=\"true\"").count(),
            0,
            "no fixture mail"
        );
        for demo_text in [
            "Trần Linh",
            "minh@company.vn",
            "Dữ liệu minh họa",
            "nav-account-work",
        ] {
            assert!(
                !html.contains(demo_text),
                "{demo_text} leaked into real mode"
            );
        }
        assert!(html.contains("Chưa có thư thật để hiển thị"));
    }

    /// Real mode with a saved account shows its real envelopes and a verified (not demo) status.
    #[test]
    fn real_mode_lists_saved_account_mail() {
        use crate::state::AppState;
        use mailbox_contracts::{AccountStatus, AccountSummary, BootstrapResponse, MessageSummary};
        fn root() -> Element {
            let mut app = use_context_provider(AppState::init);
            use_hook(move || {
                let message = MessageSummary {
                    id: "acc-x/INBOX#1#7".into(),
                    account_id: "acc-x".into(),
                    folder_id: "inbox".into(),
                    from_name: "Người gửi thật".into(),
                    from_address: "sender@example.org".into(),
                    subject: "Thư thật số 7".into(),
                    preview: String::new(),
                    body_plain: "placeholder".into(),
                    received_at: "03:04".into(),
                    unread: true,
                    starred: false,
                    attachments: Vec::new(),
                    thread_id: String::new(),
                    to_line: "me@example.com".into(),
                    received_at_full: "14/11/2026 03:04 UTC".into(),
                    labels: Vec::new(),
                };
                let account = AccountSummary {
                    id: "acc-x".into(),
                    name: "example.com".into(),
                    address: "me@example.com".into(),
                    status: AccountStatus::Connected,
                    unread: 1,
                    color: "#2672F6".into(),
                    last_synced_at: None,
                };
                app.enter_real_with(BootstrapResponse {
                    mode: "real".into(),
                    accounts: vec![account],
                    messages: vec![message],
                });
            });
            rsx! { Router::<crate::routes::Route> {} }
        }
        let html = render(root);
        assert_eq!(html.matches("data-row=\"true\"").count(), 1);
        assert!(html.contains("Thư thật số 7") && html.contains("Người gửi thật"));
        assert!(has(&html, "nav-account-acc-x") && has(&html, "topbar-real-status"));
        assert!(!html.contains("Trần Linh") && !html.contains("Dữ liệu minh họa"));
    }

    fn real_message(id: &str, folder: &str, unread: bool) -> mailbox_contracts::MessageSummary {
        mailbox_contracts::MessageSummary {
            id: id.into(),
            account_id: "acc-x".into(),
            folder_id: folder.into(),
            from_name: "Người gửi thật".into(),
            from_address: "sender@example.org".into(),
            subject: format!("Thư {id}"),
            preview: String::new(),
            body_plain: "placeholder".into(),
            received_at: "03:04".into(),
            unread,
            starred: false,
            attachments: Vec::new(),
            thread_id: String::new(),
            to_line: "me@example.com".into(),
            received_at_full: "14/11/2026 03:04 UTC".into(),
            labels: Vec::new(),
        }
    }

    /// Real mode with four server folders (no Archive, no Drafts), one pending and one refused
    /// change. `view` selects the folder the list shows.
    fn render_real_view(view: crate::state::folders::RealView) -> String {
        use crate::state::AppState;
        use crate::state::folders::{RealFolder, RealState, RealView, RowOp};
        use mailbox_contracts::{AccountStatus, AccountSummary, BootstrapResponse};
        thread_local! { static VIEW: std::cell::RefCell<RealView> = const { std::cell::RefCell::new(RealView::Inbox) }; }
        VIEW.with(|v| *v.borrow_mut() = view);
        fn root() -> Element {
            let mut app = use_context_provider(AppState::init);
            let mut real = use_context_provider(RealState::init);
            use_hook(move || {
                let folder = |key: &str, label: &str, role: Option<&str>| RealFolder {
                    key: key.into(),
                    folder_id: format!("acc-x/{label}"),
                    account_id: "acc-x".into(),
                    label: label.into(),
                    role: role.map(str::to_string),
                };
                real.folders.set(vec![
                    folder("inbox", "INBOX", Some("inbox")),
                    folder("sent", "Sent", Some("sent")),
                    folder("trash", "Trash", Some("trash")),
                    folder("acc-x/Projects", "Projects", None),
                ]);
                real.ops.set(
                    [
                        ("m-in".to_string(), RowOp::Pending),
                        ("m-in2".to_string(), RowOp::Failed),
                    ]
                    .into(),
                );
                real.view.set(VIEW.with(|v| v.borrow().clone()));
                app.enter_real_with(BootstrapResponse {
                    mode: "real".into(),
                    accounts: vec![AccountSummary {
                        id: "acc-x".into(),
                        name: "example.com".into(),
                        address: "me@example.com".into(),
                        status: AccountStatus::Connected,
                        unread: 2,
                        color: "#2672F6".into(),
                        last_synced_at: None,
                    }],
                    messages: vec![
                        real_message("m-in", "inbox", true),
                        real_message("m-in2", "inbox", true),
                        real_message("m-sent", "sent", false),
                        real_message("m-trash", "trash", true),
                        real_message("m-proj", "acc-x/Projects", true),
                    ],
                });
            });
            rsx! { Router::<crate::routes::Route> {} }
        }
        render(root)
    }

    /// MB-122/143: the sidebar lists the folders the server really has, with unread counts, and
    /// the unified inbox stays the default view.
    #[test]
    fn real_mode_sidebar_lists_real_folders_with_unread_counts() {
        let html = render_real_view(crate::state::folders::RealView::Inbox);
        for id in [
            "nav-inbox",
            "nav-folder-starred",
            "nav-folder-sent",
            "nav-folder-trash",
            "nav-real-folder-acc-x-projects",
        ] {
            assert!(has(&html, id), "missing {id}");
        }
        for absent in [
            "nav-folder-archive",
            "nav-folder-drafts",
            "nav-folder-scheduled",
        ] {
            assert!(
                !has(&html, absent),
                "{absent} is not a folder of this account"
            );
        }
        for label in [
            "aria-label=\"Hộp thư hợp nhất, 2 chưa đọc\"",
            "aria-label=\"Thùng rác, 1 chưa đọc\"",
            "aria-label=\"Projects, 1 chưa đọc\"",
            "aria-label=\"Đã gửi\"",
            "aria-label=\"Tài khoản example.com, me@example.com, 2 chưa đọc\"",
        ] {
            assert!(html.contains(label), "missing {label}");
        }
        assert_eq!(
            html.matches("data-row=\"true\"").count(),
            2,
            "the unified inbox shows inbox mail only"
        );
        assert!(has(&html, "message-row-m-in") && !has(&html, "message-row-m-trash"));
        assert!(html.contains("2 thư chưa đọc"));
        assert!(!html.contains("(demo)") && !html.contains("Dữ liệu minh họa"));
    }

    /// MB-143: choosing a real folder routes the list to it.
    #[test]
    fn real_mode_list_follows_the_selected_real_folder() {
        let html = render_real_view(crate::state::folders::RealView::Role("trash".into()));
        assert_eq!(html.matches("data-row=\"true\"").count(), 1);
        assert!(has(&html, "message-row-m-trash"));
        let title = html.find("data-testid=\"inbox-title\"").unwrap();
        assert!(html[title..title + 80].contains("Thùng rác"));

        let html = render_real_view(crate::state::folders::RealView::Folder(
            "acc-x/Projects".into(),
        ));
        assert_eq!(html.matches("data-row=\"true\"").count(), 1);
        assert!(has(&html, "message-row-m-proj"));
    }

    /// MB-132/144: rows show changes the server has not confirmed or has refused, and the
    /// reader offers the account's real folders as move targets.
    #[test]
    fn real_mode_rows_show_pending_and_failed_states_and_a_move_menu() {
        let html = render_real_view(crate::state::folders::RealView::Inbox);
        assert!(has(&html, "row-op-pending-m-in"));
        assert!(has(&html, "row-op-failed-m-in2") && has(&html, "row-op-dismiss-m-in2"));
        assert_eq!(html.matches("data-op=\"pending\"").count(), 1);
        assert_eq!(html.matches("data-op=\"failed\"").count(), 1);
        assert!(html.contains("Máy chủ từ chối"));
        // Existing controls keep their ids.
        for id in [
            "reader-archive",
            "reader-delete",
            "reader-toggle-unread",
            "reader-star",
            "message-row-star-m-in",
        ] {
            assert!(has(&html, id), "missing {id}");
        }
        let menu = html.find("data-testid=\"reader-move\"").expect("move menu");
        let end = menu + html[menu..].find("</select>").unwrap();
        let options = &html[menu..end];
        for target in ["acc-x/Sent", "acc-x/Trash", "acc-x/Projects"] {
            assert!(options.contains(&format!("value=\"{target}\"")), "{target}");
        }
        assert!(
            !options.contains("value=\"acc-x/INBOX\""),
            "the folder the message is in is not a target"
        );
    }

    /// Demo mode keeps its own folders and local-only actions: no real controls appear.
    #[test]
    fn demo_mode_has_no_real_folder_controls() {
        let html = render(crate::app::App);
        assert!(has(&html, "nav-folder-scheduled") && has(&html, "nav-folder-archive"));
        assert!(!has(&html, "reader-move"));
        assert!(!html.contains("nav-real-folder-") && !html.contains("row-op-"));
        assert_eq!(html.matches("data-op=\"none\"").count(), 9);
    }

    #[test]
    fn desktop_password_field_is_masked_and_never_prefilled() {
        let html = render_at("/accounts/connect");
        let idx = html
            .find("data-testid=\"connect-password\"")
            .expect("password field present");
        let tag_start = html[..idx].rfind('<').unwrap();
        let tag_end = idx + html[idx..].find('>').unwrap();
        let tag = &html[tag_start..tag_end];
        assert!(tag.contains("type=\"password\""), "must be masked");
        assert!(tag.contains("autocomplete=\"off\""));
        assert!(
            tag.contains("value=\"\"") || !tag.contains("value="),
            "must start empty"
        );
        // No way around certificate validation is offered anywhere on the screen.
        assert!(!html.contains("connect-bypass"));
    }

    #[test]
    fn settings_accounts_show_demo_status_not_connected() {
        let html = render_at("/settings");
        for id in ["work", "personal", "support"] {
            assert!(has(&html, &format!("settings-account-status-{id}")));
        }
        assert!(
            !html.contains("Đang hoạt động"),
            "must not claim an active connection"
        );
    }

    // ---- MB-130 / MB-131: message bodies ----

    use crate::adapters::body::{AttachmentView, BodyFailure, BodyState, BodyView};

    /// Render one body state inside the app context (no router or data source needed).
    fn render_pane(state: BodyState, show_images: bool, plain_view: bool) -> String {
        thread_local! {
            static CASE: std::cell::RefCell<Option<(BodyState, bool, bool)>> =
                const { std::cell::RefCell::new(None) };
        }
        CASE.with(|c| *c.borrow_mut() = Some((state, show_images, plain_view)));
        fn root() -> Element {
            use_context_provider(crate::state::AppState::init);
            let (state, show_images, plain_view) =
                CASE.with(|c| c.borrow().clone()).expect("case set");
            rsx! {
                crate::components::body::BodyPane {
                    id: "acc-x/INBOX#1#7",
                    state,
                    show_images,
                    plain_view,
                    on_retry: |_| {},
                    on_images: |_| {},
                    on_plain: |_| {},
                }
            }
        }
        render(root)
    }

    /// What the browser sees after decoding attribute/text escapes once (e.g. the `srcdoc` value).
    fn unescape(html: &str) -> String {
        html.replace("&#60;", "<")
            .replace("&#62;", ">")
            .replace("&#34;", "\"")
            .replace("&#39;", "'")
            .replace("&#38;", "&")
    }

    /// A hostile message through the real parser and sanitizer, as the reader receives it.
    #[cfg(not(target_arch = "wasm32"))]
    fn hostile_view() -> BodyView {
        let raw = concat!(
            "From: Sender <s@example.org>\r\nSubject: t\r\n",
            "Content-Type: multipart/mixed; boundary=b\r\n\r\n",
            "--b\r\nContent-Type: text/html; charset=utf-8\r\n\r\n",
            "<html><head><style>p{background:url(https://evil.example/c.png)}</style>",
            "<base href=\"https://evil.example/\"><meta http-equiv=\"refresh\" content=\"0;url=https://evil.example\">",
            "<link rel=\"stylesheet\" href=\"https://evil.example/a.css\"></head><body onload=\"x()\">",
            "<p style=\"color:red;background:url(https://evil.example/bg.png)\">Xin ch\u{e0}o</p>",
            "<script>alert(1)</script><img src=\"https://evil.example/pixel.gif\" onerror=\"alert(2)\" alt=\"logo\">",
            "<iframe src=\"https://evil.example\"></iframe><form action=\"https://evil.example\"><input name=\"p\"></form>",
            "<a href=\"javascript:alert(3)\">bad</a> <a href=\"https://example.org/ok\">good</a>",
            "</body></html>\r\n",
            "--b\r\nContent-Type: application/pdf\r\n",
            "Content-Disposition: attachment; filename=\"../../x\"\r\n\r\nPDF\r\n--b--\r\n"
        );
        let record = mailbox_mime::parse(raw.as_bytes(), &mailbox_mime::Limits::default())
            .unwrap()
            .into_record("m");
        BodyView {
            text: record.text,
            html: record.html,
            html_remote: record.html_remote,
            remote_blocked: record.remote_blocked,
            truncated: record.truncated,
            attachments: record
                .attachments
                .into_iter()
                .map(|a| AttachmentView {
                    part: a.part,
                    filename: a.filename,
                    media_type: a.media_type,
                    size_bytes: a.size_bytes,
                    inline: a.inline,
                })
                .collect(),
        }
    }

    #[test]
    fn body_states_have_their_own_controls() {
        let html = render_pane(BodyState::Loading, false, false);
        assert!(has(&html, "message-body") && has(&html, "body-loading"));
        assert!(html.contains("data-state=\"loading\"") && html.contains("role=\"status\""));
        assert!(!has(&html, "body-retry") && !has(&html, "body-html-frame"));

        let html = render_pane(BodyState::Failed(BodyFailure::Unreachable), false, false);
        assert!(has(&html, "body-error") && has(&html, "body-retry"));
        assert!(html.contains("data-reason=\"unreachable\"") && html.contains("role=\"alert\""));
        assert!(html.contains("Không kết nối được máy chủ thư"));

        let html = render_pane(BodyState::TooLarge, false, false);
        assert!(has(&html, "body-too-large") && html.contains("25 MB"));
        assert!(
            !has(&html, "body-retry"),
            "retrying cannot make a message smaller"
        );

        let view = BodyView {
            text: "Chỉ có văn bản <b>không phải thẻ</b>".into(),
            truncated: true,
            ..BodyView::default()
        };
        let html = render_pane(BodyState::Loaded(view), false, false);
        assert!(has(&html, "body-text") && has(&html, "body-truncated"));
        assert!(
            html.contains("&#60;b&#62;") && !html.contains("<b>không"),
            "plain text is escaped, never markup"
        );
        for absent in [
            "body-html-frame",
            "body-remote-blocked",
            "body-view-toggle",
            "body-attachments",
        ] {
            assert!(!has(&html, absent), "{absent} shown for a plain message");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn hostile_html_reaches_the_reader_only_sanitized_and_sandboxed() {
        let html = render_pane(BodyState::Loaded(hostile_view()), false, false);
        // The frame: sandboxed, content only through srcdoc. Its single permission is a
        // top-frame navigation started by a click, which the shell always refuses (S-09).
        let at = html
            .find("data-testid=\"body-html-frame\"")
            .expect("html frame");
        let tag_start = html[..at].rfind('<').unwrap();
        let tag = &html[tag_start..at + html[at..].find('>').unwrap()];
        assert!(
            tag.starts_with("<iframe")
                && tag.contains("sandbox=\"allow-top-navigation-by-user-activation\""),
            "{tag}"
        );
        assert!(tag.contains("srcdoc=\"") && !tag.contains(" src=\""));
        assert!(tag.contains("data-remote-images=\"blocked\""));
        assert!(tag.contains("referrerpolicy=\"no-referrer\""));
        for token in [
            "allow-scripts",
            "allow-same-origin",
            "allow-forms",
            "allow-popups",
            "allow-modals",
            "allow-downloads",
            // The unconditional form (a script-free document could not use it, still never).
            "allow-top-navigation\"",
            "allow-top-navigation ",
            "allow-top-navigation-to-custom-protocols",
        ] {
            assert!(!html.contains(token), "{token} must never be granted");
        }
        // Links leave the frame only as a top-frame navigation the shell refuses.
        assert!(unescape(&html).contains("<base target=\"_top\">"));
        // The only `<base>` is MailBox's own; the message cannot bring one.
        assert_eq!(unescape(&html).to_lowercase().matches("<base").count(), 1);
        let lower = unescape(&html).to_lowercase();
        assert!(
            lower.contains(
                "content=\"default-src 'none'; img-src data: cid:; style-src 'unsafe-inline'\""
            ),
            "CSP meta missing"
        );
        // Nothing active or remote survives, neither in the page nor in the frame document.
        for forbidden in [
            "<script",
            "alert(",
            "onerror",
            "onload",
            "javascript:",
            "evil.example",
            "<iframe src",
            "<form",
            "<input",
            "<base href",
            "<link",
            "http-equiv=\"refresh",
            "url(",
            "src=\"http",
            "@import",
        ] {
            assert!(!lower.contains(forbidden), "{forbidden} survived");
            assert!(
                !html.to_lowercase().contains(forbidden),
                "{forbidden} survived (raw)"
            );
        }
        assert_eq!(
            lower.matches("<iframe").count(),
            1,
            "only the reader's own frame"
        );
        // The allowed link survives, but only in its inert form (AUDIT S-09).
        assert!(lower.contains("xin chào") && !lower.contains("example.org"));
        let start = lower.find("mailbox-link:").expect("guarded link");
        let end = start + lower[start..].find('"').unwrap();
        assert_eq!(
            mailbox_core::link::unguard(&lower[start..end]).as_deref(),
            Some("https://example.org/ok")
        );
        assert!(lower.contains("noopener noreferrer"));
        // Remote content is reported, with an explicit per-message action.
        assert!(has(&html, "body-remote-blocked") && has(&html, "body-show-images"));
        assert!(!has(&html, "body-images-shown"));
        // Attachment: real metadata, traversal-free name, an enabled save control.
        assert!(has(&html, "body-attachments") && has(&html, "attachment-real-0"));
        assert!(has(&html, "attachment-save-0"));
        assert!(html.contains("title=\"x\"") && !html.contains("../"));
        assert!(html.contains("Lưu x vào thư mục Tải về"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn show_images_is_explicit_and_limited_to_images() {
        let html = render_pane(BodyState::Loaded(hostile_view()), true, false);
        assert!(html.contains("data-remote-images=\"shown\""));
        assert!(has(&html, "body-images-shown") && has(&html, "body-hide-images"));
        assert!(!has(&html, "body-show-images"));
        let lower = unescape(&html).to_lowercase();
        assert!(
            lower.contains("src=\"https://evil.example/pixel.gif\""),
            "the remote image is the one thing let through"
        );
        assert!(lower.contains("img-src data: cid: https: http:;"));
        assert_eq!(
            lower.matches("evil.example").count(),
            1,
            "no other remote reference returns"
        );
        for forbidden in [
            "<script",
            "onerror",
            "javascript:",
            "url(",
            "<iframe src",
            "<form",
            "allow-scripts",
        ] {
            assert!(
                !lower.contains(forbidden),
                "{forbidden} survived with images on"
            );
        }
        // Plain-text view: no frame at all, nothing remote.
        let html = render_pane(BodyState::Loaded(hostile_view()), true, true);
        assert!(has(&html, "body-text") && !has(&html, "body-html-frame"));
        assert!(!html.contains("evil.example") && has(&html, "body-view-toggle"));
    }

    /// Real mode: the reader loads the body (loading state first), never a fixed placeholder.
    #[test]
    fn real_reader_loads_the_body_instead_of_a_placeholder() {
        use crate::state::AppState;
        use mailbox_contracts::{AccountStatus, AccountSummary, BootstrapResponse, MessageSummary};
        fn root() -> Element {
            let mut app = use_context_provider(AppState::init);
            use_hook(move || {
                app.enter_real_with(BootstrapResponse {
                    mode: "real".into(),
                    accounts: vec![AccountSummary {
                        id: "acc-x".into(),
                        name: "example.com".into(),
                        address: "me@example.com".into(),
                        status: AccountStatus::Connected,
                        unread: 1,
                        color: "#2672F6".into(),
                        last_synced_at: None,
                    }],
                    messages: vec![MessageSummary {
                        id: "acc-x/INBOX#1#7".into(),
                        account_id: "acc-x".into(),
                        folder_id: "inbox".into(),
                        from_name: "Người gửi thật".into(),
                        from_address: "sender@example.org".into(),
                        subject: "Thư thật số 7".into(),
                        preview: String::new(),
                        body_plain: "PLACEHOLDER-MUST-NOT-SHOW".into(),
                        received_at: "03:04".into(),
                        unread: true,
                        starred: false,
                        attachments: Vec::new(),
                        thread_id: String::new(),
                        to_line: "me@example.com".into(),
                        received_at_full: "14/11/2026 03:04 UTC".into(),
                        labels: Vec::new(),
                    }],
                });
            });
            rsx! { Router::<crate::routes::Route> {} }
        }
        let html = render(root);
        assert!(has(&html, "reader-subject") && has(&html, "message-body"));
        assert!(has(&html, "body-loading"));
        assert!(html.contains("data-message=\"acc-x/INBOX#1#7\""));
        assert!(!html.contains("PLACEHOLDER-MUST-NOT-SHOW"));
    }

    /// Demo mode keeps showing fixture bodies and disabled demo attachments, unchanged.
    #[test]
    fn demo_reader_and_thread_show_fixture_bodies_unchanged() {
        for path in ["/", "/messages/m01/thread"] {
            let html = render_at(path);
            assert!(html.contains("class=\"body-text\""), "{path}");
            for real_only in [
                "message-body",
                "body-loading",
                "body-html-frame",
                "attachment-save-",
            ] {
                assert!(
                    !html.contains(real_only),
                    "{real_only} leaked into demo at {path}"
                );
            }
        }
    }

    /// One demo message carries sample links, inert and sandboxed like real mail, so the link
    /// confirmation (AUDIT S-09) can be tried without an account.
    #[test]
    fn demo_security_message_offers_guarded_sample_links() {
        let html = render_at("/messages/m06/thread");
        assert!(html.contains("data-testid=\"demo-link-frame\""));
        assert!(html.contains("data-testid=\"demo-link-note\""));
        assert!(html.contains("sandbox=\"allow-top-navigation-by-user-activation\""));
        assert!(html.contains("mailbox-link:"));
        assert!(html.contains("base target="));
        for live in ["href=\"http", "href=&quot;http", "href=&#34;http"] {
            assert!(!html.contains(live), "live link in the demo frame: {live}");
        }
        assert!(!render_at("/messages/m01/thread").contains("demo-link-frame"));
    }

    // --- MB-133 / MB-150 / MB-151: composer, send states and outbox ---

    use crate::state::AppState;
    use crate::state::compose::{Draft, RecipientField};
    use crate::state::outbox::{OUTBOX, SaveStatus, SendPhase};
    use mailbox_core::outbox::{DraftRecord, FailReason, OutboxState, SentCopyState};

    type Setup = Box<dyn Fn(&mut AppState)>;
    thread_local! {
        static REAL_PATH: std::cell::Cell<&'static str> = const { std::cell::Cell::new("/compose") };
        static SETUP: std::cell::RefCell<Option<Setup>> = const { std::cell::RefCell::new(None) };
    }

    /// Real mode with one saved account, at `path`, after `setup` prepared the state.
    fn render_real(path: &'static str, setup: impl Fn(&mut AppState) + 'static) -> String {
        use mailbox_contracts::{AccountStatus, AccountSummary, BootstrapResponse};
        REAL_PATH.with(|p| p.set(path));
        SETUP.with(|s| *s.borrow_mut() = Some(Box::new(setup)));
        fn root() -> Element {
            use_hook(|| {
                dioxus::history::provide_history_context(std::rc::Rc::new(
                    dioxus::history::MemoryHistory::with_initial_path(REAL_PATH.with(|p| p.get())),
                ));
            });
            let mut app = use_context_provider(AppState::init);
            use_hook(move || {
                app.enter_real_with(BootstrapResponse {
                    mode: "real".into(),
                    accounts: vec![AccountSummary {
                        id: "acc-x".into(),
                        name: "example.com".into(),
                        address: "me@example.com".into(),
                        status: AccountStatus::Connected,
                        unread: 0,
                        color: "#2672F6".into(),
                        last_synced_at: None,
                    }],
                    messages: Vec::new(),
                });
                SETUP.with(|s| {
                    if let Some(setup) = s.borrow().as_ref() {
                        setup(&mut app);
                    }
                });
            });
            rsx! { Router::<crate::routes::Route> {} }
        }
        render(root)
    }

    /// A saved draft `d-1` in the composer, with the given send phase.
    fn compose_in(phase: SendPhase) -> String {
        render_real("/compose", move |app| {
            let mut d = Draft::new("acc-x");
            d.add_recipient(RecipientField::To, "Linh <linh@company.vn>");
            d.subject = "Báo cáo".into();
            d.body = "Xin chào".into();
            d.id = "d-1".into();
            d.message_id = "<d-1@example.com>".into();
            app.draft.set(d);
            let mut ui = OUTBOX.write();
            ui.draft_id = "d-1".into();
            ui.phase = phase.clone();
            ui.save = SaveStatus::Saved;
        })
    }

    /// Opening tag of the element with this test id.
    fn tag<'a>(html: &'a str, id: &str) -> &'a str {
        let at = html
            .find(&format!("data-testid=\"{id}\""))
            .unwrap_or_else(|| panic!("missing {id}"));
        let start = html[..at].rfind('<').unwrap();
        &html[start..at + html[at..].find('>').unwrap()]
    }

    #[test]
    fn demo_compose_still_refuses_to_send() {
        let html = render_at("/compose");
        assert!(tag(&html, "screen-compose").contains("data-mode=\"demo\""));
        assert!(html.contains("Gửi (demo)"), "the button says it is a demo");
        assert!(tag(&html, "compose-draft-status").contains("data-state=\"local-demo\""));
        assert!(
            html.contains("không tải lên"),
            "demo attachments are labelled as fake"
        );
        for real_only in [
            "compose-send-state",
            "compose-attach-input",
            "compose-send-retry",
            "compose-unknown-resend",
            "compose-smtp-password",
        ] {
            assert!(!has(&html, real_only), "{real_only} must not exist in demo");
        }
        assert!(!html.contains("Đã gửi thư"), "demo never claims a send");
    }

    #[test]
    fn real_compose_offers_a_real_send_with_real_identities() {
        let html = render_real("/compose", |_| {});
        assert!(tag(&html, "screen-compose").contains("data-mode=\"real\""));
        for id in [
            "compose-from",
            "compose-to-input",
            "compose-subject",
            "compose-body",
            "compose-send",
            "compose-attach-input",
            "compose-close",
        ] {
            assert!(has(&html, id), "missing {id}");
        }
        assert!(tag(&html, "compose-attach-input").contains("type=\"file\""));
        assert!(
            html.contains("example.com – me@example.com"),
            "From lists the saved account"
        );
        assert!(tag(&html, "compose-draft-status").contains("data-state=\"empty\""));
        for demo_text in [
            "Gửi (demo)",
            "Bản nháp cục bộ (demo)",
            "Từ dữ liệu minh họa",
            "Thêm tệp minh họa",
            "Trần Linh",
            "không tải lên",
        ] {
            assert!(
                !html.contains(demo_text),
                "{demo_text} leaked into real mode"
            );
        }
        assert!(
            !has(&html, "compose-send-state"),
            "nothing is being sent yet"
        );
        assert!(!tag(&html, "compose-send").contains("disabled"));
    }

    #[test]
    fn autosave_indicator_reports_the_stored_truth() {
        let html = compose_in(SendPhase::Idle);
        assert!(tag(&html, "compose-draft-status").contains("data-state=\"saved\""));
        assert!(html.contains("Đã lưu bản nháp trên máy này"));
        let failed = render_real("/compose", |app| {
            let mut d = Draft::new("acc-x");
            d.subject = "x".into();
            app.draft.set(d);
            OUTBOX.write().save = SaveStatus::Failed;
        });
        assert!(tag(&failed, "compose-draft-status").contains("data-state=\"save-failed\""));
        assert!(failed.contains("Chưa lưu được bản nháp") && !failed.contains("Đã lưu bản nháp"));
    }

    #[test]
    fn sending_state_locks_the_form_and_the_send_button() {
        let html = compose_in(SendPhase::Sending);
        let state = tag(&html, "compose-send-state");
        assert!(state.contains("data-state=\"sending\"") && html.contains("Đang gửi"));
        assert!(
            tag(&html, "compose-form").contains("disabled"),
            "content is frozen while sending"
        );
        assert!(
            tag(&html, "compose-send").contains("disabled"),
            "no second click"
        );
        assert!(!has(&html, "compose-send-retry"));
    }

    #[test]
    fn sent_state_reports_the_sent_folder_copy_honestly() {
        for (copy, text) in [
            (SentCopyState::Pending, "Đang lưu bản sao"),
            (
                SentCopyState::Saved,
                "Đã lưu một bản sao vào thư mục Đã gửi",
            ),
            (SentCopyState::AlreadyThere, "không tạo bản thứ hai"),
            (SentCopyState::Failed, "chưa lưu được bản sao"),
        ] {
            let html = compose_in(SendPhase::Sent { copy });
            let state = tag(&html, "compose-send-state");
            assert!(state.contains("data-state=\"sent\""), "{copy:?}");
            assert!(state.contains(&format!("data-sent-copy=\"{}\"", copy.key())));
            assert!(
                html.contains("Đã gửi thư") && html.contains(text),
                "{copy:?}"
            );
            assert!(has(&html, "compose-new-message") && !has(&html, "compose-send-retry"));
            assert!(
                tag(&html, "compose-send").contains("disabled"),
                "a sent message cannot be sent again"
            );
        }
    }

    #[test]
    fn failed_state_shows_the_reason_and_retry_only_when_it_can_help() {
        let html = compose_in(SendPhase::Failed {
            reason: FailReason::Network,
            retryable: true,
            rejected: Vec::new(),
        });
        let state = tag(&html, "compose-send-state");
        assert!(
            state.contains("data-state=\"failed\"") && state.contains("data-reason=\"network\"")
        );
        assert!(html.contains("Thư chưa được gửi"));
        assert!(has(&html, "compose-send-retry") && has(&html, "compose-send-edit"));

        let html = compose_in(SendPhase::Failed {
            reason: FailReason::RecipientsRejected,
            retryable: false,
            rejected: vec!["b@y.org".into()],
        });
        assert!(tag(&html, "compose-send-state").contains("data-reason=\"recipients_rejected\""));
        assert!(tag(&html, "compose-rejected-recipient").contains("data-address=\"b@y.org\""));
        assert!(html.contains("chưa được gửi cho ai"));
        assert!(
            !has(&html, "compose-send-retry"),
            "the same message would be refused again"
        );
        assert!(has(&html, "compose-send-edit"));
    }

    #[test]
    fn unknown_state_says_may_have_been_sent_and_never_offers_a_plain_retry() {
        let html = compose_in(SendPhase::Unknown { checked: None });
        assert!(tag(&html, "compose-send-state").contains("data-state=\"unknown\""));
        assert!(html.contains("Thư có thể đã được gửi"));
        assert!(
            html.contains("thư mục Đã gửi"),
            "tells the user where to check"
        );
        for id in [
            "compose-unknown-check",
            "compose-unknown-mark-sent",
            "compose-unknown-resend",
            "compose-send-edit",
        ] {
            assert!(has(&html, id), "missing {id}");
        }
        assert!(
            !has(&html, "compose-send-retry"),
            "uncertain is never a one-click retry"
        );
        assert!(tag(&html, "compose-send").contains("disabled"));
        assert!(
            html.contains("có thể nhận hai thư"),
            "resend warns about a duplicate"
        );

        let html = compose_in(SendPhase::Unknown {
            checked: Some(false),
        });
        assert!(tag(&html, "compose-unknown-check-result").contains("data-found=\"false\""));
        assert!(html.contains("không chứng minh được là thư chưa gửi"));
    }

    #[test]
    fn smtp_password_is_asked_in_a_masked_field_that_starts_empty() {
        let html = render_real("/compose", |app| {
            let mut d = Draft::new("acc-x");
            d.id = "d-1".into();
            d.subject = "x".into();
            app.draft.set(d);
            let mut ui = OUTBOX.write();
            ui.draft_id = "d-1".into();
            ui.phase = SendPhase::Failed {
                reason: FailReason::AuthFailed,
                retryable: true,
                rejected: Vec::new(),
            };
            ui.ask_smtp_password = true;
        });
        assert!(has(&html, "compose-smtp-password-dialog"));
        let field = tag(&html, "compose-smtp-password");
        assert!(field.contains("type=\"password\"") && field.contains("autocomplete=\"off\""));
        assert!(
            field.contains("value=\"\"") || !field.contains("value="),
            "must start empty"
        );
        assert!(html.contains("Keychain"), "says where the password is kept");
    }

    #[test]
    fn send_state_of_another_message_does_not_leak_into_a_new_draft() {
        let html = render_real("/compose", |app| {
            let mut d = Draft::new("acc-x");
            d.id = "d-2".into();
            d.subject = "khác".into();
            app.draft.set(d);
            let mut ui = OUTBOX.write();
            ui.draft_id = "d-1".into();
            ui.phase = SendPhase::Sent {
                copy: SentCopyState::Saved,
            };
        });
        assert!(!has(&html, "compose-send-state"));
        assert!(!tag(&html, "compose-send").contains("disabled"));
    }

    fn outbox_record(id: &str, subject: &str, state: OutboxState) -> DraftRecord {
        DraftRecord {
            id: id.into(),
            owner_id: "local".into(),
            account_id: "acc-x".into(),
            message_id: format!("<{id}@example.com>"),
            from_identity: "me@example.com".into(),
            to: vec!["Linh <linh@company.vn>".into()],
            subject: subject.into(),
            state,
            ..DraftRecord::default()
        }
    }

    #[test]
    fn drafts_folder_in_real_mode_lists_local_drafts_and_outbox_states() {
        use crate::state::mail::{Filter, Folder};
        let html = render_real("/inbox", |app| {
            app.filter.set(Filter {
                folder: Folder::Drafts,
                ..Filter::default()
            });
            let mut ui = OUTBOX.write();
            ui.items_loaded = true;
            ui.items = vec![
                outbox_record("d-1", "Thư dở dang", OutboxState::Draft),
                outbox_record("d-2", "Thư bị ngắt", OutboxState::Unknown),
                outbox_record(
                    "d-3",
                    "Thư lỗi",
                    OutboxState::Failed {
                        reason: FailReason::Network,
                        retryable: true,
                    },
                ),
                outbox_record("d-4", "", OutboxState::Sent),
            ];
        });
        assert!(has(&html, "screen-outbox") && !has(&html, "screen-inbox"));
        for (id, state, label) in [
            ("d-1", "draft", "Bản nháp"),
            ("d-2", "unknown", "Có thể đã gửi"),
            ("d-3", "failed", "Chưa gửi được"),
            ("d-4", "sent", "Đã gửi"),
        ] {
            let item = tag(&html, &format!("outbox-item-{id}"));
            assert!(item.contains(&format!("data-state=\"{state}\"")), "{id}");
            assert!(html.contains(label));
            assert!(has(&html, &format!("outbox-open-{id}")));
        }
        assert!(html.contains("Thư dở dang") && html.contains("(Không có tiêu đề)"));
        assert!(html.contains("linh@company.vn") || html.contains("Linh"));
        assert!(
            html.contains("chỉ lưu trên máy này"),
            "says drafts are local, not on the server"
        );

        let empty = render_real("/inbox", |app| {
            app.filter.set(Filter {
                folder: Folder::Drafts,
                ..Filter::default()
            });
            OUTBOX.write().items_loaded = true;
        });
        assert!(has(&empty, "outbox-empty"));
    }

    #[test]
    fn demo_drafts_folder_is_not_the_real_outbox() {
        use crate::state::mail::{Filter, Folder};
        fn root() -> Element {
            let mut app = use_context_provider(AppState::init);
            use_hook(move || {
                app.filter.set(Filter {
                    folder: Folder::Drafts,
                    ..Filter::default()
                });
                OUTBOX.write().items = vec![outbox_record("d-9", "Thật", OutboxState::Draft)];
            });
            rsx! { Router::<crate::routes::Route> {} }
        }
        let html = render(root);
        assert!(has(&html, "screen-inbox") && !has(&html, "screen-outbox"));
        assert!(
            !html.contains("outbox-item-d-9"),
            "real drafts never show in demo mode"
        );
    }

    // ---- Track E (MB-200 search, MB-201 conversations) ----

    type TrackESetup = Box<dyn Fn(&mut crate::state::AppState, crate::state::search::SearchState)>;
    thread_local! {
        static TRACK_E_SETUP: std::cell::RefCell<Option<TrackESetup>> = const { std::cell::RefCell::new(None) };
    }

    fn track_e_message(id: &str, folder: &str, subject: &str) -> mailbox_contracts::MessageSummary {
        let mut m = real_message(id, folder, false);
        m.account_id = "acc-e".into();
        m.subject = subject.into();
        m
    }

    /// Real mode, one account `acc-e`, with `messages` and a hook to set filter/search state.
    fn track_e_render(
        messages: Vec<mailbox_contracts::MessageSummary>,
        setup: impl Fn(&mut crate::state::AppState, crate::state::search::SearchState) + 'static,
    ) -> String {
        use crate::state::AppState;
        use crate::state::folders::RealState;
        use crate::state::search::SearchState;
        use mailbox_contracts::{AccountStatus, AccountSummary, BootstrapResponse};
        thread_local! {
            static MESSAGES: std::cell::RefCell<Vec<mailbox_contracts::MessageSummary>> = const { std::cell::RefCell::new(Vec::new()) };
        }
        MESSAGES.with(|m| *m.borrow_mut() = messages);
        TRACK_E_SETUP.with(|s| *s.borrow_mut() = Some(Box::new(setup)));
        fn root() -> Element {
            let mut app = use_context_provider(AppState::init);
            use_context_provider(RealState::init);
            let search = use_context_provider(SearchState::init);
            use_hook(move || {
                app.enter_real_with(BootstrapResponse {
                    mode: "real".into(),
                    accounts: vec![AccountSummary {
                        id: "acc-e".into(),
                        name: "example.net".into(),
                        address: "me@example.net".into(),
                        status: AccountStatus::Connected,
                        unread: 0,
                        color: "#2672F6".into(),
                        last_synced_at: None,
                    }],
                    messages: MESSAGES.with(|m| m.borrow().clone()),
                });
                app.selected.set(None);
                TRACK_E_SETUP.with(|s| {
                    if let Some(f) = s.borrow().as_ref() {
                        f(&mut app, search);
                    }
                });
            });
            rsx! { Router::<crate::routes::Route> {} }
        }
        render(root)
    }

    /// MB-201: in Real mode a conversation is one row (its newest message in this folder) with
    /// the size of the whole conversation, Sent reply included; the Sent copy is not an inbox row.
    #[test]
    fn track_e_real_conversation_is_one_row_with_its_size() {
        crate::adapters::history::seed_threads(
            &[
                ("e-s1", "acc-e:t1"),
                ("e-r1", "acc-e:t1"),
                ("e-r0", "acc-e:t1"),
            ],
            &[("acc-e:t1", 3)],
        );
        let html = track_e_render(
            vec![
                track_e_message("e-s1", "sent", "Re: Kế hoạch"),
                track_e_message("e-r1", "inbox", "Re: Kế hoạch"),
                track_e_message("e-x", "inbox", "Khác"),
                track_e_message("e-r0", "inbox", "Kế hoạch"),
            ],
            |_, _| {},
        );
        assert_eq!(html.matches("data-row=\"true\"").count(), 2, "{html}");
        let row = html
            .find("data-testid=\"message-row-e-r1\"")
            .expect("conversation row");
        let tail: String = html[row..].chars().take(400).collect();
        assert!(tail.contains("data-thread-count=\"3\""));
        assert!(
            !has(&html, "message-row-e-r0"),
            "older member is inside the row"
        );
        assert!(
            !has(&html, "message-row-e-s1"),
            "Sent copy is not an inbox row"
        );
        assert!(has(&html, "list-load-older"), "the list offers older mail");
        assert!(!has(&html, "search-bar"));
    }

    /// MB-200: a query in Real mode shows the store's results with scope switch, count, clear
    /// control and highlighted, escaped matches.
    #[test]
    fn track_e_search_results_show_scope_count_and_escaped_highlights() {
        use crate::state::search::{SearchResult, SearchScope, request_key};
        let hits = vec![
            track_e_message(
                "e-h1",
                "inbox",
                "Hóa đơn tháng 11 <script>alert(1)</script>",
            ),
            track_e_message("e-h2", "acc-e/Projects", "hoa don cu"),
        ];
        let html = track_e_render(Vec::new(), move |app, search| {
            app.filter.write().query = "hoa don".into();
            let f = app.filter.peek().clone();
            let (mut result, hits) = (search.result, hits.clone());
            result.set(Some(SearchResult {
                request: request_key(&f, SearchScope::View, &Default::default()),
                messages: hits,
                total: 7,
                failed: false,
            }));
        });
        for id in [
            "search-bar",
            "search-scope-view",
            "search-scope-account",
            "search-scope-all",
            "search-clear",
            "search-result-e-h1",
            "search-result-e-h2",
        ] {
            assert!(has(&html, id), "missing {id}");
        }
        assert!(html.contains("7 kết quả · hiển thị 2 mới nhất"));
        assert!(html.contains("<mark class=\"search-hit\">Hóa</mark>"));
        assert!(html.contains("<mark class=\"search-hit\">hoa</mark>"));
        assert!(!html.contains("<script>"), "mail text is never markup");
        assert!(
            html.contains("&#60;script&#62;"),
            "rendered as escaped text"
        );
        let view = html.find("data-testid=\"search-scope-view\"").unwrap();
        let tail: String = html[view..].chars().take(200).collect();
        assert!(tail.contains("aria-checked=\"true\""));
        assert!(
            !has(&html, "list-load-older"),
            "no paging inside search results"
        );
    }

    /// MB-200: no match is an explicit empty state with a way out.
    #[test]
    fn track_e_search_without_results_says_so() {
        use crate::state::search::{SearchResult, SearchScope, request_key};
        let html = track_e_render(
            vec![track_e_message("e-1", "inbox", "Xin chào")],
            |app, search| {
                app.filter.write().query = "không có".into();
                let f = app.filter.peek().clone();
                let mut result = search.result;
                result.set(Some(SearchResult {
                    request: request_key(&f, SearchScope::View, &Default::default()),
                    messages: Vec::new(),
                    total: 0,
                    failed: false,
                }));
            },
        );
        assert!(has(&html, "search-bar") && has(&html, "empty-state"));
        assert!(html.contains("0 kết quả"));
        assert!(html.contains("Không tìm thấy thư phù hợp"));
        assert!(
            !has(&html, "message-row-e-1"),
            "the folder list is not shown as results"
        );
        assert!(!has(&html, "search-pending"));
    }

    /// MB-200: while a search is still running the list says so instead of "no results".
    #[test]
    fn track_e_pending_search_is_not_an_empty_result() {
        let html = track_e_render(
            vec![track_e_message("e-1", "inbox", "Xin chào")],
            |app, _| {
                app.filter.write().query = "xin".into();
            },
        );
        assert!(has(&html, "search-bar") && has(&html, "search-pending"));
        assert!(!has(&html, "empty-state"));
        assert!(html.contains("data-state=\"busy\""));
    }

    /// MB-200: Demo mode keeps the in-memory search, without the real search bar.
    #[test]
    fn track_e_demo_search_stays_in_memory() {
        use crate::state::AppState;
        fn root() -> Element {
            let mut app = use_context_provider(AppState::init);
            use_context_provider(crate::state::search::SearchState::init);
            use_hook(move || app.filter.write().query = "greentech".into());
            rsx! { Router::<crate::routes::Route> {} }
        }
        let html = render(root);
        assert!(!has(&html, "search-bar"));
        assert!(has(&html, "message-row-m07"));
        assert_eq!(html.matches("data-row=\"true\"").count(), 1);
    }

    // ---- Track D: live sync status (MB-141) ----

    use crate::state::live::{AccountLive, Attention, LIVE, LinkState, LiveMode};

    /// Real mode at `path` with the one saved account `acc-x` in `state`.
    fn live_render(path: &'static str, live: AccountLive) -> String {
        render_real(path, move |_| {
            LIVE.write().insert("acc-x".into(), live.clone());
        })
    }

    fn live_at(state: LinkState) -> AccountLive {
        AccountLive {
            state,
            last_synced: Some(3_600 * 9 + 60 * 5),
            failed_folders: 0,
            generation: 1,
        }
    }

    /// The opening tag of the first element with this test id.
    fn live_tag<'a>(html: &'a str, id: &str) -> &'a str {
        let at = html
            .find(&format!("data-testid=\"{id}\""))
            .unwrap_or_else(|| panic!("missing {id}"));
        let start = html[..at].rfind('<').unwrap();
        &html[start..at + html[at..].find('>').unwrap()]
    }

    #[test]
    fn live_status_shows_every_state_on_the_account_and_in_the_top_bar() {
        let cases = [
            (LinkState::Connecting, "connecting", "Đang kết nối"),
            (LinkState::Syncing, "syncing", "Đang đồng bộ"),
            (
                LinkState::Online(LiveMode::Idle),
                "online-idle",
                "nhận thư ngay khi đến",
            ),
            (
                LinkState::Online(LiveMode::Polling),
                "online-polling",
                "kiểm tra thư mỗi 2 phút",
            ),
            (
                LinkState::Offline {
                    retry_at: 3_600 * 10,
                },
                "offline",
                "tự thử lại lúc 10:00 UTC",
            ),
            (
                LinkState::NeedsAttention(Attention::AuthFailed),
                "auth-failed",
                "từ chối mật khẩu",
            ),
            (
                LinkState::NeedsAttention(Attention::Certificate),
                "certificate",
                "Chứng chỉ máy chủ không hợp lệ",
            ),
            (
                LinkState::NeedsAttention(Attention::Storage),
                "storage",
                "Không dùng được dữ liệu",
            ),
        ];
        for (state, key, text) in cases {
            let html = live_render("/", live_at(state));
            assert!(
                live_tag(&html, "topbar-real-status").contains(&format!("data-live=\"{key}\"")),
                "{key}: top bar"
            );
            assert!(
                live_tag(&html, "topbar-live-status").contains(&format!("data-live=\"{key}\""))
            );
            assert!(
                live_tag(&html, "account-live-acc-x").contains(&format!("data-live=\"{key}\"")),
                "{key}: account row"
            );
            assert!(html.contains(text), "{key}: {text}");
            assert!(
                html.contains("Đồng bộ lần cuối 09:05 UTC"),
                "{key}: last sync"
            );
            assert_eq!(
                has(&html, "account-retry-acc-x"),
                state.can_retry(),
                "{key}: retry"
            );
            assert_eq!(
                has(&html, "account-reauth-acc-x"),
                key == "auth-failed",
                "{key}: reauth"
            );
            assert!(!html.contains("Chế độ demo") && !html.contains("topbar-demo-status"));
        }
    }

    #[test]
    fn live_online_is_never_claimed_before_a_sync_and_partial_failure_stays_on_the_account() {
        let html = live_render(
            "/",
            AccountLive {
                state: LinkState::Connecting,
                last_synced: None,
                failed_folders: 0,
                generation: 0,
            },
        );
        assert!(!html.contains("Trực tuyến") && !html.contains("Đã đồng bộ"));
        assert!(!html.contains("Đồng bộ lần cuối"));
        let html = live_render(
            "/",
            AccountLive {
                failed_folders: 2,
                ..live_at(LinkState::Online(LiveMode::Idle))
            },
        );
        assert!(has(&html, "account-partial-acc-x"));
        assert!(html.contains("2 thư mục chưa đồng bộ được"));
        assert!(
            live_tag(&html, "account-live-acc-x").contains("online-idle"),
            "the account still works"
        );
    }

    #[test]
    fn live_reauth_link_opens_the_connect_form_with_an_empty_password() {
        let html = live_render(
            "/",
            live_at(LinkState::NeedsAttention(Attention::AuthFailed)),
        );
        assert!(live_tag(&html, "account-reauth-acc-x").contains("button"));
        // The connect form the link opens never carries the old password.
        let form = live_render(
            "/connect",
            live_at(LinkState::NeedsAttention(Attention::AuthFailed)),
        );
        assert!(
            !form.contains("value=\"pw"),
            "no stored password is ever rendered"
        );
    }

    #[test]
    fn live_settings_sync_panel_shows_real_per_account_state() {
        let html = live_render(
            "/settings/sync",
            live_at(LinkState::Offline { retry_at: 0 }),
        );
        assert!(has(&html, "settings-live-status"));
        assert!(live_tag(&html, "account-live-acc-x").contains("data-live=\"offline\""));
        assert!(html.contains("me@example.com"));
        assert!(
            !html.contains("Chưa kích hoạt: bản demo"),
            "no demo copy in real mode"
        );
    }

    #[test]
    fn live_states_never_appear_in_demo_mode() {
        fn root() -> Element {
            use_hook(|| {
                dioxus::history::provide_history_context(std::rc::Rc::new(
                    dioxus::history::MemoryHistory::with_initial_path("/settings/sync"),
                ));
            });
            use_context_provider(AppState::init);
            use_hook(|| {
                // Even if a stale entry existed, demo mode must not show it.
                LIVE.write().insert(
                    "work".into(),
                    AccountLive {
                        state: LinkState::Online(LiveMode::Idle),
                        ..AccountLive::default()
                    },
                );
            });
            rsx! { Router::<crate::routes::Route> {} }
        }
        let html = render(root);
        assert!(has(&html, "topbar-demo-status"));
        for id in [
            "topbar-live-status",
            "account-live-work",
            "settings-live-status",
        ] {
            assert!(!has(&html, id), "{id} in demo mode");
        }
        assert!(html.contains("Chưa kích hoạt: bản demo"));
        assert!(!html.contains("Trực tuyến"));
    }

    // ---- Track F: settings persistence, templates, shortcuts, OAuth mock, autoconfig ----

    #[test]
    fn f_oauth_buttons_are_a_labelled_mock_without_inputs_or_links() {
        let html = render_at("/accounts/connect");
        assert!(has(&html, "oauth-mock-buttons"));
        assert!(has(&html, "oauth-mock-google") && has(&html, "oauth-mock-microsoft"));
        assert_eq!(
            html.matches("mock / chưa hoạt động thật").count(),
            2,
            "both buttons say mock"
        );
        assert!(
            !has(&html, "oauth-mock-panel"),
            "panel stays closed until clicked"
        );

        fn panel() -> Element {
            rsx! { crate::components::oauth_mock::OAuthMockPanel { provider: crate::components::oauth_mock::OAuthProvider::Google, onclose: |_| {} } }
        }
        let html = render(panel);
        assert!(has(&html, "oauth-mock-panel"));
        assert!(html.contains("Mock / chưa hoạt động thật"));
        assert!(
            html.contains("OAuth") && html.contains("client ID"),
            "says what is needed"
        );
        assert!(html.contains("mật khẩu ứng dụng"), "offers the real path");
        assert!(html.contains("không có kết nối mạng nào được thực hiện"));
        for forbidden in ["<input", "<form", "href=", "type=\"password\"", "token\""] {
            assert!(
                !html.contains(forbidden),
                "mock panel must not contain {forbidden}"
            );
        }
    }

    fn f_detection() -> mailbox_autoconfig::Detection {
        use mailbox_autoconfig::{Detection, Endpoint, Found, Security, Source};
        Detection {
            imap: Some(Found {
                endpoint: Endpoint {
                    host: "imap.corp.vn".into(),
                    port: 993,
                    security: Security::ImplicitTls,
                },
                source: Source::DnsSrv,
            }),
            smtp: Some(Found {
                endpoint: Endpoint {
                    host: "smtp.corp.vn".into(),
                    port: 587,
                    security: Security::StartTls,
                },
                source: Source::AutoconfigSubdomain,
            }),
            provider: None,
            hint: None,
            notes: vec!["Chỉ phát hiện được một phần cấu hình; điền phần còn lại thủ công."],
        }
    }

    #[test]
    fn f_autoconfig_result_shows_values_sources_and_waits_for_the_user() {
        fn view() -> Element {
            rsx! { crate::components::autoconfig::AutoconfigResult { detection: f_detection(), domain: "corp.vn".to_string(), onapply: |_| {} } }
        }
        let html = render(view);
        assert!(has(&html, "autoconfig-result"));
        assert!(html.contains("imap.corp.vn:993") && html.contains("SSL/TLS"));
        assert!(html.contains("smtp.corp.vn:587") && html.contains("STARTTLS"));
        assert!(
            html.contains("data-source=\"dns-srv\"")
                && html.contains("data-source=\"autoconfig-subdomain\"")
        );
        assert!(html.contains("Bản ghi DNS SRV") && html.contains("autoconfig."));
        assert!(
            html.contains("Chỉ tên miền") && html.contains("corp.vn"),
            "states that only the domain is used"
        );
        assert!(html.contains("chưa có gì được lưu"));
        assert!(has(&html, "autoconfig-apply"));
        assert!(
            !html.contains("@"),
            "no email address anywhere in the result"
        );

        fn empty() -> Element {
            rsx! { crate::components::autoconfig::AutoconfigResult { detection: mailbox_autoconfig::Detection::default(), domain: "x.vn".to_string(), onapply: |_| {} } }
        }
        let html = render(empty);
        assert!(html.contains("Nhập thủ công") && !has(&html, "autoconfig-apply"));
    }

    #[test]
    fn f_autoconfig_button_is_real_mode_only_and_demo_keeps_its_scenarios() {
        let demo = render_at("/accounts/connect");
        assert!(!has(&demo, "autoconfig-run"));
        let real = render_real("/accounts/connect", |_| {});
        if crate::adapters::connect::REAL_CHECK {
            assert!(has(&real, "autoconfig-run"));
            assert!(real.contains("Chỉ tên miền được dùng"));
        }
    }

    #[test]
    fn f_real_settings_show_persistence_controls_and_demo_does_not() {
        use crate::state::prefs::{PREFS, SAVED};
        for (path, ids) in [
            (
                "/settings/appearance",
                vec![
                    "settings-density-compact",
                    "settings-theme-dark",
                    "settings-save-state",
                    "settings-toggle-reduce-motion",
                ],
            ),
            (
                "/settings/notifications",
                vec![
                    "settings-notify-off",
                    "settings-notify-count",
                    "settings-notify-detail",
                    "settings-toggle-sound",
                ],
            ),
            (
                "/settings/signature",
                vec![
                    "settings-signature-acc-x",
                    "settings-signature-reply-acc-x",
                    "settings-template-new",
                    "settings-templates-empty",
                ],
            ),
            ("/settings/security", vec!["settings-remote-images"]),
            (
                "/settings/advanced",
                vec!["settings-toggle-shortcuts", "settings-shortcut-help"],
            ),
        ] {
            let html = render_real(path, |_| {});
            for id in ids {
                assert!(has(&html, id), "{path} missing {id}");
            }
        }
        // Remote images stay blocked: the switch is off and disabled.
        let html = render_real("/settings/security", |_| {});
        assert!(html.contains("aria-checked=\"false\"") && html.contains("disabled"));
        // Dirty marker follows unsaved edits.
        let clean = render_real("/settings/appearance", |_| {});
        assert!(clean.contains("data-dirty=\"false\""));
        let dirty = render_real("/settings/appearance", |_| {
            *SAVED.write() = mailbox_core::prefs::Preferences::default();
            PREFS.write().density = mailbox_core::prefs::Density::Compact;
        });
        assert!(dirty.contains("data-dirty=\"true\"") && dirty.contains("Có thay đổi chưa lưu"));
        // Demo: in-memory settings, no persistence controls, no storage claim.
        let demo = render_at("/settings/appearance");
        assert!(!has(&demo, "settings-density-compact") && !has(&demo, "settings-save-state"));
        assert!(demo.contains("Không có gì được ghi ra đĩa"));
    }

    #[test]
    fn f_composer_template_rail_uses_stored_templates_in_real_mode_only() {
        use crate::state::prefs::USER_TEMPLATES;
        let real = render_real("/compose", |_| {
            *USER_TEMPLATES.write() = vec![mailbox_core::prefs::Template {
                id: "t1".into(),
                name: "Cảm ơn khách".into(),
                subject: "Cảm ơn {{first_name}}".into(),
                body: "b".into(),
            }];
        });
        assert!(
            has(&real, "compose-template-t1")
                && real.contains("Cảm ơn khách")
                && real.contains("Mẫu thư của bạn")
        );
        assert!(
            !has(&real, "compose-template-followup"),
            "fixture templates never appear in real mode"
        );
        let empty = render_real("/compose", |_| {
            USER_TEMPLATES.write().clear();
        });
        assert!(has(&empty, "compose-templates-empty"));
        let demo = render_at("/compose");
        assert!(!demo.contains("Mẫu thư của bạn"));
    }

    #[test]
    fn f_shortcut_help_lists_every_binding_with_vietnamese_labels() {
        fn view() -> Element {
            rsx! { crate::components::shortcuts::ShortcutHelp {} }
        }
        let html = render(view);
        assert!(has(&html, "shortcut-help"));
        assert_eq!(
            html.matches("data-testid=\"shortcut-row\"").count(),
            crate::state::shortcuts::BINDINGS.len()
        );
        for text in [
            "Soạn thư mới",
            "Trả lời tất cả",
            "Chuyển tiếp",
            "Lưu trữ",
            "Gắn / bỏ sao",
            "Đánh dấu chưa đọc",
            "Ctrl/Cmd + K",
            "Ctrl/Cmd + Enter",
            "Esc",
        ] {
            assert!(html.contains(text), "help is missing {text}");
        }
        assert!(html.contains("đang gõ"), "explains the typing rule");
    }

    // ---- Track G (MB-218 scheduled send, compose gaps) ----

    fn track_g_scheduled_record(id: &str, at: i64) -> mailbox_core::outbox::DraftRecord {
        mailbox_core::outbox::DraftRecord {
            id: id.into(),
            owner_id: "local".into(),
            account_id: "acc-x".into(),
            message_id: format!("<{id}@example.com>"),
            from_identity: "me@example.com".into(),
            to: vec!["Linh <linh@company.vn>".into()],
            subject: "Hẹn giờ".into(),
            state: mailbox_core::outbox::OutboxState::Scheduled,
            scheduled_at: Some(at),
            ..Default::default()
        }
    }

    #[test]
    fn track_g_real_composer_offers_scheduling_and_demo_does_not() {
        let real = compose_in(crate::state::outbox::SendPhase::Idle);
        let button = tag(&real, "compose-schedule-open");
        assert!(!button.contains("disabled"), "{button}");
        assert!(real.contains("Hẹn giờ gửi"));
        assert!(
            !real.contains("sau MVP (MB-218)"),
            "the old placeholder is gone"
        );
        let demo = render_at("/compose");
        let demo_button = tag(&demo, "compose-schedule-open");
        assert!(demo_button.contains("disabled"));
        assert!(demo.contains("Hẹn giờ gửi (demo)"));
        assert!(!has(&demo, "compose-scheduled"));
    }

    #[test]
    fn track_g_scheduled_draft_is_locked_and_states_the_rule() {
        use crate::state::schedule::{CLOSED_APP_NOTE, RULE_TEXT};
        let at = 1_800_000_000;
        let html = compose_in(crate::state::outbox::SendPhase::Scheduled { at: Some(at) });
        assert!(tag(&html, "compose-send-state").contains("data-state=\"scheduled\""));
        assert!(has(&html, "compose-scheduled"));
        assert!(html.contains("Đã hẹn giờ gửi:"));
        assert!(html.contains(RULE_TEXT), "the missed-time rule is stated");
        assert!(
            html.contains(CLOSED_APP_NOTE),
            "a closed app cannot send: said so"
        );
        assert!(has(&html, "compose-schedule-cancel") && has(&html, "compose-schedule-send-now"));
        assert!(
            tag(&html, "compose-send").contains("disabled"),
            "no second send"
        );
        assert!(tag(&html, "compose-schedule-open").contains("disabled"));
        assert!(
            !html.contains("Đã gửi thư"),
            "scheduled is never shown as sent"
        );
    }

    #[test]
    fn track_g_outbox_lists_scheduled_items_with_time_and_cancel() {
        use crate::state::mail::{Filter, Folder};
        let html = render_real("/inbox", |app| {
            app.filter.set(Filter {
                folder: Folder::Drafts,
                ..Filter::default()
            });
            let mut ui = OUTBOX.write();
            ui.items_loaded = true;
            ui.items = vec![track_g_scheduled_record("d-s", 1_800_000_000)];
        });
        assert!(tag(&html, "outbox-item-d-s").contains("data-state=\"scheduled\""));
        assert!(html.contains("Đã hẹn giờ"));
        assert!(has(&html, "outbox-scheduled-at-d-s") && has(&html, "outbox-unschedule-d-s"));
        assert!(has(&html, "outbox-schedule-rule"));
    }

    #[test]
    fn track_g_a_missed_schedule_waits_for_the_user_with_a_retry() {
        let html = compose_in(SendPhase::Failed {
            reason: FailReason::MissedSchedule,
            retryable: true,
            rejected: Vec::new(),
        });
        assert!(tag(&html, "compose-send-state").contains("data-reason=\"missed_schedule\""));
        assert!(html.contains("Đã quá giờ hẹn hơn 12 giờ"));
        assert!(has(&html, "compose-send-retry"));
    }

    #[test]
    fn track_g_auto_sync_setting_is_bound_to_the_stored_preference() {
        let on = render_real("/settings/sync", |_| {
            *crate::state::prefs::PREFS.write() = mailbox_core::prefs::Preferences::default();
        });
        assert!(tag(&on, "settings-auto-sync").contains("data-on=\"true\""));
        assert!(tag(&on, "settings-toggle-auto-sync").contains("aria-checked=\"true\""));
        assert!(!has(&on, "settings-auto-sync-off"));
        let off = render_real("/settings/sync", |_| {
            *crate::state::prefs::PREFS.write() = mailbox_core::prefs::Preferences {
                auto_sync: false,
                ..Default::default()
            };
        });
        assert!(tag(&off, "settings-toggle-auto-sync").contains("aria-checked=\"false\""));
        assert!(has(&off, "settings-auto-sync-off"));
        assert!(
            off.contains("Bấm Đồng bộ để lấy thư khi cần"),
            "manual sync still offered"
        );
        // Demo keeps its in-memory toggle and never touches the stored setting.
        let demo = render_at("/settings/sync");
        assert!(!has(&demo, "settings-auto-sync"));
        assert!(has(&demo, "settings-toggle-auto-sync"));
    }

    fn track_g_forward_compose(
        status: crate::state::outbox::ForwardFiles,
        with_file: bool,
    ) -> String {
        render_real("/compose", move |app| {
            use crate::state::compose::{DraftAttachment, ReplyKind};
            let mut d = Draft::new("acc-x");
            d.subject = "Fwd: Báo giá".into();
            d.reply_source = Some(("acc-x/INBOX#7#5".into(), ReplyKind::Forward));
            if with_file {
                d.attachments.push(DraftAttachment {
                    name: "báo giá.pdf".into(),
                    size: 2048,
                    path: None,
                    forwarded: Some(mailbox_core::outbox::ForwardedPart {
                        message_id: "acc-x/INBOX#7#5".into(),
                        part: 1,
                    }),
                });
            }
            app.draft.set(d);
            *crate::state::outbox::FORWARD_FILES.write() = Some(("acc-x/INBOX#7#5".into(), status));
        })
    }

    #[test]
    fn track_g_forward_carries_the_original_files_or_says_why_not() {
        use crate::state::outbox::ForwardFiles;
        let added = track_g_forward_compose(ForwardFiles::Added(1), true);
        assert!(tag(&added, "compose-forward-files").contains("data-state=\"added\""));
        assert!(tag(&added, "compose-attachment").contains("data-forwarded=\"true\""));
        assert!(added.contains("Chuyển tiếp từ thư gốc"));
        assert!(
            !added.contains("Không có trên máy này"),
            "a forwarded part is not shown as a missing file"
        );
        let missing = track_g_forward_compose(ForwardFiles::NotDownloaded, false);
        assert!(tag(&missing, "compose-forward-files").contains("data-state=\"not-downloaded\""));
        assert!(missing.contains("chưa được tải về máy này"));
        let none = track_g_forward_compose(ForwardFiles::NoFiles, false);
        assert!(!has(&none, "compose-forward-files"));
    }

    // ---- MB-304 / AUDIT S-09: links in mail ask before they open -----------------------------

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn links_in_the_mail_frame_are_inert() {
        let html = unescape(&render_pane(
            BodyState::Loaded(hostile_view()),
            false,
            false,
        ));
        assert!(
            html.contains("href=\"mailbox-link:"),
            "the allowed link is shown, guarded"
        );
        for live in ["href=\"http", "href=\"mailto", "example.org"] {
            assert!(!html.contains(live), "{live} is still a live link");
        }
    }

    thread_local! {
        static LINK_CASE: std::cell::Cell<(&'static str, bool)> = const { std::cell::Cell::new(("", false)) };
    }

    /// The link dialog after a click on `url` in a message (English when `english`).
    fn link_dialog(url: &'static str, english: bool) -> String {
        LINK_CASE.with(|c| c.set((url, english)));
        fn root() -> Element {
            use_hook(|| {
                let (url, english) = LINK_CASE.with(|c| c.get());
                if english {
                    crate::i18n::set_lang(crate::i18n::Lang::En);
                }
                // The same path a real click takes: guarded markup, then the blocked navigation.
                let guarded = mailbox_core::link::guard_links(&format!("<a href=\"{url}\">x</a>"));
                let start = guarded.find("mailbox-link:").expect("guarded");
                let end = start + guarded[start..].find('"').unwrap();
                crate::state::link::request(&guarded[start..end]);
            });
            rsx! { crate::components::link_confirm::LinkConfirmHost {} }
        }
        render(root)
    }

    /// Text of the element with `data-testid` = `id`.
    fn text_of(html: &str, id: &str) -> String {
        let at = html
            .find(&format!("data-testid=\"{id}\""))
            .unwrap_or_else(|| panic!("{id} missing in {html}"));
        let open = at + html[at..].find('>').unwrap() + 1;
        let close = open + html[open..].find('<').unwrap();
        unescape(&html[open..close])
    }

    #[test]
    fn a_clicked_link_shows_its_real_address_and_waits() {
        let html = link_dialog("https://Example.org/pay?id=7", false);
        assert!(has(&html, "link-confirm") && html.contains("aria-modal=\"true\""));
        assert_eq!(text_of(&html, "link-confirm-host"), "example.org");
        assert_eq!(
            text_of(&html, "link-confirm-url"),
            "https://example.org/pay?id=7"
        );
        assert!(has(&html, "link-confirm-open") && has(&html, "link-confirm-cancel"));
        for absent in [
            "link-confirm-insecure",
            "link-confirm-lookalike",
            "link-refused",
        ] {
            assert!(!has(&html, absent), "{absent}");
        }
        assert!(html.contains("Mở liên kết này?"));
    }

    #[test]
    fn risky_links_carry_a_warning() {
        let plain = link_dialog("http://example.org/", false);
        assert!(has(&plain, "link-confirm-insecure") && has(&plain, "link-confirm-open"));
        // Cyrillic "а": shown in its xn-- form, with a look-alike warning.
        let idn = link_dialog("https://\u{430}pple.com/", false);
        assert!(has(&idn, "link-confirm-lookalike"));
        assert!(text_of(&idn, "link-confirm-host").starts_with("xn--"));
        let mail = link_dialog("mailto:linh@company.vn", false);
        assert!(mail.contains("data-kind=\"mail\""));
        assert_eq!(text_of(&mail, "link-confirm-host"), "linh@company.vn");
    }

    #[test]
    fn deceptive_links_are_refused_without_an_open_button() {
        let html = link_dialog("https://bank.example@evil.example/login", false);
        assert!(has(&html, "link-refused") && html.contains("data-reason=\"credentials\""));
        assert!(!has(&html, "link-confirm-open") && !has(&html, "link-confirm"));
        let scheme = link_dialog("file:///etc/passwd", false);
        assert!(scheme.contains("data-reason=\"unsupported\""));
        assert!(!has(&scheme, "link-confirm-open"));
    }

    #[test]
    fn the_link_dialog_is_translated_and_part_of_the_shell() {
        let en = link_dialog("http://example.org/", true);
        assert!(en.contains("Open this link?") && !en.contains("Mở liên kết"));
        assert!(
            !has(&render(crate::app::App), "link-confirm"),
            "no dialog without a click"
        );
        fn with_click() -> Element {
            use_hook(|| {
                let guarded =
                    mailbox_core::link::guard_links("<a href=\"https://example.org/\">x</a>");
                let start = guarded.find("mailbox-link:").unwrap();
                let end = start + guarded[start..].find('"').unwrap();
                crate::state::link::request(&guarded[start..end]);
            });
            rsx! { crate::app::App {} }
        }
        assert!(has(&render(with_click), "link-confirm"));
    }

    // ---- MB-105 / MB-221: interface language -------------------------------------------------

    thread_local! {
        static I18N_PATH: std::cell::Cell<&'static str> = const { std::cell::Cell::new("/") };
        static I18N_LANG: std::cell::Cell<crate::i18n::Lang> = const { std::cell::Cell::new(crate::i18n::Lang::Vi) };
    }

    /// The demo app at `path` in `lang` (the language is set before the first component renders).
    fn i18n_render_demo(path: &'static str, lang: crate::i18n::Lang) -> String {
        I18N_PATH.with(|p| p.set(path));
        I18N_LANG.with(|l| l.set(lang));
        fn root() -> Element {
            use_hook(|| {
                dioxus::history::provide_history_context(std::rc::Rc::new(
                    dioxus::history::MemoryHistory::with_initial_path(I18N_PATH.with(|p| p.get())),
                ));
                crate::i18n::set_lang(I18N_LANG.with(|l| l.get()));
            });
            rsx! { crate::app::App {} }
        }
        render(root)
    }

    /// The real-mode app (one saved account, no mail) at `path` in English.
    fn i18n_render_real_en(path: &'static str) -> String {
        render_real(path, |_| crate::i18n::set_lang(crate::i18n::Lang::En))
    }

    /// Every string of the demo data that is Vietnamese by design (fixture mail is not translated).
    fn i18n_fixture_strings() -> Vec<String> {
        let data = mailbox_contracts::fixtures::bootstrap();
        let mut out: Vec<String> = Vec::new();
        for a in &data.accounts {
            out.extend([a.name.clone(), a.address.clone()]);
        }
        for m in &data.messages {
            out.extend([
                m.from_name.clone(),
                m.subject.clone(),
                m.preview.clone(),
                m.body_plain.clone(),
                m.to_line.clone(),
                m.received_at_full.clone(),
            ]);
            out.extend(m.labels.iter().cloned());
            out.extend(m.attachments.iter().map(|a| a.filename.clone()));
        }
        for c in crate::state::demo::CONTACTS.iter() {
            out.push(c.name.to_string());
        }
        // The language names are written in their own language on purpose; "Nguyễn" is a name.
        out.extend(["Tiếng Việt".to_string(), "Minh Nguyễn".to_string()]);
        out.retain(|s| !s.is_ascii());
        out.sort_by_key(|s| std::cmp::Reverse(s.len()));
        out
    }

    /// Vietnamese letters left in `html` once the demo fixture text is removed.
    fn i18n_leftover_vietnamese(html: &str) -> Vec<String> {
        let escape = |s: &str| {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&#39;")
        };
        let mut rest = html.to_string();
        for f in i18n_fixture_strings() {
            rest = rest.replace(&f, "").replace(&escape(&f), "");
        }
        let mut left = Vec::new();
        let chars: Vec<char> = rest.chars().collect();
        for (i, c) in chars.iter().enumerate() {
            if !c.is_ascii() && c.is_alphabetic() {
                let from = i.saturating_sub(30);
                let to = (i + 30).min(chars.len());
                left.push(chars[from..to].iter().collect::<String>());
            }
        }
        left
    }

    #[test]
    fn i18n_default_language_is_vietnamese() {
        let html = render_at("/");
        assert!(html.contains("lang=\"vi\""), "document language is vi");
        assert!(html.contains("Chế độ demo"));
    }

    #[test]
    fn i18n_english_demo_shell_and_inbox_have_no_vietnamese_ui_text() {
        use crate::i18n::Lang::En;
        let html = i18n_render_demo("/", En);
        assert!(html.contains("lang=\"en\""));
        for text in [
            "Demo mode",
            "Unified inbox",
            "Compose",
            "Skip to main content",
            "Sample data",
        ] {
            assert!(html.contains(text), "English UI is missing {text:?}");
        }
        // The demo label stays visible in English.
        assert!(has(&html, "topbar-demo-status"));
        // Checked list of Vietnamese UI phrases that must be gone.
        for vi in [
            "Chế độ demo",
            "Hộp thư hợp nhất",
            "Soạn thư",
            "Cài đặt",
            "Thử lại",
            "Tìm kiếm",
            "Thêm tài khoản",
            "Dữ liệu minh họa",
            "Thư gắn sao",
            "Bỏ qua đến nội dung chính",
            "Chưa đọc",
        ] {
            assert!(
                !html.contains(vi),
                "Vietnamese {vi:?} left in the English shell"
            );
        }
        let left = i18n_leftover_vietnamese(&html);
        assert!(left.is_empty(), "Vietnamese text left: {left:#?}");
    }

    #[test]
    fn i18n_english_demo_screens_have_no_vietnamese_ui_text() {
        use crate::i18n::Lang::En;
        for path in [
            "/messages/m11/thread",
            "/compose",
            "/accounts/connect",
            "/settings",
            "/settings/sync",
            "/settings/appearance",
            "/settings/security",
            "/settings/signature",
            "/settings/notifications",
            "/settings/integrations",
            "/settings/advanced",
            "/settings/about",
            "/no/such/page",
        ] {
            let html = i18n_render_demo(path, En);
            let left = i18n_leftover_vietnamese(&html);
            assert!(left.is_empty(), "{path}: Vietnamese text left: {left:#?}");
        }
    }

    #[test]
    fn i18n_english_real_screens_have_no_vietnamese_ui_text() {
        for path in [
            "/",
            "/compose",
            "/accounts/connect",
            "/settings",
            "/settings/sync",
            "/settings/appearance",
            "/settings/security",
            "/settings/signature",
            "/settings/notifications",
            "/settings/advanced",
        ] {
            let html = i18n_render_real_en(path);
            assert!(html.contains("lang=\"en\""), "{path}: not English");
            let left = i18n_leftover_vietnamese(&html);
            assert!(left.is_empty(), "{path}: Vietnamese text left: {left:#?}");
        }
    }

    #[test]
    fn i18n_settings_offer_a_language_choice_in_both_modes_and_mark_the_current_one() {
        use crate::i18n::Lang::{En, Vi};
        let demo_vi = i18n_render_demo("/settings/appearance", Vi);
        assert!(has(&demo_vi, "settings-lang-vi") && has(&demo_vi, "settings-lang-en"));
        assert!(demo_vi.contains("Chọn ngôn ngữ giao diện"));
        let demo_en = i18n_render_demo("/settings/appearance", En);
        assert!(demo_en.contains("Choose the interface language"));
        // The names of the languages are the same in every language.
        assert!(demo_en.contains("Tiếng Việt") && demo_en.contains("English"));
        let real_en = i18n_render_real_en("/settings/appearance");
        assert!(has(&real_en, "settings-lang-en"));
        let on = real_en.find("data-testid=\"settings-lang-en\"").unwrap();
        let tag_start = real_en[..on].rfind("<button").unwrap();
        assert!(
            real_en[tag_start..on].contains("aria-checked=\"true\""),
            "English is marked as chosen"
        );
    }

    #[test]
    fn i18n_counts_and_labels_follow_the_language() {
        use crate::i18n::Lang::{En, Vi};
        let vi = i18n_render_demo("/", Vi);
        let en = i18n_render_demo("/", En);
        assert!(vi.contains("aria-label=\"Hộp thư hợp nhất, 5 chưa đọc\""));
        assert!(en.contains("aria-label=\"Unified inbox, 5 unread\""));
        assert!(en.contains("aria-label=\"Account Work, minh@company.vn, 4 unread\""));
        let thread_en = i18n_render_demo("/messages/m11/thread", En);
        assert!(
            thread_en.contains("messages in the conversation")
                || thread_en.contains("message in the conversation")
        );
    }

    #[test]
    fn i18n_dates_follow_the_language() {
        fn en_root() -> Element {
            use_hook(|| crate::i18n::set_lang(crate::i18n::Lang::En));
            use crate::i18n::when;
            rsx! {
                p { "data-testid": "when-a", {when("Thứ Năm, 12/11/2026 16:05")} }
                p { "data-testid": "when-b", {when("Hôm qua")} }
                p { "data-testid": "when-c", {when("01/11")} }
                p { "data-testid": "when-d", {when("14/11/2026 03:04 UTC")} }
                p { "data-testid": "when-e", {when("09:12")} }
                p { "data-testid": "when-f", {when("31/13/2026")} }
            }
        }
        let html = render(en_root);
        for expected in [
            ">Thu, 12 Nov 2026 16:05<",
            ">Yesterday<",
            ">1 Nov<",
            ">14 Nov 2026 03:04 UTC<",
            ">09:12<",
            ">31/13/2026<",
        ] {
            assert!(html.contains(expected), "missing {expected} in {html}");
        }
        use crate::i18n::Lang::{En, Vi};
        assert!(i18n_render_demo("/", Vi).contains(">Hôm qua<"));
        assert!(i18n_render_demo("/", En).contains(">Yesterday<"));
        assert!(i18n_render_demo("/messages/m11/thread", En).contains("Nov 2026"));
    }
}
