//! IMAP connection check (MB-111).
//!
//! Security properties, all enforced here and covered by tests:
//! - TLS is mandatory (implicit TLS). There is no plaintext mode and no switch to skip or relax
//!   certificate validation; an invalid certificate aborts before any credential is sent.
//! - Credentials are written to the socket only, as IMAP literals, never formatted into a string
//!   that could be logged. Errors carry no server text, user name or secret.
//! - Every step has a timeout and every server line a size limit.
pub mod append;
pub mod body;
pub mod borrow;
mod folders;
pub mod idle;
pub mod live;
pub mod mutate;
pub mod parse;

use mailbox_core::secrets::Secret;
use mailbox_core::{MailServerEndpoint, TlsMode};
use mailbox_net::{NetError, step};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

const MAX_LINE: usize = 8 * 1024;
const MAX_CAPABILITIES: usize = 64;

/// Why a connection check failed. Variants map 1:1 to the UI states in docs/03-ui/SCREENS.md S02.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectError {
    /// Host name or port is not acceptable (checked before any network access).
    InvalidEndpoint,
    /// Only implicit TLS is supported so far; STARTTLS is refused rather than downgraded.
    UnsupportedTlsMode,
    /// DNS failure, refused connection or unreachable network.
    Unreachable,
    Timeout,
    /// The server certificate is not valid for this host. Never bypassable.
    CertificateInvalid,
    /// TLS could not be established for another reason (protocol/cipher mismatch).
    TlsFailed,
    /// The peer is not an IMAP server or broke the protocol.
    NotImap,
    /// The server does not allow LOGIN (e.g. OAuth-only provider).
    LoginDisabled,
    /// Wrong user name, password or app password.
    AuthFailed,
}

/// What the server told us, without any account data.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServerInfo {
    pub capabilities: Vec<String>,
    /// Number of messages in INBOX, when a login check could read it (read-only EXAMINE).
    pub inbox_messages: Option<u32>,
}

impl ServerInfo {
    pub fn has(&self, capability: &str) -> bool {
        self.capabilities
            .iter()
            .any(|c| c.eq_ignore_ascii_case(capability))
    }
}

impl From<NetError> for ConnectError {
    fn from(e: NetError) -> Self {
        match e {
            NetError::InvalidEndpoint => ConnectError::InvalidEndpoint,
            NetError::Unreachable => ConnectError::Unreachable,
            NetError::Timeout => ConnectError::Timeout,
            NetError::CertificateInvalid => ConnectError::CertificateInvalid,
            NetError::TlsFailed => ConnectError::TlsFailed,
        }
    }
}

/// Open a verified TLS connection. Returns only after the certificate has been validated.
async fn connect_tls(endpoint: &MailServerEndpoint) -> Result<mailbox_net::Tls, ConnectError> {
    if endpoint.tls_mode != TlsMode::ImplicitTls {
        // Shape is still validated first so a bad host is reported as such.
        endpoint
            .validate_shape()
            .map_err(|_| ConnectError::InvalidEndpoint)?;
        return Err(ConnectError::UnsupportedTlsMode);
    }
    Ok(mailbox_net::connect_tls(endpoint).await?)
}

/// Most bytes one command may make the client read (lines and literals together). The per-line,
/// per-literal and per-response caps bound each item, not their product; this bounds the product
/// (AUDIT S-08). A requested message body is not counted here: it has its own caller-set limit.
const COMMAND_BUDGET: usize = 32 * 1024 * 1024;

