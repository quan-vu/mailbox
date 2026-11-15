//! Read-write IMAP paths (MB-132 flags, MB-144 move / archive / trash / permanent delete).
//!
//! [`crate::Session`] stays read-only by construction; everything that can change a mailbox goes
//! through [`MutableSession`], which a caller has to ask for explicitly.
//!
//! Rules enforced here and covered by `tests/mutate.rs`:
//! - messages are addressed by UID only, in a folder opened with SELECT in this session; the
//!   caller compares the UIDVALIDITY returned by `select` with the one its UID belongs to;
//! - a plain `EXPUNGE` is never sent: it would remove every `\Deleted` message of the folder,
//!   including ones flagged by other clients. Without UIDPLUS the message is left flagged and the
//!   caller is told;
//! - a NO or BAD answer is a refusal, a closed or silent connection is "outcome unknown"; the two
//!   are never mixed up, because the caller rolls back on the first and retries on the second;
//! - folder names are quoted and checked before use; server text never leaves this module.
use crate::{ConnectError, Dialogue, Session, parse, quote_mailbox};
use mailbox_core::actions::{
    Expunged, FlagSource, MessageFlag, Moved, MutableMailbox, RemoteFailure, RemoteFlags, Selected,
};
use mailbox_core::secrets::Secret;
use mailbox_core::{MailServerEndpoint, MailboxError};
use mailbox_net::{NetError, step};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAX_LINE: usize = 8 * 1024;
const MAX_LITERAL: usize = 64 * 1024;
const MAX_UNTAGGED: usize = 5_000;
/// A flag scan answers with one line per cached message.
const MAX_FLAG_LINES: usize = 100_000;

/// Local wrapper so a timeout from the transport becomes "connection lost".
struct Fail(RemoteFailure);

impl From<NetError> for Fail {
    fn from(_: NetError) -> Self {
        Fail(RemoteFailure::ConnectionLost)
    }
}

struct Reply {
    /// Text after `OK` on the tagged line (response code and human text).
    tagged: String,
    untagged: Vec<String>,
}

async fn read_line<S: AsyncRead + AsyncWrite + Unpin>(
    d: &mut Dialogue<S>,
) -> Result<String, RemoteFailure> {
    let buf = step(async {
        let mut buf = Vec::new();
        let mut limited = (&mut d.stream).take(MAX_LINE as u64);
        let n = limited
            .read_until(b'\n', &mut buf)
            .await
            .map_err(|_| Fail(RemoteFailure::ConnectionLost))?;
        if n == 0 {
            return Err(Fail(RemoteFailure::ConnectionLost));
        }
        if !buf.ends_with(b"\n") {
            // Either an over-long line or the stream ended mid-line.
            return Err(Fail(if n >= MAX_LINE {
                RemoteFailure::Protocol
            } else {
                RemoteFailure::ConnectionLost
            }));
        }
        Ok(buf)
    })
    .await
    .map_err(|Fail(e)| e)?;
    // Same per-command byte budget as the read-only dialogue (AUDIT S-08).
    if !d.charge(buf.len()) {
        return Err(RemoteFailure::Protocol);
    }
    Ok(String::from_utf8_lossy(&buf).trim_end().to_string())
}

/// One response line; literals are consumed and dropped (nothing here needs their content).
async fn read_logical<S: AsyncRead + AsyncWrite + Unpin>(
    d: &mut Dialogue<S>,
) -> Result<String, RemoteFailure> {
    let mut out = String::new();
    for _ in 0..8 {
        let line = read_line(d).await?;
        let literal = parse::literal_len(&line);
        out.push_str(&line);
        let Some(n) = literal else { return Ok(out) };
        if n > MAX_LITERAL || !d.charge(n) {
            return Err(RemoteFailure::Protocol);
        }
        let mut buf = vec![0u8; n];
        step(async {
            d.stream
                .read_exact(&mut buf)
                .await
                .map(|_| ())
                .map_err(|_| Fail(RemoteFailure::ConnectionLost))
        })
        .await
        .map_err(|Fail(e)| e)?;
        out.push(' ');
    }
    Err(RemoteFailure::Protocol)
}

