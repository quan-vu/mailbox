//! Message bodies for the reader (MB-130, MB-131).
//!
//! The view types and the frame document are shared by Web and Desktop. Loading is Desktop only:
//! cache first, else one read-only IMAP fetch (`BODY.PEEK[]`), bounded MIME parsing and HTML
//! sanitising in `mailbox-mime`, then the safe result is cached. Attachment bytes are never kept;
//! saving one fetches the message again and writes a single file into the Downloads folder.

use crate::i18n::tr;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentView {
    pub part: u32,
    pub filename: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub inline: bool,
}

/// What the reader may show. `html` and `html_remote` are already sanitized.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BodyView {
    pub text: String,
    pub html: Option<String>,
    /// Variant that keeps remote `<img>` sources; used only after the explicit per-message action.
    pub html_remote: Option<String>,
    pub remote_blocked: bool,
    pub truncated: bool,
    pub attachments: Vec<AttachmentView>,
}

// The Web build has no body source yet, so most variants are only built on Desktop.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyFailure {
    /// Bigger than the size limit; never downloaded.
    TooLarge,
    Unreachable,
    Auth,
    /// The server certificate was not valid. Never bypassable.
    Security,
    /// Not on the server any more, or the folder was rebuilt (UIDVALIDITY changed).
    Gone,
    /// This build cannot read real message bodies (Web).
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    Unsupported,
    /// Could not write the file.
    Disk,
    Failed,
}

impl BodyFailure {
    pub fn key(self) -> &'static str {
        match self {
            BodyFailure::TooLarge => "too-large",
            BodyFailure::Unreachable => "unreachable",
            BodyFailure::Auth => "auth",
            BodyFailure::Security => "security",
            BodyFailure::Gone => "gone",
            BodyFailure::Unsupported => "unsupported",
            BodyFailure::Disk => "disk",
            BodyFailure::Failed => "failed",
        }
    }

    /// User-facing copy: what happened and the safe next step. No addresses, hosts or content.
    pub fn message(self) -> &'static str {
        match self {
            BodyFailure::TooLarge => tr(
                "Thư này lớn hơn 25 MB nên MailBox không tải về. Hãy mở thư bằng webmail của nhà cung cấp.",
            ),
            BodyFailure::Unreachable => {
                tr("Không kết nối được máy chủ thư. Kiểm tra mạng rồi thử lại.")
            }
            BodyFailure::Auth => {
                tr("Máy chủ từ chối đăng nhập. Hãy kết nối lại tài khoản trong phần Cài đặt.")
            }
            BodyFailure::Security => tr(
                "Chứng chỉ của máy chủ không hợp lệ nên MailBox đã dừng kết nối. Không có cách bỏ qua.",
            ),
            BodyFailure::Gone => {
                tr("Thư không còn trên máy chủ hoặc thư mục đã thay đổi. Hãy đồng bộ lại hộp thư.")
            }
            BodyFailure::Unsupported => tr("Bản Web chưa đọc được nội dung thư thật."),
            BodyFailure::Disk => {
                tr("Không ghi được tệp vào thư mục Tải về. Kiểm tra quyền ghi và dung lượng trống.")
            }
            BodyFailure::Failed => tr("Không đọc được nội dung thư này. Thử lại sau."),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyState {
    Loading,
    Loaded(BodyView),
    TooLarge,
    Failed(BodyFailure),
}

impl BodyState {
    pub fn key(&self) -> &'static str {
        match self {
            BodyState::Loading => "loading",
            BodyState::Loaded(_) => "loaded",
            BodyState::TooLarge => "too-large",
            BodyState::Failed(_) => "failed",
        }
    }

    pub fn from_result(result: Result<BodyView, BodyFailure>) -> Self {
        match result {
            Ok(view) => BodyState::Loaded(view),
            Err(BodyFailure::TooLarge) => BodyState::TooLarge,
            Err(e) => BodyState::Failed(e),
        }
    }
}

