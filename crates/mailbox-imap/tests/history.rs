//! MB-200/201: UID-range and UID-set envelope fetches with threading headers, against a
//! scripted in-memory server. Asserts the session stays read-only (`BODY.PEEK`, no STORE).
use mailbox_core::history::RemoteHistory;
use mailbox_core::secrets::Secret;
use mailbox_imap::Session;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};

const REPLY: &str = "From: B <b@example.org>\r\nTo: =?UTF-8?B?VHLhuqduIExpbmg=?= <linh@example.org>, c@example.org\r\nCc: d@example.org\r\nSubject: Re: Plan\r\nMessage-ID: <reply@example.org>\r\nIn-Reply-To: <root@example.org>\r\nReferences: <root@example.org>\r\n <mid@example.org>\r\n\r\n";
const ROOT: &str = "From: a@example.org\r\nSubject: Plan\r\nMessage-ID: <root@example.org>\r\n\r\n";

fn fetch(seq: u32, uid: u32, headers: &str) -> String {
    format!(
        "* {seq} FETCH (UID {uid} FLAGS () INTERNALDATE \"14-Nov-2026 10:05:00 +0700\" RFC822.SIZE 99 BODY[HEADER.FIELDS (FROM SUBJECT DATE MESSAGE-ID IN-REPLY-TO REFERENCES TO CC)] {{{}}}\r\n{headers})\r\n",
        headers.len()
    )
}

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
            format!("* CAPABILITY IMAP4rev1\r\n{tag} OK\r\n")
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
        } else if upper.starts_with("EXAMINE") {
            format!("* 9 EXISTS\r\n* OK [UIDVALIDITY 5] ok\r\n{tag} OK [READ-ONLY]\r\n")
        } else if upper.starts_with("UID FETCH 1:40 ") {
            // A server may answer with a UID outside the range; the client must drop it.
            format!("{}{}{tag} OK\r\n", fetch(1, 7, ROOT), fetch(9, 99, ROOT))
        } else if upper.starts_with("UID FETCH 7,12 ") {
            format!("{}{tag} OK\r\n", fetch(2, 12, REPLY))
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
async fn uid_range_and_set_fetches_carry_threading_headers_and_stay_read_only() {
    let (client, srv) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(server(srv));
    let mut s = Session::login_stream(client, "user@example.com", &Secret::new("pw"))
        .await
        .unwrap();
    s.examine("INBOX").await.unwrap();

    let range = RemoteHistory::fetch_uid_range(&mut s, 1, 40).await.unwrap();
    assert_eq!(range.iter().map(|e| e.uid).collect::<Vec<_>>(), vec![7]);
    assert_eq!(range[0].message_id.as_deref(), Some("<root@example.org>"));
    assert!(range[0].headers.references.is_empty());

    let set = RemoteHistory::fetch_uid_set(&mut s, &[7, 12])
        .await
        .unwrap();
    assert_eq!(set.len(), 1);
    let h = &set[0].headers;
    assert_eq!(h.in_reply_to.as_deref(), Some("<root@example.org>"));
    assert_eq!(
        h.references,
        vec!["<root@example.org>", "<mid@example.org>"]
    );
    assert_eq!(h.to, "Trần Linh <linh@example.org>, c@example.org");
    assert_eq!(h.cc, "d@example.org");

    // Degenerate inputs send nothing.
    assert!(s.fetch_uid_range(5, 4).await.unwrap().is_empty());
    assert!(s.fetch_uid_set(&[]).await.unwrap().is_empty());
    s.logout().await;

    let seen = task.await.unwrap();
    let fetches: Vec<&String> = seen.iter().filter(|c| c.contains("FETCH")).collect();
    assert_eq!(fetches.len(), 2);
    assert!(fetches.iter().all(|c| c.contains("BODY.PEEK[")));
    assert!(
        !seen
            .iter()
            .any(|c| c.to_ascii_uppercase().contains("STORE"))
    );
}