struct Dialogue<S> {
    stream: BufReader<S>,
    tag: u32,
    /// Bytes the current command may still read; reset when the next command is tagged.
    budget: usize,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Dialogue<S> {
    fn new(stream: S) -> Self {
        Self {
            stream: BufReader::new(stream),
            tag: 0,
            budget: COMMAND_BUDGET,
        }
    }

    /// Take `bytes` from the current command's budget; `false` when the command has used it up.
    /// Called before a literal is allocated, so an over-budget literal is never read.
    fn charge(&mut self, bytes: usize) -> bool {
        match self.budget.checked_sub(bytes) {
            Some(left) => {
                self.budget = left;
                true
            }
            None => {
                self.budget = 0;
                false
            }
        }
    }

    async fn line(&mut self) -> Result<String, ConnectError> {
        let buf = step(async {
            let mut buf = Vec::new();
            let mut limited = (&mut self.stream).take(MAX_LINE as u64);
            let n = limited
                .read_until(b'\n', &mut buf)
                .await
                .map_err(|_| ConnectError::NotImap)?;
            if n == 0 || !buf.ends_with(b"\n") {
                return Err(ConnectError::NotImap);
            }
            Ok(buf)
        })
        .await?;
        if !self.charge(buf.len()) {
            return Err(ConnectError::NotImap);
        }
        Ok(String::from_utf8_lossy(&buf).trim_end().to_string())
    }

    async fn send(&mut self, bytes: &[u8]) -> Result<(), ConnectError> {
        step(async {
            let w = self.stream.get_mut();
            w.write_all(bytes)
                .await
                .map_err(|_| ConnectError::NotImap)?;
            w.flush().await.map_err(|_| ConnectError::NotImap)
        })
        .await
    }

    /// A new tag starts a new command, and with it a fresh byte budget.
    fn next_tag(&mut self) -> String {
        self.budget = COMMAND_BUDGET;
        self.tag += 1;
        format!("a{}", self.tag)
    }

    /// Read until the tagged completion of `tag`; returns (ok, untagged lines).
    async fn completion(&mut self, tag: &str) -> Result<(bool, Vec<String>), ConnectError> {
        let mut untagged = Vec::new();
        for _ in 0..256 {
            let line = self.line().await?;
            if let Some(rest) = line.strip_prefix(tag).and_then(|r| r.strip_prefix(' ')) {
                let status = rest.split(' ').next().unwrap_or("").to_ascii_uppercase();
                return match status.as_str() {
                    "OK" => Ok((true, untagged)),
                    "NO" | "BAD" => Ok((false, untagged)),
                    _ => Err(ConnectError::NotImap),
                };
            }
            if !line.starts_with('*') && !line.starts_with('+') {
                return Err(ConnectError::NotImap);
            }
            untagged.push(line);
        }
        Err(ConnectError::NotImap)
    }

    async fn greeting(&mut self) -> Result<(), ConnectError> {
        let line = self.line().await?;
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("* OK") || upper.starts_with("* PREAUTH") {
            Ok(())
        } else {
            Err(ConnectError::NotImap)
        }
    }

    async fn capabilities(&mut self) -> Result<ServerInfo, ConnectError> {
        let tag = self.next_tag();
        self.send(format!("{tag} CAPABILITY\r\n").as_bytes())
            .await?;
        let (ok, untagged) = self.completion(&tag).await?;
        if !ok {
            return Err(ConnectError::NotImap);
        }
        let capabilities: Vec<String> = untagged
            .iter()
            .filter_map(|l| l.strip_prefix("* CAPABILITY "))
            .flat_map(|l| l.split_ascii_whitespace())
            .filter(|c| c.len() <= 64 && c.chars().all(|ch| ch.is_ascii_graphic()))
            .take(MAX_CAPABILITIES)
            .map(str::to_string)
            .collect();
        if !capabilities
            .iter()
            .any(|c| c.eq_ignore_ascii_case("IMAP4rev1") || c.eq_ignore_ascii_case("IMAP4rev2"))
        {
            return Err(ConnectError::NotImap);
        }
        Ok(ServerInfo {
            capabilities,
            inbox_messages: None,
        })
    }

    /// Send one argument as a synchronizing literal, so any byte in a password is transmitted
    /// exactly and nothing has to be quoted or escaped.
    async fn literal(
        &mut self,
        prefix: &str,
        value: &[u8],
        suffix: &[u8],
    ) -> Result<(), ConnectError> {
        self.send(format!("{prefix}{{{}}}\r\n", value.len()).as_bytes())
            .await?;
        let line = self.line().await?;
        if !line.starts_with('+') {
            return Err(ConnectError::NotImap);
        }
        self.send(value).await?;
        self.send(suffix).await
    }

    async fn login(&mut self, username: &str, password: &Secret) -> Result<(), ConnectError> {
        let tag = self.next_tag();
        self.literal(&format!("{tag} LOGIN "), username.as_bytes(), b" ")
            .await
            .map_err(|e| {
                // A server that answers the first literal with a tagged NO/BAD is rejecting LOGIN.
                if e == ConnectError::NotImap {
                    ConnectError::AuthFailed
                } else {
                    e
                }
            })?;
        self.send(format!("{{{}}}\r\n", password.expose().len()).as_bytes())
            .await?;
        let line = self.line().await?;
        if !line.starts_with('+') {
            return Err(ConnectError::AuthFailed);
        }
        self.send(password.expose().as_bytes()).await?;
        self.send(b"\r\n").await?;
        let (ok, _) = self.completion(&tag).await?;
        if ok {
            Ok(())
        } else {
            Err(ConnectError::AuthFailed)
        }
    }

    /// Read-only look at INBOX: returns the message count. Opens nothing for writing and
    /// changes no flags (EXAMINE, not SELECT).
    async fn inbox_count(&mut self) -> Option<u32> {
        let tag = self.next_tag();
        self.send(format!("{tag} EXAMINE INBOX\r\n").as_bytes())
            .await
            .ok()?;
        let (ok, untagged) = self.completion(&tag).await.ok()?;
        if !ok {
            return None;
        }
        untagged.iter().find_map(|l| {
            let mut parts = l.split_ascii_whitespace();
            match (parts.next(), parts.next(), parts.next()) {
                (Some("*"), Some(n), Some(word)) if word.eq_ignore_ascii_case("EXISTS") => {
                    n.parse().ok()
                }
                _ => None,
            }
        })
    }

    async fn logout(&mut self) {
        let tag = self.next_tag();
        if self
            .send(format!("{tag} LOGOUT\r\n").as_bytes())
            .await
            .is_ok()
        {
            let _ = self.completion(&tag).await;
        }
    }
}

const MAX_LITERAL: usize = 64 * 1024;
const MAX_UNTAGGED: usize = 5_000;

impl<S: AsyncRead + AsyncWrite + Unpin> Dialogue<S> {
    /// One response line including any literals it carries.
    async fn logical(&mut self) -> Result<parse::Logical, ConnectError> {
        let mut out = parse::Logical::default();
        loop {
            let text = self.line().await?;
            let literal = parse::literal_len(&text);
            out.texts.push(text);
            let Some(n) = literal else { return Ok(out) };
            if n > MAX_LITERAL || out.literals.len() >= 8 || !self.charge(n) {
                return Err(ConnectError::NotImap);
            }
            let mut buf = vec![0u8; n];
            step(async {
                self.stream
                    .read_exact(&mut buf)
                    .await
                    .map(|_| ())
                    .map_err(|_| ConnectError::NotImap)
            })
            .await?;
            out.literals.push(buf);
        }
    }