/// Policy of the frame that shows mail HTML: nothing loads, nothing runs.
pub const CSP_BLOCKED: &str = "default-src 'none'; img-src data: cid:; style-src 'unsafe-inline'";
/// After the explicit "show images" action for one message: remote images only.
pub const CSP_IMAGES: &str =
    "default-src 'none'; img-src data: cid: https: http:; style-src 'unsafe-inline'";

const FRAME_STYLE: &str = "html{color-scheme:light}body{margin:12px;font:14px/1.6 -apple-system,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;color:#1b2333;background:#fff;overflow-wrap:anywhere}img{max-width:100%;height:auto}table{max-width:100%}a{color:#1a5fd0}blockquote{margin:0 0 0 12px;padding-left:12px;border-left:3px solid #d5dbe6;color:#55617a}pre{white-space:pre-wrap}";

/// Sandbox of the mail frame: no scripts, no forms, no same-origin access, no pop-ups. The one
/// permission lets a click by the user (never the document by itself) navigate the top frame.
/// Every link in the frame is the inert `mailbox-link:` form, and the Desktop shell refuses
/// every top-frame navigation, so the click only reaches `adapters::link::gate`, which asks
/// the user. A navigation of the sandboxed frame itself is not reported by WKWebView at all
/// (checked on macOS), which is why links target the top frame.
pub const FRAME_SANDBOX: &str = "allow-top-navigation-by-user-activation";

/// Complete document for the sandboxed `iframe srcdoc`. `sanitized_html` must come from
/// `mailbox-mime`; the CSP here is the second layer, not a substitute for sanitising.
///
/// Every link is made inert here (`mailbox-link:`), also for bodies cached before this rule
/// existed: a click cannot open or load anything, it only asks the shell to show the address
/// for confirmation (AUDIT S-09).
pub fn frame_document(sanitized_html: &str, show_images: bool) -> String {
    let csp = if show_images { CSP_IMAGES } else { CSP_BLOCKED };
    let sanitized_html = mailbox_core::link::guard_links(sanitized_html);
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"{csp}\"><meta name=\"referrer\" content=\"no-referrer\"><base target=\"_top\"><style>{FRAME_STYLE}</style></head><body>{sanitized_html}</body></html>"
    )
}

#[cfg(target_arch = "wasm32")]
pub async fn load_body(_message_id: &str) -> Result<BodyView, BodyFailure> {
    Err(BodyFailure::Unsupported)
}