async fn exec<S: AsyncRead + AsyncWrite + Unpin>(
    d: &mut Dialogue<S>,
    command: &str,
    max_untagged: usize,
) -> Result<Reply, RemoteFailure> {
    let tag = d.next_tag();
    let bytes = format!("{tag} {command}\r\n");
    step(async {
        let w = d.stream.get_mut();
        w.write_all(bytes.as_bytes())
            .await
            .map_err(|_| Fail(RemoteFailure::ConnectionLost))?;
        w.flush()
            .await
            .map_err(|_| Fail(RemoteFailure::ConnectionLost))
    })
    .await
    .map_err(|Fail(e)| e)?;
    let mut untagged = Vec::new();
    loop {
        let text = read_logical(d).await?;
        if let Some(rest) = text
            .strip_prefix(tag.as_str())
            .and_then(|r| r.strip_prefix(' '))
        {
            let (status, tail) = rest.split_once(' ').unwrap_or((rest, ""));
            return match status.to_ascii_uppercase().as_str() {
                "OK" => Ok(Reply {
                    tagged: tail.to_string(),
                    untagged,
                }),
                "NO" | "BAD" => Err(RemoteFailure::Refused),
                _ => Err(RemoteFailure::Protocol),
            };
        }
        if !text.starts_with('*') || untagged.len() >= max_untagged {
            return Err(RemoteFailure::Protocol);
        }
        untagged.push(text);
    }
}

/// `[COPYUID <uidvalidity> <source-uid> <dest-uid>]` for a single message (RFC 4315).
fn copyuid(text: &str) -> Option<(u64, u32)> {
    let at = text.to_ascii_uppercase().find("[COPYUID ")?;
    let inner = &text[at + 9..];
    let inner = &inner[..inner.find(']')?];
    let mut parts = inner.split_ascii_whitespace();
    let uid_validity = parts.next()?.parse().ok()?;
    let _source = parts.next()?;
    let uid = parts.next()?.parse().ok()?;
    Some((uid_validity, uid))
}

fn copyuid_of(reply: &Reply) -> Option<(u64, u32)> {
    copyuid(&reply.tagged).or_else(|| reply.untagged.iter().find_map(|l| copyuid(l)))
}

/// `(uid, flags)` of one `* n FETCH (...)` line. `Ok(None)` for other untagged lines; an error
/// for a FETCH that cannot be attributed to a UID.
fn fetch_flags_line(text: &str) -> Result<Option<RemoteFlags>, RemoteFailure> {
    let mut words = text.split_ascii_whitespace();
    if !(words.next() == Some("*")
        && words.next().is_some_and(|n| n.parse::<u32>().is_ok())
        && words
            .next()
            .is_some_and(|w| w.eq_ignore_ascii_case("FETCH")))
    {
        return Ok(None);
    }
    let upper = text.to_ascii_uppercase();
    let (flags, rest) = match upper.find("FLAGS (") {
        Some(start) => {
            let end = text[start..]
                .find(')')
                .map(|e| start + e)
                .ok_or(RemoteFailure::Protocol)?;
            (
                &text[start + 7..end],
                format!("{} {}", &text[..start], &text[end + 1..]),
            )
        }
        None => return Err(RemoteFailure::Protocol),
    };
    // Look for the UID outside the flag list: a keyword flag may be spelled like anything.
    let cleaned = rest.replace(['(', ')'], " ");
    let mut tokens = cleaned.split_ascii_whitespace();
    let uid = loop {
        match tokens.next() {
            Some(t) if t.eq_ignore_ascii_case("UID") => {
                break tokens.next().and_then(|v| v.parse::<u32>().ok());
            }
            Some(_) => {}
            None => break None,
        }
    }
    .ok_or(RemoteFailure::Protocol)?;
    let has = |flag: &str| {
        flags
            .split_ascii_whitespace()
            .any(|f| f.eq_ignore_ascii_case(flag))
    };
    Ok(Some(RemoteFlags {
        uid,
        seen: has("\\Seen"),
        flagged: has("\\Flagged"),
    }))
}

