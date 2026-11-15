//! APPEND, Message-ID search and UID-scoped delete against a scripted in-memory server.
use mailbox_core::secrets::Secret;
use mailbox_imap::append::{
    AppendError, AppendFlag, FolderRole, SentCopy, find_folder, save_sent_copy,
};
use mailbox_imap::parse::ListEntry;
use mailbox_imap::{ConnectError, Session};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};

#[derive(Clone, Copy)]
struct Script {
    caps: &'static str,
    /// Reply to the APPEND command line: `+` continues, anything else is sent with the tag.
    append_continue: bool,
    /// Tagged completion of APPEND (after the literal).
    append_done: &'static str,
    search: &'static str,
}

const OK: Script = Script {
    caps: "IMAP4rev1 UIDPLUS SPECIAL-USE",
    append_continue: true,
    append_done: "OK [APPENDUID 38505 3955] APPEND completed",
    search: "* SEARCH\r\n",
};

/// Returns (commands seen without tags, literals received by APPEND).
async fn server(io: DuplexStream, s: Script) -> (Vec<String>, Vec<Vec<u8>>) {
    let (mut seen, mut literals) = (Vec::new(), Vec::new());
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
        let literal_len = |l: &str| -> usize {
            l.trim_end()
                .rsplit('{')
                .next()
                .unwrap()
                .trim_end_matches('}')
                .parse()
                .unwrap()
        };
        let reply = if upper.starts_with("CAPABILITY") {
            format!("* CAPABILITY {}\r\n{tag} OK\r\n", s.caps)
        } else if upper.starts_with("LOGIN") {
            for _ in 0..2 {
                w.write_all(b"+ go\r\n").await.unwrap();
                let mut buf = vec![0u8; literal_len(&line)];
                r.read_exact(&mut buf).await.unwrap();
                line.clear();
                r.read_line(&mut line).await.unwrap();
            }
            format!("{tag} OK\r\n")
        } else if upper.starts_with("APPEND") {
            if !s.append_continue {
                format!("{tag} NO [OVERQUOTA] mailbox full\r\n")
            } else {
                w.write_all(b"+ Ready for literal data\r\n").await.unwrap();
                let mut buf = vec![0u8; literal_len(&line)];
                r.read_exact(&mut buf).await.unwrap();
                literals.push(buf);
                line.clear();
                r.read_line(&mut line).await.unwrap();
                assert_eq!(line, "\r\n", "command ends right after the literal");
                format!("* 12 EXISTS\r\n* 1 RECENT\r\n{tag} {}\r\n", s.append_done)
            }
        } else if upper.starts_with("EXAMINE") || upper.starts_with("SELECT") {
            format!("* 11 EXISTS\r\n* OK [UIDVALIDITY 38505] ok\r\n{tag} OK\r\n")
        } else if upper.starts_with("UID SEARCH") {
            format!("{}{tag} OK\r\n", s.search)
        } else if upper.starts_with("UID STORE") || upper.starts_with("UID EXPUNGE") {
            format!("{tag} OK\r\n")
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
    (seen, literals)
}

type Server = tokio::task::JoinHandle<(Vec<String>, Vec<Vec<u8>>)>;

async fn session(s: Script) -> (Session<DuplexStream>, Server) {
    let (client, srv) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(server(srv, s));
    let mut session = Session::login_stream(client, "user@example.com", &Secret::new("pw"))
        .await
        .unwrap();
    session.refresh_capabilities().await.unwrap();
    (session, task)
}

const MSG: &[u8] =
    b"Message-ID: <d1.abc@example.com>\r\nSubject: Th\xc3\xad\r\n\r\nbody {5}\r\n.\r\n";
const ID: &str = "<d1.abc@example.com>";

#[tokio::test]
async fn append_sends_the_exact_bytes_as_a_literal_and_reads_appenduid() {
    let (mut s, task) = session(OK).await;
    let done = s
        .append("INBOX.Sent", &[AppendFlag::Seen], MSG)
        .await
        .unwrap();
    assert_eq!((done.uid_validity, done.uid), (Some(38505), Some(3955)));
    s.logout().await;
    let (seen, literals) = task.await.unwrap();
    assert!(
        seen.contains(&format!("APPEND \"INBOX.Sent\" (\\Seen) {{{}}}", MSG.len())),
        "{seen:?}"
    );
    assert_eq!(literals, vec![MSG.to_vec()]);
}

#[tokio::test]
async fn append_without_uidplus_reply_still_succeeds() {
    let (mut s, task) = session(Script {
        append_done: "OK done",
        ..OK
    })
    .await;
    let done = s
        .append("Drafts", &[AppendFlag::Seen, AppendFlag::Draft], MSG)
        .await
        .unwrap();
    assert_eq!((done.uid_validity, done.uid), (None, None));
    s.logout().await;
    let (seen, _) = task.await.unwrap();
    assert!(
        seen.iter()
            .any(|c| c.starts_with("APPEND \"Drafts\" (\\Seen \\Draft) {"))
    );
}

#[tokio::test]
async fn refused_append_is_reported_and_nothing_is_retried() {
    for script in [
        Script {
            append_continue: false,
            ..OK
        },
        Script {
            append_done: "NO [TRYCREATE] no such mailbox",
            ..OK
        },
    ] {
        let (mut s, task) = session(script).await;
        assert_eq!(
            s.append("INBOX.Sent", &[AppendFlag::Seen], MSG)
                .await
                .unwrap_err(),
            AppendError::Refused
        );
        s.logout().await;
        let (seen, _) = task.await.unwrap();
        assert_eq!(seen.iter().filter(|c| c.starts_with("APPEND")).count(), 1);
    }
}

#[tokio::test]
async fn unsafe_input_is_refused_before_any_command() {
    let (mut s, task) = session(OK).await;
    assert_eq!(
        s.append("INBOX.Sent", &[], b"a\0b").await.unwrap_err(),
        AppendError::InvalidMessage
    );
    assert_eq!(
        s.append("INBOX.Sent", &[], b"").await.unwrap_err(),
        AppendError::InvalidMessage
    );
    assert_eq!(
        s.append("Sent\r\na1 DELETE INBOX", &[], MSG)
            .await
            .unwrap_err(),
        AppendError::Connection(ConnectError::NotImap)
    );
    for bad in [
        "<a@b>\" OR ALL \"",
        "<a@b>\r\na9 LOGOUT",
        "no-brackets@x",
        "<a b@c>",
        "<>",
        "",
    ] {
        assert_eq!(
            s.find_by_message_id("INBOX.Sent", bad).await.unwrap_err(),
            AppendError::InvalidMessageId,
            "{bad:?}"
        );
    }
    s.logout().await;
    let (seen, literals) = task.await.unwrap();
    assert!(literals.is_empty());
    assert!(
        !seen
            .iter()
            .any(|c| c.starts_with("APPEND") || c.contains("SEARCH") || c.contains("DELETE"))
    );
}

#[tokio::test]
async fn message_id_search_examines_read_only_and_returns_uids() {
    let (mut s, task) = session(Script {
        search: "* SEARCH 7 12\r\n",
        ..OK
    })
    .await;
    assert_eq!(
        s.find_by_message_id("INBOX.Sent", ID).await.unwrap(),
        vec![7, 12]
    );
    s.logout().await;
    let (seen, _) = task.await.unwrap();
    let examine = seen
        .iter()
        .position(|c| c == "EXAMINE \"INBOX.Sent\"")
        .expect("read-only open");
    let search = seen
        .iter()
        .position(|c| c == &format!("UID SEARCH HEADER Message-ID \"{ID}\""))
        .expect("exact search");
    assert!(examine < search);
    assert!(!seen.iter().any(|c| c.starts_with("SELECT")));

    let (mut s, task) = session(OK).await;
    assert!(
        s.find_by_message_id("INBOX.Sent", ID)
            .await
            .unwrap()
            .is_empty()
    );
    s.logout().await;
    task.await.unwrap();
}

#[tokio::test]
async fn sent_copy_is_appended_once_and_skipped_when_the_server_already_has_it() {
    let (mut s, task) = session(OK).await;
    assert_eq!(
        save_sent_copy(&mut s, "INBOX.Sent", ID, MSG).await.unwrap(),
        SentCopy::Appended { uid: Some(3955) }
    );
    s.logout().await;
    let (seen, literals) = task.await.unwrap();
    assert_eq!(seen.iter().filter(|c| c.starts_with("APPEND")).count(), 1);
    assert!(
        seen.iter()
            .any(|c| c.starts_with("APPEND \"INBOX.Sent\" (\\Seen) {"))
    );
    assert_eq!(literals.len(), 1);

    // The provider saved the submitted message itself: no second copy.
    let (mut s, task) = session(Script {
        search: "* SEARCH 41\r\n",
        ..OK
    })
    .await;
    assert_eq!(
        save_sent_copy(&mut s, "INBOX.Sent", ID, MSG).await.unwrap(),
        SentCopy::AlreadyThere
    );
    s.logout().await;
    let (seen, literals) = task.await.unwrap();
    assert!(!seen.iter().any(|c| c.starts_with("APPEND")) && literals.is_empty());
}

#[tokio::test]
async fn delete_is_limited_to_the_given_uids_and_needs_uidplus() {
    let (mut s, task) = session(OK).await;
    s.delete_uids("INBOX.Sent", &[7, 12]).await.unwrap();
    s.delete_uids("INBOX.Sent", &[]).await.unwrap();
    s.logout().await;
    let (seen, _) = task.await.unwrap();
    let tail: Vec<&str> = seen
        .iter()
        .map(String::as_str)
        .filter(|c| !c.starts_with("LOG") && *c != "CAPABILITY")
        .collect();
    assert_eq!(
        tail,
        [
            "SELECT \"INBOX.Sent\"",
            "UID STORE 7,12 +FLAGS.SILENT (\\Deleted)",
            "UID EXPUNGE 7,12"
        ],
        "never a plain EXPUNGE, which would remove other people's deletions too"
    );

    let (mut s, task) = session(Script {
        caps: "IMAP4rev1",
        ..OK
    })
    .await;
    assert_eq!(
        s.delete_uids("INBOX.Sent", &[7]).await.unwrap_err(),
        AppendError::Unsupported
    );
    s.logout().await;
    let (seen, _) = task.await.unwrap();
    assert!(
        !seen
            .iter()
            .any(|c| c.contains("STORE") || c.contains("EXPUNGE") || c.starts_with("SELECT"))
    );
}

fn entry(name: &str, flags: &[&str]) -> ListEntry {
    ListEntry {
        name: name.into(),
        delimiter: Some(".".into()),
        flags: flags.iter().map(|f| f.to_string()).collect(),
    }
}

#[test]
fn sent_and_drafts_are_found_by_special_use_then_by_name() {
    let with_roles = [
        entry("INBOX", &[]),
        entry("INBOX.Sent", &[]),
        entry("INBOX.Outgoing", &["\\Sent"]),
        entry("INBOX.Entwuerfe", &["\\Drafts"]),
    ];
    assert_eq!(
        find_folder(&with_roles, FolderRole::Sent).unwrap().name,
        "INBOX.Outgoing"
    );
    assert_eq!(
        find_folder(&with_roles, FolderRole::Drafts).unwrap().name,
        "INBOX.Entwuerfe"
    );

    let by_name = [
        entry("INBOX", &[]),
        entry("Archive.Sent", &["\\Noselect"]),
        entry("INBOX.Sent Items", &[]),
        entry("INBOX.Drafts", &[]),
        // "Đã gửi" in modified UTF-7.
        entry("&ARAA4w- g&Hu0-i", &[]),
    ];
    assert_eq!(
        find_folder(&by_name, FolderRole::Sent).unwrap().name,
        "INBOX.Sent Items"
    );
    assert_eq!(
        find_folder(&by_name, FolderRole::Drafts).unwrap().name,
        "INBOX.Drafts"
    );
    assert_eq!(
        find_folder(&by_name[4..], FolderRole::Sent).unwrap().name,
        "&ARAA4w- g&Hu0-i"
    );
    assert!(
        find_folder(&by_name[..2], FolderRole::Sent).is_none(),
        "no guess, no \\Noselect"
    );
}
