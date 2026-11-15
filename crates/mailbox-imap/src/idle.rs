//! IMAP IDLE (RFC 2177) for live updates (MB-140).
//!
//! - `IDLE` is only used when the server advertises it; the caller polls otherwise.
//! - Waiting is bounded by the caller's `max` (the worker re-issues before 29 minutes) and can
//!   be interrupted (another use case wants the connection, the user asked for a sync). Every
//!   way out sends `DONE` and reads the tagged completion before returning, so no other command
//!   is ever written while the server is still idling.
//! - Reading while idle is cancel-safe: the wait only looks at the socket buffer (`fill_buf`);
//!   a whole line is read only once data has arrived.
//! - `BYE` or a closed socket is reported as `Unreachable`: a connection state, not a failure of
//!   the account. Server text never leaves this module.
use crate::{ConnectError, Session};
use std::future::Future;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite};

/// One untagged notification received while idling (or with the `DONE` completion).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleEvent {
    /// `* n EXISTS`: the folder now has `n` messages.
    Exists(u32),
    /// `* n EXPUNGE`: message sequence number `n` was removed.
    Expunge(u32),
    /// `* n FETCH (FLAGS ...)`: flags of a message changed.
    FlagsChanged,
    /// `* VANISHED` (QRESYNC servers).
    Vanished,
}

/// Why the wait ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleEnd<T> {
    /// The server reported a change to the folder.
    Changed,
    /// `max` elapsed without a change.
    TimedOut,
    /// The interrupt future finished first; its value.
    Interrupted(T),
    /// The server answered `IDLE` with NO/BAD: poll on this connection instead.
    Refused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdleOutcome<T> {
    pub end: IdleEnd<T>,
    pub events: Vec<IdleEvent>,
}

enum Untagged {
    Event(IdleEvent),
    Bye,
    Other,
}

fn untagged(line: &str) -> Untagged {
    let mut words = line.split_ascii_whitespace();
    if words.next() != Some("*") {
        return Untagged::Other;
    }
    let (Some(first), second) = (words.next(), words.next()) else {
        return Untagged::Other;
    };
    if first.eq_ignore_ascii_case("BYE") {
        return Untagged::Bye;
    }
    if first.eq_ignore_ascii_case("VANISHED") {
        return Untagged::Event(IdleEvent::Vanished);
    }
    let (Ok(n), Some(kind)) = (first.parse::<u32>(), second) else {
        return Untagged::Other;
    };
    match kind.to_ascii_uppercase().as_str() {
        "EXISTS" => Untagged::Event(IdleEvent::Exists(n)),
        "EXPUNGE" => Untagged::Event(IdleEvent::Expunge(n)),
        "FETCH" => Untagged::Event(IdleEvent::FlagsChanged),
        _ => Untagged::Other,
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> Session<S> {
    /// Whether the server advertised `IDLE` (call after login; capabilities are refreshed by
    /// [`crate::mutate::MutableSession::upgrade`] and [`Session::refresh_capabilities`]).
    pub fn supports_idle(&self) -> bool {
        self.info.has("IDLE")
    }

    /// Idle on the currently opened folder until a change, `max`, or `interrupt` — whichever
    /// comes first — then end the idle with `DONE`.
    pub async fn idle<T, F: Future<Output = T>>(
        &mut self,
        max: Duration,
        interrupt: F,
    ) -> Result<IdleOutcome<T>, ConnectError> {
        let tag = self.d.next_tag();
        self.d.send(format!("{tag} IDLE\r\n").as_bytes()).await?;
        let mut events = Vec::new();
        // Up to the continuation request; a tagged answer here means the server will not idle.
        loop {
            let line = self.d.line().await?;
            if line.starts_with('+') {
                break;
            }
            if line
                .strip_prefix(tag.as_str())
                .is_some_and(|r| r.starts_with(' '))
            {
                return Ok(IdleOutcome {
                    end: IdleEnd::Refused,
                    events,
                });
            }
            match untagged(&line) {
                Untagged::Bye => return Err(ConnectError::Unreachable),
                Untagged::Event(e) => events.push(e),
                Untagged::Other if line.starts_with('*') => {}
                Untagged::Other => return Err(ConnectError::NotImap),
            }
        }
        let deadline = tokio::time::Instant::now() + max;
        let mut interrupt = std::pin::pin!(interrupt);
        let end = loop {
            if !events.is_empty() {
                break IdleEnd::Changed;
            }
            let arrived = tokio::select! {
                biased;
                ready = self.d.stream.fill_buf() => match ready {
                    Ok(buf) => !buf.is_empty(),
                    Err(_) => false,
                },
                value = &mut interrupt => break IdleEnd::Interrupted(value),
                () = tokio::time::sleep_until(deadline) => break IdleEnd::TimedOut,
            };
            if !arrived {
                return Err(ConnectError::Unreachable);
            }
            let line = self.d.line().await.map_err(|_| ConnectError::Unreachable)?;
            match untagged(&line) {
                Untagged::Bye => return Err(ConnectError::Unreachable),
                Untagged::Event(e) => events.push(e),
                Untagged::Other if line.starts_with('*') => {}
                Untagged::Other => return Err(ConnectError::NotImap),
            }
        };
        self.d.send(b"DONE\r\n").await?;
        let (_, tail) = self.d.completion(&tag).await?;
        for line in &tail {
            match untagged(line) {
                Untagged::Bye => return Err(ConnectError::Unreachable),
                Untagged::Event(e) => events.push(e),
                Untagged::Other => {}
            }
        }
        Ok(IdleOutcome { end, events })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untagged_lines_are_classified_without_keeping_server_text() {
        assert!(matches!(
            untagged("* 12 EXISTS"),
            Untagged::Event(IdleEvent::Exists(12))
        ));
        assert!(matches!(
            untagged("* 3 expunge"),
            Untagged::Event(IdleEvent::Expunge(3))
        ));
        assert!(matches!(
            untagged("* 3 FETCH (FLAGS (\\Seen))"),
            Untagged::Event(IdleEvent::FlagsChanged)
        ));
        assert!(matches!(
            untagged("* VANISHED 4:5"),
            Untagged::Event(IdleEvent::Vanished)
        ));
        assert!(matches!(untagged("* BYE idle too long"), Untagged::Bye));
        assert!(matches!(untagged("* 0 RECENT"), Untagged::Other));
        assert!(matches!(untagged("* OK still here"), Untagged::Other));
        assert!(matches!(untagged("garbage"), Untagged::Other));
    }
}