impl<S: AsyncRead + AsyncWrite + Unpin> Session<S> {
    /// `\Seen` / `\Flagged` of the messages with `first <= UID <= last` in the open folder.
    /// Read-only: no body section is fetched, so nothing gets marked as read.
    pub async fn fetch_flags(
        &mut self,
        first: u32,
        last: u32,
    ) -> Result<Vec<RemoteFlags>, RemoteFailure> {
        if first == 0 || first > last {
            return Ok(Vec::new());
        }
        let reply = exec(
            &mut self.d,
            &format!("UID FETCH {first}:{last} (UID FLAGS)"),
            MAX_FLAG_LINES,
        )
        .await?;
        let mut out = Vec::new();
        for line in &reply.untagged {
            if let Some(flags) = fetch_flags_line(line)?
                && flags.uid >= first
                && flags.uid <= last
            {
                out.push(flags);
            }
        }
        Ok(out)
    }
}

fn flag_name(flag: MessageFlag) -> &'static str {
    match flag {
        MessageFlag::Seen => "\\Seen",
        MessageFlag::Flagged => "\\Flagged",
    }
}

/// An authenticated session that may change the mailbox.
pub struct MutableSession<S> {
    session: Session<S>,
    can_move: bool,
    uidplus: bool,
    selected: bool,
}