#[cfg(target_arch = "wasm32")]
pub async fn save_attachment(_message_id: &str, _part: u32) -> Result<String, BodyFailure> {
    Err(BodyFailure::Unsupported)
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{attachment_data, cached_attachments, load_body, save_attachment};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{AttachmentView, BodyFailure, BodyView};
    use crate::adapters::account::{OWNER, data_dir};
    use mailbox_core::body::{BodyRecord, BodyStore, MessageLocator};
    use mailbox_core::secrets::SecretStore;
    use mailbox_core::store::MailStore;
    use mailbox_core::{MailServerEndpoint, MailboxError, TlsMode};
    use mailbox_imap::ConnectError;
    use mailbox_imap::body::BodyError;
    use mailbox_mime::{Limits, MimeError};
    use mailbox_secrets::KeychainStore;
    use mailbox_store::SqliteStore;
    use std::future::Future;
    use std::io::Write;
    use std::path::{Path, PathBuf};

    fn view(record: BodyRecord) -> BodyView {
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

    fn storage(e: MailboxError) -> BodyFailure {
        match e {
            MailboxError::NotFound => BodyFailure::Gone,
            _ => BodyFailure::Failed,
        }
    }

    fn connect(e: ConnectError) -> BodyFailure {
        match e {
            ConnectError::AuthFailed | ConnectError::LoginDisabled => BodyFailure::Auth,
            ConnectError::Unreachable | ConnectError::Timeout => BodyFailure::Unreachable,
            ConnectError::CertificateInvalid | ConnectError::TlsFailed => BodyFailure::Security,
            _ => BodyFailure::Failed,
        }
    }

    fn mime(e: MimeError) -> BodyFailure {
        match e {
            MimeError::TooLarge | MimeError::AttachmentTooLarge => BodyFailure::TooLarge,
            MimeError::NoSuchAttachment => BodyFailure::Gone,
            _ => BodyFailure::Failed,
        }
    }

    /// Run CPU-heavy work on the blocking pool so the task that drives the UI keeps rendering
    /// (MB-304): MIME parsing, HTML sanitising and attachment decoding of up to 25 MB.
    pub(super) async fn off_ui<T, F>(work: F) -> Result<T, BodyFailure>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        tokio::task::spawn_blocking(work)
            .await
            .map_err(|_| BodyFailure::Failed)
    }

    async fn open_store() -> Result<SqliteStore, BodyFailure> {
        let dir = data_dir();
        std::fs::create_dir_all(&dir).map_err(|_| BodyFailure::Failed)?;
        SqliteStore::open(&dir.join("mailbox.sqlite3"))
            .await
            .map_err(storage)
    }

    /// EXAMINE, epoch check and `BODY.PEEK[]` on an open session. On failure also says whether
    /// the session is still in step with the server and may be used again.
    async fn fetch_on<S>(
        session: &mut mailbox_imap::Session<S>,
        locator: &MessageLocator,
        max_bytes: u64,
    ) -> Result<Vec<u8>, (BodyFailure, bool)>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let status = session
            .examine(&locator.folder_remote_name)
            .await
            .map_err(|e| (connect(e), false))?;
        // UID n of another epoch is a different message: never fetch across a UIDVALIDITY change.
        if status.uid_validity != Some(locator.uid_validity) {
            return Err((BodyFailure::Gone, true));
        }
        match session.fetch_body(locator.imap_uid, max_bytes).await {
            Ok(raw) => Ok(raw),
            // After these the session may be out of step with the server: just drop it.
            Err(BodyError::TooLarge { .. }) => Err((BodyFailure::TooLarge, false)),
            Err(BodyError::NotFound) => Err((BodyFailure::Gone, true)),
            Err(BodyError::Connect(e)) => Err((connect(e), false)),
        }
    }

    /// One read-only round trip: EXAMINE, size check, `BODY.PEEK[]`. Uses the account's live
    /// connection when its sync worker has one (MB-140), otherwise logs in for this fetch.
    async fn fetch_raw(
        store: &SqliteStore,
        locator: MessageLocator,
        max_bytes: u64,
    ) -> Result<Vec<u8>, BodyFailure> {
        if let Some(live) = crate::adapters::worker::handle(&locator.account_id)
            && let Some(mut rw) = live.session().await
        {
            match fetch_on(rw.reader_mut(), &locator, max_bytes).await {
                Ok(raw) => return Ok(raw),
                Err((failure, true)) => return Err(failure),
                Err((failure, false)) => {
                    rw.discard();
                    // A dead borrowed connection is retried once on a fresh one below.
                    if failure != BodyFailure::Unreachable {
                        return Err(failure);
                    }
                }
            }
        }
        let account = store
            .get_account(OWNER, &locator.account_id)
            .await
            .map_err(storage)?;
        // A missing keychain entry means the user removed it: the account needs re-authentication.
        let password = KeychainStore::default()
            .get(&account.credential_ref)
            .map_err(|_| BodyFailure::Auth)?;
        let endpoint = MailServerEndpoint {
            hostname: account.imap_host.clone(),
            port: account.imap_port,
            tls_mode: TlsMode::ImplicitTls,
        };
        let mut session =
            mailbox_imap::Session::connect(&endpoint, &account.email_address, &password)
                .await
                .map_err(connect)?;
        match fetch_on(&mut session, &locator, max_bytes).await {
            Ok(raw) => {
                session.logout().await;
                Ok(raw)
            }
            Err((failure, usable)) => {
                if usable {
                    session.logout().await;
                }
                Err(failure)
            }
        }
    }

    /// Cache first; otherwise fetch through `fetch`, parse within `limits`, cache the safe result.
    pub(super) async fn load_with<S, F, Fut>(
        store: &S,
        message_id: &str,
        limits: &Limits,
        fetch: F,
    ) -> Result<BodyRecord, BodyFailure>
    where
        S: BodyStore,
        F: FnOnce(MessageLocator) -> Fut,
        Fut: Future<Output = Result<Vec<u8>, BodyFailure>>,
    {
        if let Some(cached) = store.load_body(OWNER, message_id).await.map_err(storage)? {
            return Ok(cached);
        }
        let locator = store
            .locate_message(OWNER, message_id)
            .await
            .map_err(storage)?;
        // The size from the envelope sync: refuse before opening a connection at all.
        if locator.size_bytes > limits.max_message_bytes as u64 {
            return Err(BodyFailure::TooLarge);
        }
        let raw = fetch(locator).await?;
        let limits_copy = *limits;
        let id = message_id.to_string();
        let record = off_ui(move || {
            mailbox_mime::parse(&raw, &limits_copy).map(|parsed| parsed.into_record(&id))
        })
        .await?
        .map_err(mime)?;
        // A failed cache write costs a re-fetch next time; the body can still be shown now.
        let _ = store.save_body(OWNER, &record).await;
        Ok(record)
    }

    pub async fn load_body(message_id: &str) -> Result<BodyView, BodyFailure> {
        let store = open_store().await?;
        let limits = Limits::default();
        let max = limits.max_message_bytes as u64;
        load_with(&store, message_id, &limits, |locator| {
            fetch_raw(&store, locator, max)
        })
        .await
        .map(view)
    }

    /// Where saved attachments go (overridable with `MAILBOX_DOWNLOAD_DIR`).
    fn downloads_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("MAILBOX_DOWNLOAD_DIR") {
            return PathBuf::from(dir);
        }
        PathBuf::from(
            std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".into()),
        )
        .join("Downloads")
    }

    /// Mark the file as downloaded so macOS Gatekeeper checks it before it can run.
    #[cfg(target_os = "macos")]
    fn quarantine(path: &Path) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = std::process::Command::new("/usr/bin/xattr")
            .arg("-w")
            .arg("com.apple.quarantine")
            .arg(format!("0081;{stamp:08x};MailBox;"))
            .arg(path)
            .output();
    }

    #[cfg(not(target_os = "macos"))]
    fn quarantine(_path: &Path) {}

    /// Write `bytes` as a new file directly inside `dir`. The name is reduced to one safe path
    /// component again here, an existing file is never overwritten, the file is not executable
    /// and nothing opens or runs it. Returns the final file name.
    pub(super) fn write_new(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<String> {
        let name = mailbox_mime::safe_filename(name);
        std::fs::create_dir_all(dir)?;
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
            _ => (name.clone(), String::new()),
        };
        for n in 0..100 {
            let candidate = if n == 0 {
                name.clone()
            } else {
                format!("{stem} ({n}){ext}")
            };
            let path = dir.join(&candidate);
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(mut file) => {
                    file.write_all(bytes)?;
                    file.sync_all()?;
                    quarantine(&path);
                    return Ok(candidate);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
    }

    pub(super) async fn save_with<S, F, Fut>(
        store: &S,
        message_id: &str,
        part: u32,
        limits: &Limits,
        dir: &Path,
        fetch: F,
    ) -> Result<String, BodyFailure>
    where
        S: BodyStore,
        F: FnOnce(MessageLocator) -> Fut,
        Fut: Future<Output = Result<Vec<u8>, BodyFailure>>,
    {
        let locator = store
            .locate_message(OWNER, message_id)
            .await
            .map_err(storage)?;
        if locator.size_bytes > limits.max_message_bytes as u64 {
            return Err(BodyFailure::TooLarge);
        }
        let raw = fetch(locator).await?;
        let limits_copy = *limits;
        let (name, bytes) =
            off_ui(move || mailbox_mime::attachment_bytes(&raw, part, &limits_copy))
                .await?
                .map_err(mime)?;
        write_new(dir, &name, &bytes).map_err(|_| BodyFailure::Disk)
    }

    /// Attachments listed by the cached body, without any network. `None`: the body has not been
    /// downloaded on this device (forward cannot know the attachments yet).
    pub async fn cached_attachments(
        message_id: &str,
    ) -> Result<Option<Vec<mailbox_core::body::AttachmentMeta>>, BodyFailure> {
        let store = open_store().await?;
        Ok(store
            .load_body(OWNER, message_id)
            .await
            .map_err(storage)?
            .map(|b| b.attachments))
    }

    /// Fetch the message once more and return the bytes of the given attachment parts, for
    /// forwarding. Uses the live connection when the account's worker has one.
    pub async fn attachment_data(
        message_id: &str,
        parts: &[u32],
    ) -> Result<Vec<Vec<u8>>, BodyFailure> {
        let store = open_store().await?;
        let limits = Limits::default();
        let locator = store
            .locate_message(OWNER, message_id)
            .await
            .map_err(storage)?;
        let max = limits.max_message_bytes as u64;
        if locator.size_bytes > max {
            return Err(BodyFailure::TooLarge);
        }
        let raw = fetch_raw(&store, locator, max).await?;
        parts
            .iter()
            .map(|&part| {
                mailbox_mime::attachment_bytes(&raw, part, &limits)
                    .map(|(_, bytes)| bytes)
                    .map_err(mime)
            })
            .collect()
    }

    /// Fetch the message again and save one attachment into the Downloads folder.
    pub async fn save_attachment(message_id: &str, part: u32) -> Result<String, BodyFailure> {
        let store = open_store().await?;
        let limits = Limits::default();
        let max = limits.max_message_bytes as u64;
        save_with(
            &store,
            message_id,
            part,
            &limits,
            &downloads_dir(),
            |locator| fetch_raw(&store, locator, max),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_document_blocks_everything_but_inline_images_by_default() {
        let doc = frame_document("<p>hi</p>", false);
        assert!(doc.contains(
            "content=\"default-src 'none'; img-src data: cid:; style-src 'unsafe-inline'\""
        ));
        assert!(doc.contains("<body><p>hi</p></body>"));
        for forbidden in [
            "<script",
            "script-src",
            "https:",
            "http:",
            "<base href",
            "<link",
        ] {
            assert!(!doc.contains(forbidden), "{forbidden} in the blocked frame");
        }
        // The only `<base>` is MailBox's own: it sets the link target, never an address.
        assert_eq!(doc.matches("<base").count(), 1);
        assert!(doc.contains("<base target=\"_top\">"));
        let doc = frame_document("<p>hi</p>", true);
        assert!(doc.contains("img-src data: cid: https: http:;"));
        assert!(doc.contains("default-src 'none'") && !doc.contains("script-src"));
    }

    #[test]
    fn failures_have_actionable_copy_without_internals() {
        for e in [
            BodyFailure::TooLarge,
            BodyFailure::Unreachable,
            BodyFailure::Auth,
            BodyFailure::Security,
            BodyFailure::Gone,
            BodyFailure::Unsupported,
            BodyFailure::Disk,
            BodyFailure::Failed,
        ] {
            let m = e.message();
            assert!(!m.is_empty() && !m.contains('@') && !m.contains("imap") && !m.contains("993"));
            assert!(!e.key().is_empty());
        }
        assert_eq!(
            BodyState::from_result(Err(BodyFailure::TooLarge)),
            BodyState::TooLarge
        );
        assert_eq!(
            BodyState::from_result(Err(BodyFailure::Gone)).key(),
            "failed"
        );
        assert_eq!(
            BodyState::from_result(Ok(BodyView::default())).key(),
            "loaded"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    mod native {
        use super::super::native::{load_with, save_with, write_new};
        use super::super::*;
        use crate::adapters::account::OWNER;
        use mailbox_core::body::BodyStore;
        use mailbox_core::store::{AccountRecord, FolderRecord, MailStore, MessageRecord};
        use mailbox_mime::Limits;
        use mailbox_store::SqliteStore;
        use std::cell::Cell;

        const RAW: &[u8] = b"From: Sender <s@example.org>\r\nSubject: t\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\nContent-Type: text/html\r\n\r\n<p onclick=\"x()\">Hello</p><script>alert(1)</script><img src=\"https://track.example/p.gif\">\r\n--b\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"../../../evil.sh\"\r\n\r\necho pwned\r\n--b--\r\n";

        fn rt() -> tokio::runtime::Runtime {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
        }

        async fn seeded(size: u64) -> SqliteStore {
            let store = SqliteStore::in_memory().await.unwrap();
            store
                .upsert_account(&AccountRecord {
                    id: "acc-t".into(),
                    owner_id: OWNER.into(),
                    display_name: "example.com".into(),
                    email_address: "t@example.com".into(),
                    imap_host: "imap.example.com".into(),
                    imap_port: 993,
                    smtp_host: "smtp.example.com".into(),
                    smtp_port: 465,
                    auth_type: "password".into(),
                    credential_ref: "imap.acc-t".into(),
                    status: "active".into(),
                })
                .await
                .unwrap();
            store
                .upsert_folder(
                    OWNER,
                    &FolderRecord {
                        id: "acc-t/INBOX".into(),
                        account_id: "acc-t".into(),
                        remote_name: "INBOX".into(),
                        delimiter: None,
                        special_use: Some("inbox".into()),
                        uid_validity: Some(5),
                        uid_next: None,
                    },
                )
                .await
                .unwrap();
            store
                .upsert_messages(
                    OWNER,
                    &[MessageRecord {
                        id: "m1".into(),
                        account_id: "acc-t".into(),
                        folder_id: "acc-t/INBOX".into(),
                        uid_validity: 5,
                        imap_uid: 42,
                        message_id_header: None,
                        subject: "t".into(),
                        from_display: String::new(),
                        from_address: "s@example.org".into(),
                        preview: String::new(),
                        internal_date: 1,
                        size_bytes: size,
                        seen: false,
                        flagged: false,
                    }],
                )
                .await
                .unwrap();
            store
        }

        /// MB-304: MIME parsing and HTML sanitising run on the blocking pool, not on the task
        /// that drives the UI, and give exactly the same record as a direct call.
        #[test]
        fn parsing_runs_off_the_calling_thread_with_identical_output() {
            use super::super::native::off_ui;
            rt().block_on(async {
                let here = std::thread::current().id();
                let there = off_ui(|| std::thread::current().id()).await.unwrap();
                assert_ne!(here, there, "the work must not run on the UI thread");

                let limits = Limits::default();
                let direct = mailbox_mime::parse(RAW, &limits).unwrap().into_record("m1");
                let store = seeded(RAW.len() as u64).await;
                let loaded = load_with(&store, "m1", &limits, |_| async { Ok(RAW.to_vec()) })
                    .await
                    .unwrap();
                assert_eq!(loaded, direct);
            });
        }

        #[test]
        fn body_is_fetched_once_sanitized_and_then_served_from_cache() {
            rt().block_on(async {
                let store = seeded(RAW.len() as u64).await;
                let calls = Cell::new(0);
                let limits = Limits::default();
                let fetch = |locator: mailbox_core::body::MessageLocator| {
                    calls.set(calls.get() + 1);
                    assert_eq!(
                        (
                            locator.folder_remote_name.as_str(),
                            locator.uid_validity,
                            locator.imap_uid
                        ),
                        ("INBOX", 5, 42)
                    );
                    async { Ok(RAW.to_vec()) }
                };
                let first = load_with(&store, "m1", &limits, fetch).await.unwrap();
                let html = first.html.clone().unwrap();
                assert!(html.contains("Hello"));
                for bad in ["script", "onclick", "alert", "track.example"] {
                    assert!(!html.contains(bad), "{bad} reached the reader");
                }
                assert!(first.remote_blocked);
                assert!(
                    first
                        .html_remote
                        .as_deref()
                        .unwrap()
                        .contains("https://track.example/p.gif")
                );
                assert_eq!(first.attachments[0].filename, "evil.sh");
                // Cached copy is identical and needs no second fetch.
                let again = load_with(&store, "m1", &limits, |_| async {
                    panic!("cache hit must not touch the network")
                })
                .await
                .unwrap();
                assert_eq!(again, first);
                assert_eq!(calls.get(), 1);
                assert_eq!(store.load_body(OWNER, "m1").await.unwrap(), Some(first));
            });
        }

        #[test]
        fn failures_are_reported_and_never_cached() {
            rt().block_on(async {
                let limits = Limits::default();
                // Unknown message.
                let store = seeded(10).await;
                let r = load_with(&store, "nope", &limits, |_| async { Ok(Vec::new()) }).await;
                assert_eq!(r, Err(BodyFailure::Gone));
                // Envelope says it is too large: refused without fetching.
                let big = seeded(26 * 1024 * 1024).await;
                let r = load_with(&big, "m1", &limits, |_| async {
                    panic!("an oversized message must not be fetched")
                })
                .await;
                assert_eq!(r, Err(BodyFailure::TooLarge));
                // Network failure, then unparseable content: reported, nothing stored.
                let r = load_with(&store, "m1", &limits, |_| async {
                    Err(BodyFailure::Unreachable)
                })
                .await;
                assert_eq!(r, Err(BodyFailure::Unreachable));
                let r = load_with(&store, "m1", &limits, |_| async { Ok(Vec::new()) }).await;
                assert_eq!(r, Err(BodyFailure::Failed));
                // The server sent more than the envelope claimed.
                let small = Limits {
                    max_message_bytes: 64,
                    ..Limits::default()
                };
                let r = load_with(&store, "m1", &small, |_| async { Ok(RAW.to_vec()) }).await;
                assert_eq!(r, Err(BodyFailure::TooLarge));
                assert_eq!(store.load_body(OWNER, "m1").await.unwrap(), None);
            });
        }

        fn temp_dir(tag: &str) -> std::path::PathBuf {
            let dir =
                std::env::temp_dir().join(format!("mailbox-save-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            dir
        }

        #[test]
        fn attachment_is_saved_inside_the_target_folder_only() {
            rt().block_on(async {
                let store = seeded(RAW.len() as u64).await;
                let root = temp_dir("att");
                let dir = root.join("a/b/Downloads");
                let limits = Limits::default();
                let name = save_with(&store, "m1", 0, &limits, &dir, |_| async {
                    Ok(RAW.to_vec())
                })
                .await
                .unwrap();
                assert_eq!(name, "evil.sh", "the traversal part of the name is gone");
                assert_eq!(std::fs::read(dir.join("evil.sh")).unwrap(), b"echo pwned");
                // Nothing escaped to a parent directory.
                for parent in [root.join("a/b"), root.join("a"), root.clone()] {
                    assert!(!parent.join("evil.sh").exists());
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = std::fs::metadata(dir.join("evil.sh"))
                        .unwrap()
                        .permissions()
                        .mode();
                    assert_eq!(mode & 0o111, 0, "saved files are never executable");
                }
                // Saving again never overwrites.
                let second = save_with(&store, "m1", 0, &limits, &dir, |_| async {
                    Ok(RAW.to_vec())
                })
                .await
                .unwrap();
                assert_eq!(second, "evil (1).sh");
                // Unknown part, unknown message.
                let r = save_with(&store, "m1", 7, &limits, &dir, |_| async {
                    Ok(RAW.to_vec())
                })
                .await;
                assert_eq!(r, Err(BodyFailure::Gone));
                let r = save_with(&store, "x", 0, &limits, &dir, |_| async {
                    Ok(RAW.to_vec())
                })
                .await;
                assert_eq!(r, Err(BodyFailure::Gone));
                assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
                std::fs::remove_dir_all(&root).unwrap();
            });
        }

        #[test]
        fn hostile_names_cannot_leave_the_folder_even_if_passed_directly() {
            let root = temp_dir("names");
            let dir = root.join("inner");
            for name in [
                "../up.txt",
                "/abs.txt",
                "..\\win.txt",
                "..",
                "",
                ".hidden",
                "a/b/c.txt",
            ] {
                let written = write_new(&dir, name, b"x").unwrap();
                assert!(
                    !written.contains('/') && !written.contains('\\') && !written.starts_with('.')
                );
                assert!(dir.join(&written).is_file(), "{name:?} -> {written:?}");
            }
            assert_eq!(
                std::fs::read_dir(&root).unwrap().count(),
                1,
                "only `inner` exists"
            );
            std::fs::remove_dir_all(&root).unwrap();
        }
    }
}
