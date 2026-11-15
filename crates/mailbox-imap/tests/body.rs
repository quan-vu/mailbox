//! `Session::fetch_body` against a scripted in-memory server. Synthetic data only.
use mailbox_core::secrets::Secret;
use mailbox_imap::body::BodyError;
use mailbox_imap::{ConnectError, Session};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};

const SMALL: &[u8] = b"Subject: hi\r\n\r\nbody with a tricky tail\r\n)\r\na1 OK fake\r\n";

fn big() -> Vec<u8> {
    // Larger than the envelope literal cap and the pipe buffer; not valid UTF-8.
    (0..200_000u32).map(|i| (i % 251) as u8).collect()
}

/// Scripted server. Returns every command line it received (login literals excluded).
async fn server(io: DuplexStream) -> Vec<String> {
    let mut seen = Vec::new();
    let (r, mut w) = tokio::io::split(io);
    let mut r = BufReader::new(r);
    w.write_all(b"* OK ready\r\n").await.unwrap();
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).await.unwrap_or(0) == 0 {
            break;
        }
        let cmd = line.trim_end().to_string();
        let tag = cmd.split(' ').next().unwrap_or("").to_string();
        let body = cmd[tag.len()..].trim().to_string();
        seen.push(body.clone());
        let mut out: Vec<u8> = Vec::new();
        let size =
            |uid: u32, n: usize| format!("* 3 FETCH (UID {uid} RFC822.SIZE {n})\r\n{tag} OK\r\n");
        match body.as_str() {
            "CAPABILITY" => out.extend(format!("* CAPABILITY IMAP4rev1\r\n{tag} OK\r\n").bytes()),
            b if b.starts_with("LOGIN") => {
                for _ in 0..2 {
                    w.write_all(b"+ go\r\n").await.unwrap();
                    let n: usize = line
                        .trim_end()
                        .rsplit('{')
                        .next()
                        .unwrap()
                        .trim_end_matches('}')
                        .parse()
                        .unwrap();
                    let mut buf = vec![0u8; n];
                    r.read_exact(&mut buf).await.unwrap();
                    line.clear();
                    r.read_line(&mut line).await.unwrap();
                }
                out.extend(format!("{tag} OK\r\n").bytes());
            }
            "EXAMINE \"INBOX\"" => out.extend(
                format!("* 3 EXISTS\r\n* OK [UIDVALIDITY 777] ok\r\n{tag} OK [READ-ONLY]\r\n")
                    .bytes(),
            ),
            "UID FETCH 7 (RFC822.SIZE)" => out.extend(size(7, SMALL.len()).bytes()),
            "UID FETCH 7 (BODY.PEEK[])" => {
                // Unsolicited data around the answer must be ignored.
                out.extend(b"* 4 EXISTS\r\n* 2 FETCH (FLAGS (\\Seen))\r\n");
                out.extend(format!("* 3 FETCH (UID 7 BODY[] {{{}}}\r\n", SMALL.len()).bytes());
                out.extend(SMALL);
                out.extend(format!(")\r\n* 1 RECENT\r\n{tag} OK\r\n").bytes());
            }
            "UID FETCH 8 (RFC822.SIZE)" => out.extend(size(8, 50_000_000).bytes()),
            "UID FETCH 9 (RFC822.SIZE)" => out.extend(size(9, 10).bytes()),
            // The size answer was a lie: the literal is far larger than allowed.
            "UID FETCH 9 (BODY.PEEK[])" => out.extend(b"* 3 FETCH (UID 9 BODY[] {999999999}\r\n"),
            "UID FETCH 10 (RFC822.SIZE)" => out.extend(format!("{tag} OK nothing\r\n").bytes()),
            "UID FETCH 11 (RFC822.SIZE)" => out.extend(size(11, 5).bytes()),
            "UID FETCH 11 (BODY.PEEK[])" => out.extend(
                format!("* 1 FETCH (BODY[] {{5}}\r\nhello UID 11 FLAGS (\\Seen))\r\n{tag} OK\r\n")
                    .bytes(),
            ),
            "UID FETCH 12 (RFC822.SIZE)" => out.extend(size(12, 5).bytes()),
            // Answers about another message must not be taken for the requested one.
            "UID FETCH 12 (BODY.PEEK[])" => out.extend(
                format!("* 1 FETCH (UID 99 BODY[] {{5}}\r\nwrong)\r\n{tag} OK\r\n").bytes(),
            ),
            "UID FETCH 13 (RFC822.SIZE)" => out.extend(size(13, 200_000).bytes()),
            "UID FETCH 13 (BODY.PEEK[])" => {
                out.extend(b"* 3 FETCH (UID 13 BODY[] {200000}\r\n");
                out.extend(big());
                out.extend(format!(")\r\n{tag} OK\r\n").bytes());
            }
            "UID FETCH 14 (RFC822.SIZE)" => out.extend(size(14, 5).bytes()),
            "UID FETCH 14 (BODY.PEEK[])" => {
                out.extend(format!("* 3 FETCH (UID 14 BODY[] NIL)\r\n{tag} OK\r\n").bytes())
            }
            "UID FETCH 15 (RFC822.SIZE)" => out.extend(size(15, 100).bytes()),
            "UID FETCH 15 (BODY.PEEK[])" => {
                // Connection dies in the middle of the literal.
                w.write_all(b"* 3 FETCH (UID 15 BODY[] {100}\r\nonly this")
                    .await
                    .unwrap();
                break;
            }
            "UID FETCH 16 (RFC822.SIZE)" => out.extend(format!("{tag} NO not now\r\n").bytes()),
            "LOGOUT" => {
                w.write_all(format!("* BYE\r\n{tag} OK\r\n").as_bytes())
                    .await
                    .unwrap();
                break;
            }
            _ => out.extend(format!("{tag} BAD unknown\r\n").bytes()),
        }
        w.write_all(&out).await.unwrap();
    }
    seen
}

