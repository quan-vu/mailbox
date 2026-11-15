//! Read-only session tests against a scripted in-memory server.
use mailbox_core::secrets::Secret;
use mailbox_imap::{ConnectError, Session};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};

const HDR_A: &str = "From: \"Tr\u{1ea7}n Linh\" <linh@company.vn>\r\nSubject: Xin ch\u{e0}o\r\nMessage-ID: <a@company.vn>\r\n\r\n";
const HDR_B: &str = "From: ketoan@company.vn\r\nSubject: =?UTF-8?B?SMOzYSDEkcahbg==?=\r\n\r\n";

fn fetch(seq: u32, uid: u32, flags: &str, headers: &str) -> String {
    format!(
        "* {seq} FETCH (UID {uid} FLAGS ({flags}) INTERNALDATE \"14-Nov-2026 10:05:00 +0700\" RFC822.SIZE 2048 BODY[HEADER.FIELDS (FROM SUBJECT DATE MESSAGE-ID)] {{{}}}\r\n{headers})\r\n",
        headers.len()
    )
}

/// Returns the commands the client sent (without tags' literal payloads).
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
        let upper = body.to_ascii_uppercase();
        let reply = if upper.starts_with("CAPABILITY") {
            format!("* CAPABILITY IMAP4rev1 IDLE SPECIAL-USE\r\n{tag} OK\r\n")
        } else if upper.starts_with("LOGIN") {
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
            format!("{tag} OK\r\n")
        } else if upper.starts_with("LIST") {
            format!(
                "* LIST (\\HasNoChildren) \".\" INBOX\r\n* LIST (\\HasNoChildren \\Sent) \".\" \"INBOX.Sent\"\r\n* LIST (\\Noselect) \".\" \"INBOX.Parent\"\r\n* LIST (\\Trash) \".\" {{11}}\r\nINBOX.Trash\r\n{tag} OK\r\n"
            )
        } else if upper.starts_with("EXAMINE \"INBOX\"") {
            format!(
                "* FLAGS (\\Seen)\r\n* 3 EXISTS\r\n* OK [UIDVALIDITY 777] ok\r\n* OK [UIDNEXT 121] ok\r\n{tag} OK [READ-ONLY]\r\n"
            )
        } else if upper.starts_with("EXAMINE") {
            format!("{tag} NO no such mailbox\r\n")
        } else if upper.starts_with("FETCH 2:3") {
            format!(
                "{}{}{tag} OK\r\n",
                fetch(2, 119, "\\Seen", HDR_A),
                fetch(3, 120, "\\Flagged", HDR_B)
            )
        } else if upper.starts_with("UID FETCH 121:*") {
            // Nothing new: the server still returns the last message (UID 120).
            format!("{}{tag} OK\r\n", fetch(3, 120, "\\Flagged", HDR_B))
        } else if upper.starts_with("UID FETCH 120:*") {
            format!("{}{tag} OK\r\n", fetch(3, 120, "\\Flagged", HDR_B))
        } else if upper.starts_with("LOGOUT") {
            w.write_all(format!("* BYE\r\n{tag} OK\r\n").as_bytes())
                .await
                .unwrap();
            break;
        } else {
            format!("{tag} BAD unknown\r\n")
        };
        w.write_all(reply.as_bytes()).await.unwrap();
    }
    seen
}

#[tokio::test]
async fn read_only_session_lists_examines_and_fetches() {
    let (client, srv) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(server(srv));
    let mut s = Session::login_stream(client, "user@example.com", &Secret::new("pw"))
        .await
        .unwrap();

    let folders = s.list_folders().await.unwrap();
    let names: Vec<_> = folders
        .iter()
        .map(|f| (f.name.as_str(), f.special_use(), f.selectable()))
        .collect();
    assert_eq!(
        names,
        vec![
            ("INBOX", Some("inbox"), true),
            ("INBOX.Sent", Some("sent"), true),
            ("INBOX.Parent", None, false),
            ("INBOX.Trash", Some("trash"), true)
        ]
    );

    let status = s.examine("INBOX").await.unwrap();
    assert_eq!(
        (status.exists, status.uid_validity, status.uid_next),
        (3, Some(777), Some(121))
    );

    let latest = s.fetch_latest(status.exists, 2).await.unwrap();
    assert_eq!(
        latest.iter().map(|e| e.uid).collect::<Vec<_>>(),
        vec![119, 120]
    );
    assert_eq!(
        (
            latest[0].from_display.as_str(),
            latest[0].subject.as_str(),
            latest[0].seen
        ),
        ("Trần Linh", "Xin chào", true)
    );
    assert_eq!(
        (latest[1].subject.as_str(), latest[1].flagged),
        ("Hóa đơn", true)
    );

    assert!(
        s.fetch_from_uid(121).await.unwrap().is_empty(),
        "the echoed last message is not new"
    );
    assert_eq!(s.fetch_from_uid(120).await.unwrap().len(), 1);
    assert!(s.fetch_latest(0, 50).await.unwrap().is_empty());
    assert_eq!(s.examine("Missing").await, Err(ConnectError::NotImap));
    assert_eq!(s.examine("bad\r\nname").await, Err(ConnectError::NotImap));
    s.logout().await;

    // Read-only guarantee: nothing that could change the mailbox was ever sent.
    let seen = task.await.unwrap();
    for forbidden in [
        "SELECT", "STORE", "EXPUNGE", "DELETE", "APPEND", "COPY", "MOVE", "CREATE", "RENAME",
    ] {
        assert!(
            !seen
                .iter()
                .any(|c| c.to_ascii_uppercase().starts_with(forbidden)
                    || c.to_ascii_uppercase()
                        .starts_with(&format!("UID {forbidden}"))),
            "{forbidden} was sent"
        );
    }
    assert!(
        seen.iter()
            .filter(|c| c.contains("FETCH"))
            .all(|c| c.contains("BODY.PEEK[")),
        "FETCH must not set \\Seen"
    );
}