impl MutableSession<mailbox_net::Tls> {
    /// Connect over verified TLS, log in and read the post-login capabilities.
    pub async fn connect(
        endpoint: &MailServerEndpoint,
        username: &str,
        password: &Secret,
    ) -> Result<Self, ConnectError> {
        Self::upgrade(Session::connect(endpoint, username, password).await?).await
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> MutableSession<S> {
    /// Turn a logged-in session into a read-write one. Capabilities are asked for again because
    /// servers commonly announce MOVE and UIDPLUS only after authentication.
    pub async fn upgrade(mut session: Session<S>) -> Result<Self, ConnectError> {
        let info = session.d.capabilities().await?;
        let (can_move, uidplus) = (info.has("MOVE"), info.has("UIDPLUS"));
        session.info = info;
        Ok(Self {
            session,
            can_move,
            uidplus,
            selected: false,
        })
    }

    pub fn can_move(&self) -> bool {
        self.can_move
    }

    pub fn has_uidplus(&self) -> bool {
        self.uidplus
    }

    /// The read-only API on the same connection, without giving up write access (MB-140: the
    /// live worker and borrowers sync and fetch through it). Whatever folder it opens, the next
    /// write must `select` again first.
    pub fn reader_mut(&mut self) -> &mut Session<S> {
        self.selected = false;
        &mut self.session
    }

    /// Server capabilities as seen after login.
    pub fn reader(&self) -> &Session<S> {
        &self.session
    }

    /// Back to the read-only API (the folder opened last stays open until the next EXAMINE).
    pub fn into_reader(self) -> Session<S> {
        self.session
    }

    fn require_selected(&self, uid: u32) -> Result<(), RemoteFailure> {
        if self.selected && uid != 0 {
            Ok(())
        } else {
            Err(RemoteFailure::Protocol)
        }
    }

    async fn run(&mut self, command: &str) -> Result<Reply, RemoteFailure> {
        exec(&mut self.session.d, command, MAX_UNTAGGED).await
    }

    /// Open a folder for writing. Compare the returned UIDVALIDITY before using any cached UID.
    pub async fn select(&mut self, folder: &str) -> Result<Selected, RemoteFailure> {
        let name = quote_mailbox(folder).map_err(|_| RemoteFailure::Protocol)?;
        self.selected = false;
        let reply = self.run(&format!("SELECT {name}")).await?;
        self.selected = true;
        Ok(Selected {
            uid_validity: parse::parse_examine(&reply.untagged).uid_validity,
            read_only: reply.tagged.to_ascii_uppercase().contains("[READ-ONLY]"),
        })
    }

    /// Whether a message with this UID is in the selected folder.
    pub async fn uid_exists(&mut self, uid: u32) -> Result<bool, RemoteFailure> {
        self.require_selected(uid)?;
        let reply = self.run(&format!("UID FETCH {uid} (UID)")).await?;
        let wanted = uid.to_string();
        Ok(reply.untagged.iter().any(|l| {
            let cleaned = l.replace(['(', ')'], " ");
            let tokens: Vec<&str> = cleaned.split_ascii_whitespace().collect();
            tokens.iter().any(|t| t.eq_ignore_ascii_case("FETCH"))
                && tokens
                    .windows(2)
                    .any(|w| w[0].eq_ignore_ascii_case("UID") && w[1] == wanted)
        }))
    }

    /// `UID STORE +FLAGS` / `-FLAGS`. Other flags of the message are left alone.
    pub async fn store_flags(
        &mut self,
        uid: u32,
        add: bool,
        flags: &[MessageFlag],
    ) -> Result<(), RemoteFailure> {
        self.require_selected(uid)?;
        if flags.is_empty() {
            return Err(RemoteFailure::Protocol);
        }
        let list: Vec<&str> = flags.iter().map(|f| flag_name(*f)).collect();
        let sign = if add { '+' } else { '-' };
        self.run(&format!("UID STORE {uid} {sign}FLAGS ({})", list.join(" ")))
            .await
            .map(|_| ())
    }

    async fn mark_deleted(&mut self, uid: u32) -> Result<(), RemoteFailure> {
        self.run(&format!("UID STORE {uid} +FLAGS.SILENT (\\Deleted)"))
            .await
            .map(|_| ())
    }

    /// Move one message to `dest`. Uses `UID MOVE` when the server has it, otherwise copy, flag
    /// the original `\Deleted` and expunge exactly that UID (UIDPLUS). Without UIDPLUS the
    /// original stays flagged and `source_left_deleted` says so.
    pub async fn move_to(&mut self, uid: u32, dest: &str) -> Result<Moved, RemoteFailure> {
        self.require_selected(uid)?;
        let dest = quote_mailbox(dest).map_err(|_| RemoteFailure::Protocol)?;
        if self.can_move {
            let reply = self.run(&format!("UID MOVE {uid} {dest}")).await?;
            return Ok(Moved {
                new_uid: copyuid_of(&reply),
                source_left_deleted: false,
            });
        }
        let reply = self.run(&format!("UID COPY {uid} {dest}")).await?;
        let new_uid = copyuid_of(&reply);
        self.mark_deleted(uid).await?;
        let expunged = self.uidplus && self.run(&format!("UID EXPUNGE {uid}")).await.is_ok();
        Ok(Moved {
            new_uid,
            source_left_deleted: !expunged,
        })
    }

    /// Permanently delete one message of the selected folder.
    pub async fn delete_forever(&mut self, uid: u32) -> Result<Expunged, RemoteFailure> {
        self.require_selected(uid)?;
        self.mark_deleted(uid).await?;
        if !self.uidplus {
            return Ok(Expunged::LeftDeleted);
        }
        self.run(&format!("UID EXPUNGE {uid}")).await?;
        Ok(Expunged::Removed)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> MutableMailbox for MutableSession<S> {
    async fn select(&mut self, folder: &str) -> Result<Selected, RemoteFailure> {
        MutableSession::select(self, folder).await
    }
    async fn uid_exists(&mut self, uid: u32) -> Result<bool, RemoteFailure> {
        MutableSession::uid_exists(self, uid).await
    }
    async fn store_flag(
        &mut self,
        uid: u32,
        flag: MessageFlag,
        on: bool,
    ) -> Result<(), RemoteFailure> {
        self.store_flags(uid, on, &[flag]).await
    }
    async fn move_message(&mut self, uid: u32, dest_folder: &str) -> Result<Moved, RemoteFailure> {
        self.move_to(uid, dest_folder).await
    }
    async fn expunge_message(&mut self, uid: u32) -> Result<Expunged, RemoteFailure> {
        self.delete_forever(uid).await
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> FlagSource for Session<S> {
    async fn fetch_flags(
        &mut self,
        first_uid: u32,
        last_uid: u32,
    ) -> Result<Vec<RemoteFlags>, MailboxError> {
        Session::fetch_flags(self, first_uid, last_uid)
            .await
            .map_err(|e| match e {
                RemoteFailure::ConnectionLost => MailboxError::TransientConnectionFailure,
                RemoteFailure::Refused | RemoteFailure::Protocol => {
                    MailboxError::InconsistentData("imap protocol")
                }
            })
    }
}