async fn session() -> (Session<DuplexStream>, tokio::task::JoinHandle<Vec<String>>) {
    let (client, srv) = tokio::io::duplex(4096);
    let task = tokio::spawn(server(srv));
    let mut s = Session::login_stream(client, "user@example.com", &Secret::new("pw"))
        .await
        .unwrap();
    s.examine("INBOX").await.unwrap();
    (s, task)
}

const MAX: u64 = 1024 * 1024;

#[tokio::test]
async fn fetches_bodies_exactly_and_read_only() {
    let (mut s, task) = session().await;
    assert_eq!(
        s.fetch_body(7, MAX).await.unwrap(),
        SMALL,
        "literal bytes are taken verbatim"
    );
    assert_eq!(
        s.fetch_body(13, MAX).await.unwrap(),
        big(),
        "large binary literal in chunks"
    );
    assert_eq!(
        s.fetch_body(11, MAX).await.unwrap(),
        b"hello",
        "UID after the literal"
    );
    // The session is still in sync after each of these.
    assert_eq!(
        s.fetch_body(8, MAX).await,
        Err(BodyError::TooLarge { size: 50_000_000 })
    );
    assert_eq!(s.fetch_body(10, MAX).await, Err(BodyError::NotFound));
    assert_eq!(s.fetch_body(12, MAX).await, Err(BodyError::NotFound));
    assert_eq!(s.fetch_body(14, MAX).await, Err(BodyError::NotFound));
    assert_eq!(s.fetch_body(0, MAX).await, Err(BodyError::NotFound));
    assert_eq!(
        s.fetch_body(16, MAX).await,
        Err(BodyError::Connect(ConnectError::NotImap))
    );
    assert_eq!(s.fetch_body(7, MAX).await.unwrap(), SMALL);
    s.logout().await;

    let seen = task.await.unwrap();
    assert!(
        !seen.iter().any(|c| c.contains("FETCH 8 (BODY")),
        "an oversized body is never requested"
    );
    assert!(
        !seen.iter().any(|c| c.contains("FETCH 0")),
        "UID 0 is never sent"
    );
    for c in seen.iter().filter(|c| c.contains("FETCH")) {
        assert!(
            c.contains("RFC822.SIZE") || c.contains("BODY.PEEK[]"),
            "unexpected fetch: {c}"
        );
        assert!(
            !c.contains("BODY[") || c.contains("BODY.PEEK["),
            "must not set \\Seen: {c}"
        );
    }
    for forbidden in [
        "SELECT", "STORE", "EXPUNGE", "DELETE", "APPEND", "COPY", "MOVE",
    ] {
        assert!(
            !seen
                .iter()
                .any(|c| c.to_ascii_uppercase().contains(forbidden)),
            "{forbidden} was sent"
        );
    }
}

#[tokio::test]
async fn a_lying_server_cannot_make_the_client_allocate_or_read_past_the_limit() {
    let (mut s, _task) = session().await;
    assert_eq!(
        s.fetch_body(9, MAX).await,
        Err(BodyError::TooLarge { size: 999_999_999 })
    );
}

#[tokio::test]
async fn a_connection_lost_mid_literal_is_an_error_not_a_short_body() {
    let (mut s, _task) = session().await;
    assert_eq!(
        s.fetch_body(15, MAX).await,
        Err(BodyError::Connect(ConnectError::NotImap))
    );
}