    /// Run a command and collect its untagged responses (with literals).
    async fn run(&mut self, command: &str) -> Result<Vec<parse::Logical>, ConnectError> {
        let tag = self.next_tag();
        self.send(format!("{tag} {command}\r\n").as_bytes()).await?;
        let mut untagged = Vec::new();
        loop {
            let l = self.logical().await?;
            let first = l.texts.first().map(String::as_str).unwrap_or("");
            if let Some(rest) = first
                .strip_prefix(tag.as_str())
                .and_then(|r| r.strip_prefix(' '))
            {
                return if rest.to_ascii_uppercase().starts_with("OK") {
                    Ok(untagged)
                } else {
                    Err(ConnectError::NotImap)
                };
            }
            if !first.starts_with('*') || untagged.len() >= MAX_UNTAGGED {
                return Err(ConnectError::NotImap);
            }
            untagged.push(l);
        }
    }
}

/// Mailbox names come from the server's own LIST reply (ASCII, modified UTF-7).
fn quote_mailbox(name: &str) -> Result<String, ConnectError> {
    if name.is_empty() || name.len() > 1024 || !name.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        return Err(ConnectError::NotImap);
    }
    Ok(format!(
        "\"{}\"",
        name.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

/// Envelope items. `IN-REPLY-TO REFERENCES TO CC` were added for threading and search (MB-201);
/// servers return only the fields a message has, so older parsers see the same shape.
const FETCH_ITEMS: &str = "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[HEADER.FIELDS (FROM SUBJECT DATE MESSAGE-ID IN-REPLY-TO REFERENCES TO CC)])";

/// An authenticated, read-only IMAP session. It only ever issues LIST, EXAMINE and FETCH with
/// `BODY.PEEK`, so it cannot change flags, move or delete anything on the server.
pub struct Session<S> {
    d: Dialogue<S>,
    pub info: ServerInfo,
}

impl Session<mailbox_net::Tls> {
    /// Connect over verified TLS and log in.
    pub async fn connect(
        endpoint: &MailServerEndpoint,
        username: &str,
        password: &Secret,
    ) -> Result<Self, ConnectError> {
        Self::login_stream(connect_tls(endpoint).await?, username, password).await
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> Session<S> {
    /// Greeting, CAPABILITY and LOGIN over an already-secured stream.
    pub async fn login_stream(
        stream: S,
        username: &str,
        password: &Secret,
    ) -> Result<Self, ConnectError> {
        validate_credentials(username, password)?;
        let mut d = Dialogue::new(stream);
        d.greeting().await?;
        let info = d.capabilities().await?;
        if info.has("LOGINDISABLED") {
            d.logout().await;
            return Err(ConnectError::LoginDisabled);
        }
        if let Err(e) = d.login(username, password).await {
            d.logout().await;
            return Err(e);
        }
        Ok(Self { d, info })
    }

    /// All folders the account can see.
    pub async fn list_folders(&mut self) -> Result<Vec<parse::ListEntry>, ConnectError> {
        let lines = self.d.run("LIST \"\" \"*\"").await?;
        Ok(lines.iter().filter_map(parse::parse_list).collect())
    }

    /// Open a folder read-only and report its UIDVALIDITY, UIDNEXT and message count.
    pub async fn examine(&mut self, folder: &str) -> Result<parse::MailboxStatus, ConnectError> {
        let lines = self
            .d
            .run(&format!("EXAMINE {}", quote_mailbox(folder)?))
            .await?;
        let texts: Vec<String> = lines.iter().map(parse::Logical::joined).collect();
        Ok(parse::parse_examine(&texts))
    }

    /// Envelopes of the newest `count` messages of the examined folder (`exists` from EXAMINE).
    pub async fn fetch_latest(
        &mut self,
        exists: u32,
        count: u32,
    ) -> Result<Vec<parse::Envelope>, ConnectError> {
        if exists == 0 || count == 0 {
            return Ok(Vec::new());
        }
        let first = exists.saturating_sub(count - 1).max(1);
        let lines = self
            .d
            .run(&format!("FETCH {first}:{exists} {FETCH_ITEMS}"))
            .await?;
        Ok(lines.iter().filter_map(parse::parse_fetch).collect())
    }

    /// Envelopes of messages with UID >= `from_uid` in the examined folder.
    pub async fn fetch_from_uid(
        &mut self,
        from_uid: u32,
    ) -> Result<Vec<parse::Envelope>, ConnectError> {
        let lines = self
            .d
            .run(&format!("UID FETCH {from_uid}:* {FETCH_ITEMS}"))
            .await?;
        // `n:*` always returns the last message, even when its UID is below n.
        Ok(lines
            .iter()
            .filter_map(parse::parse_fetch)
            .filter(|e| e.uid >= from_uid)
            .collect())
    }

    /// Envelopes with `lo <= UID <= hi` in the examined folder (MB-200 "load older").
    pub async fn fetch_uid_range(
        &mut self,
        lo: u32,
        hi: u32,
    ) -> Result<Vec<parse::Envelope>, ConnectError> {
        if lo == 0 || hi < lo {
            return Ok(Vec::new());
        }
        let lines = self
            .d
            .run(&format!("UID FETCH {lo}:{hi} {FETCH_ITEMS}"))
            .await?;
        Ok(lines
            .iter()
            .filter_map(parse::parse_fetch)
            .filter(|e| e.uid >= lo && e.uid <= hi)
            .collect())
    }

    /// Envelopes of exactly these UIDs (at most 200 per call) in the examined folder.
    pub async fn fetch_uid_set(
        &mut self,
        uids: &[u32],
    ) -> Result<Vec<parse::Envelope>, ConnectError> {
        let uids: Vec<u32> = uids.iter().copied().filter(|u| *u > 0).take(200).collect();
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let set = uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let lines = self
            .d
            .run(&format!("UID FETCH {set} {FETCH_ITEMS}"))
            .await?;
        Ok(lines
            .iter()
            .filter_map(parse::parse_fetch)
            .filter(|e| uids.contains(&e.uid))
            .collect())
    }

    pub async fn logout(mut self) {
        self.d.logout().await;
    }
}

/// Errors as the core sees them. Authentication problems ask for re-auth; network problems are
/// transient; everything else is a protocol problem. No server text is carried over.
impl From<ConnectError> for mailbox_core::MailboxError {
    fn from(e: ConnectError) -> Self {
        use mailbox_core::MailboxError as M;
        match e {
            ConnectError::AuthFailed | ConnectError::LoginDisabled => M::ReauthenticationRequired,
            ConnectError::Unreachable | ConnectError::Timeout => M::TransientConnectionFailure,
            ConnectError::CertificateInvalid => M::Storage("certificate_invalid"),
            ConnectError::TlsFailed => M::Storage("tls_failed"),
            ConnectError::InvalidEndpoint | ConnectError::UnsupportedTlsMode => {
                M::Storage("invalid_endpoint")
            }
            ConnectError::NotImap => M::InconsistentData("imap protocol"),
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> mailbox_core::sync::RemoteMailbox for Session<S> {
    async fn list_folders(
        &mut self,
    ) -> Result<Vec<mailbox_core::sync::RemoteFolder>, mailbox_core::MailboxError> {
        Ok(Session::list_folders(self)
            .await?
            .into_iter()
            .map(|f| mailbox_core::sync::RemoteFolder {
                selectable: f.selectable(),
                special_use: f.special_use().map(str::to_string),
                name: f.name,
                delimiter: f.delimiter,
            })
            .collect())
    }

    async fn examine(
        &mut self,
        folder: &str,
    ) -> Result<mailbox_core::sync::RemoteStatus, mailbox_core::MailboxError> {
        let s = Session::examine(self, folder).await?;
        Ok(mailbox_core::sync::RemoteStatus {
            exists: s.exists,
            uid_validity: s.uid_validity,
            uid_next: s.uid_next,
        })
    }

    async fn fetch_latest(
        &mut self,
        exists: u32,
        count: u32,
    ) -> Result<Vec<mailbox_core::sync::RemoteEnvelope>, mailbox_core::MailboxError> {
        Ok(Session::fetch_latest(self, exists, count)
            .await?
            .into_iter()
            .map(to_remote)
            .collect())
    }

    async fn fetch_from_uid(
        &mut self,
        from_uid: u32,
    ) -> Result<Vec<mailbox_core::sync::RemoteEnvelope>, mailbox_core::MailboxError> {
        Ok(Session::fetch_from_uid(self, from_uid)
            .await?
            .into_iter()
            .map(to_remote)
            .collect())
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> mailbox_core::history::RemoteHistory for Session<S> {
    async fn fetch_uid_range(
        &mut self,
        lo: u32,
        hi: u32,
    ) -> Result<Vec<mailbox_core::sync::RemoteEnvelope>, mailbox_core::MailboxError> {
        Ok(Session::fetch_uid_range(self, lo, hi)
            .await?
            .into_iter()
            .map(to_remote)
            .collect())
    }

    async fn fetch_uid_set(
        &mut self,
        uids: &[u32],
    ) -> Result<Vec<mailbox_core::sync::RemoteEnvelope>, mailbox_core::MailboxError> {
        Ok(Session::fetch_uid_set(self, uids)
            .await?
            .into_iter()
            .map(to_remote)
            .collect())
    }
}

fn to_remote(e: parse::Envelope) -> mailbox_core::sync::RemoteEnvelope {
    use mailbox_core::threading::{EnvelopeHeaders, parse_msg_ids};
    mailbox_core::sync::RemoteEnvelope {
        headers: EnvelopeHeaders {
            in_reply_to: e
                .in_reply_to
                .as_deref()
                .and_then(|v| parse_msg_ids(v).into_iter().next()),
            references: e
                .references
                .as_deref()
                .map(parse_msg_ids)
                .unwrap_or_default(),
            to: e.to,
            cc: e.cc,
        },
        uid: e.uid,
        seen: e.seen,
        flagged: e.flagged,
        internal_date: e.internal_date,
        size_bytes: e.size_bytes,
        subject: e.subject,
        from_display: e.from_display,
        from_address: e.from_address,
        message_id: e.message_id,
    }
}

fn validate_credentials(username: &str, password: &Secret) -> Result<(), ConnectError> {
    let bad = |s: &str| s.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0);
    if username.is_empty()
        || username.len() > 320
        || bad(username)
        || password.expose().is_empty()
        || bad(password.expose())
    {
        return Err(ConnectError::AuthFailed);
    }
    Ok(())
}

/// Greeting + CAPABILITY over an already-secured stream. Sends no credentials.
pub async fn probe_stream<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
) -> Result<ServerInfo, ConnectError> {
    let mut d = Dialogue::new(stream);
    d.greeting().await?;
    let info = d.capabilities().await?;
    d.logout().await;
    Ok(info)
}

/// Greeting, CAPABILITY, LOGIN, LOGOUT over an already-secured stream.
pub async fn check_login_stream<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    username: &str,
    password: &Secret,
) -> Result<ServerInfo, ConnectError> {
    if username.is_empty()
        || username.len() > 320
        || username.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0)
    {
        return Err(ConnectError::AuthFailed);
    }
    if password.expose().is_empty()
        || password
            .expose()
            .bytes()
            .any(|b| b == b'\r' || b == b'\n' || b == 0)
    {
        return Err(ConnectError::AuthFailed);
    }
    let mut d = Dialogue::new(stream);
    d.greeting().await?;
    let info = d.capabilities().await?;
    if info.has("LOGINDISABLED") {
        d.logout().await;
        return Err(ConnectError::LoginDisabled);
    }
    let result = d.login(username, password).await;
    let mut info = info;
    if result.is_ok() {
        info.inbox_messages = d.inbox_count().await;
    }
    d.logout().await;
    result.map(|()| info)
}

/// Connect with verified TLS and read the server capabilities. No credentials involved.
pub async fn probe(endpoint: &MailServerEndpoint) -> Result<ServerInfo, ConnectError> {
    probe_stream(connect_tls(endpoint).await?).await
}

/// Connect with verified TLS and check that the credentials are accepted.
pub async fn check_login(
    endpoint: &MailServerEndpoint,
    username: &str,
    password: &Secret,
) -> Result<ServerInfo, ConnectError> {
    check_login_stream(connect_tls(endpoint).await?, username, password).await
}

/// [`check_login`] over addresses that were already resolved and checked by the caller (SSRF
/// guard). The host name is not resolved again; the certificate is validated for that name.
pub async fn check_login_pinned(
    endpoint: &MailServerEndpoint,
    addrs: &[std::net::SocketAddr],
    username: &str,
    password: &Secret,
) -> Result<ServerInfo, ConnectError> {
    if endpoint.tls_mode != TlsMode::ImplicitTls {
        endpoint
            .validate_shape()
            .map_err(|_| ConnectError::InvalidEndpoint)?;
        return Err(ConnectError::UnsupportedTlsMode);
    }
    let tls = mailbox_net::connect_tls_pinned(endpoint, addrs).await?;
    check_login_stream(tls, username, password).await
}
