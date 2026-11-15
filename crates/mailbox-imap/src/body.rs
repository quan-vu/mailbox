//! Fetching one whole message (MB-130).
//!
//! Read-only: `UID FETCH <uid> (RFC822.SIZE)` followed by `UID FETCH <uid> (BODY.PEEK[])`.
//! `BODY.PEEK` never sets `\Seen`. The server is not trusted about sizes: the announced message
//! size is checked first so an oversized body is never requested, and the literal length is
//! checked again before a single byte of it is read or allocated.
use crate::{ConnectError, Dialogue, Session, parse};
use mailbox_net::step;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};

/// Bodies are read in pieces so each piece gets its own timeout: a large message on a slow
/// line is fine, a stalled connection is not.
const CHUNK: usize = 64 * 1024;
/// Largest literal accepted in a response that is not the requested body.
const MAX_OTHER_LITERAL: usize = 64 * 1024;
const MAX_RESPONSES: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyError {
    /// The message (or the literal the server announced) exceeds the caller's limit.
    TooLarge {
        size: u64,
    },
    /// No such UID in the examined folder (expunged, or the folder changed).
    NotFound,
    Connect(ConnectError),
}

impl From<ConnectError> for BodyError {
    fn from(e: ConnectError) -> Self {
        BodyError::Connect(e)
    }
}

/// The number following the atom `key` in a FETCH response (`UID 7`, `RFC822.SIZE 2048`).
fn number_after(text: &str, key: &str) -> Option<u64> {
    let mut tokens = text
        .split(|c: char| c.is_ascii_whitespace() || c == '(' || c == ')')
        .filter(|t| !t.is_empty());
    while let Some(token) = tokens.next() {
        if token.eq_ignore_ascii_case(key) {
            return tokens.next()?.parse().ok();
        }
    }
    None
}

fn is_fetch(line: &str) -> bool {
    let mut parts = line.split_ascii_whitespace();
    parts.next() == Some("*")
        && parts.next().is_some_and(|n| n.parse::<u32>().is_ok())
        && parts
            .next()
            .is_some_and(|w| w.eq_ignore_ascii_case("FETCH"))
}

impl<S: AsyncRead + AsyncWrite + Unpin> Dialogue<S> {
    async fn read_literal(&mut self, len: usize) -> Result<Vec<u8>, ConnectError> {
        let mut buf = Vec::new();
        while buf.len() < len {
            let start = buf.len();
            buf.resize(start + CHUNK.min(len - start), 0);
            step(async {
                self.stream
                    .read_exact(&mut buf[start..])
                    .await
                    .map(|_| ())
                    .map_err(|_| ConnectError::NotImap)
            })
            .await?;
        }
        Ok(buf)
    }

    async fn body_literal(&mut self, uid: u32, max_bytes: u64) -> Result<Vec<u8>, BodyError> {
        let tag = self.next_tag();
        self.send(format!("{tag} UID FETCH {uid} (BODY.PEEK[])\r\n").as_bytes())
            .await?;
        let mut body = None;
        for _ in 0..MAX_RESPONSES {
            let line = self.line().await?;
            if let Some(rest) = line
                .strip_prefix(tag.as_str())
                .and_then(|r| r.strip_prefix(' '))
            {
                return if rest.to_ascii_uppercase().starts_with("OK") {
                    body.ok_or(BodyError::NotFound)
                } else {
                    Err(ConnectError::NotImap.into())
                };
            }
            if !line.starts_with('*') {
                return Err(ConnectError::NotImap.into());
            }
            // Untagged data without a literal (EXISTS, flag updates, `BODY[] NIL`) is not a body.
            let Some(len) = parse::literal_len(&line) else {
                continue;
            };
            let wanted = is_fetch(&line) && line.to_ascii_uppercase().contains("BODY[] {");
            if wanted && len as u64 > max_bytes {
                // Nothing of the literal is read; the caller must drop this session.
                return Err(BodyError::TooLarge { size: len as u64 });
            }
            // The requested body is bounded by `max_bytes`; everything else counts against the
            // command's byte budget.
            if !wanted && (len > MAX_OTHER_LITERAL || !self.charge(len)) {
                return Err(ConnectError::NotImap.into());
            }
            let bytes = self.read_literal(len).await?;
            // The rest of this response: usually `)`, sometimes more items or literals.
            let mut tail = String::new();
            for _ in 0..8 {
                let more = self.line().await?;
                let next = parse::literal_len(&more);
                tail.push(' ');
                tail.push_str(&more);
                match next {
                    Some(n) if n <= MAX_OTHER_LITERAL && self.charge(n) => {
                        drop(self.read_literal(n).await?)
                    }
                    Some(_) => return Err(ConnectError::NotImap.into()),
                    None => break,
                }
            }
            // Only an answer that names the requested UID counts.
            let named = number_after(&line, "UID").or_else(|| number_after(&tail, "UID"));
            if wanted && named == Some(u64::from(uid)) {
                body = Some(bytes);
            }
        }
        Err(ConnectError::NotImap.into())
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> Session<S> {
    /// The raw RFC 5322 bytes of message `uid` in the folder opened with [`Session::examine`].
    ///
    /// Refuses with [`BodyError::TooLarge`] when the message is bigger than `max_bytes`. After
    /// `TooLarge` caused by the literal itself, or any `Connect` error, the session is out of
    /// step with the server and must be dropped.
    pub async fn fetch_body(&mut self, uid: u32, max_bytes: u64) -> Result<Vec<u8>, BodyError> {
        if uid == 0 {
            return Err(BodyError::NotFound);
        }
        let lines = self
            .d
            .run(&format!("UID FETCH {uid} (RFC822.SIZE)"))
            .await?;
        let size = lines
            .iter()
            .map(parse::Logical::joined)
            .filter(|text| is_fetch(text) && number_after(text, "UID") == Some(u64::from(uid)))
            .find_map(|text| number_after(&text, "RFC822.SIZE"))
            .ok_or(BodyError::NotFound)?;
        if size > max_bytes {
            return Err(BodyError::TooLarge { size });
        }
        self.d.body_literal(uid, max_bytes).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_items_are_read_as_atoms_not_substrings() {
        assert_eq!(
            number_after("* 3 FETCH (UID 7 RFC822.SIZE 2048)", "UID"),
            Some(7)
        );
        assert_eq!(
            number_after("* 3 FETCH (RFC822.SIZE 2048 UID 7)", "rfc822.size"),
            Some(2048)
        );
        assert_eq!(number_after("* 3 FETCH (XUID 9)", "UID"), None);
        assert_eq!(number_after("* 3 FETCH (UID x)", "UID"), None);
        assert!(is_fetch("* 12 fetch (UID 1)") && !is_fetch("* OK FETCH") && !is_fetch("a1 OK"));
    }
}
