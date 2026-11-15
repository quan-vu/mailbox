//! Read-write session paths (MB-132, MB-144) against a scripted in-memory server: every command
//! path, with and without MOVE / UIDPLUS, refusals and a dropped connection.
use mailbox_core::actions::{Expunged, MessageFlag, Moved, RemoteFailure};
use mailbox_core::folders::FolderSource;
use mailbox_core::secrets::Secret;
use mailbox_imap::Session;
use mailbox_imap::mutate::MutableSession;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};

#[derive(Clone)]
struct Script {
    /// Capabilities announced after login (servers reveal MOVE/UIDPLUS only then).
    caps: &'static str,
    /// Where the COPYUID response code is sent: "tagged", "untagged" or "none".
    copyuid: &'static str,
    /// Commands starting with this get a tagged NO.
    refuse: &'static str,
    /// Commands starting with this get a tagged BAD.
    bad: &'static str,
    /// The connection is closed when a command starting with this arrives.
    drop_on: &'static str,
    read_only: bool,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            caps: "IMAP4rev1 MOVE UIDPLUS SPECIAL-USE",
            copyuid: "tagged",
            refuse: "-",
            bad: "-",
            drop_on: "-",
            read_only: false,
        }
    }
}

/// Returns the commands received, without tags.
async fn server(io: DuplexStream, script: Script) -> Vec<String> {
    let mut seen = Vec::new();
    let mut logged_in = false;
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
        let copyuid = |kind: &str| {
            if script.copyuid == kind {
                " [COPYUID 77 5 42]"
            } else {
                ""
            }
        };
        let reply = if upper.starts_with(script.drop_on) {
            break;
        } else if upper.starts_with(script.refuse) {
            format!("{tag} NO [CANNOT] not allowed\r\n")
        } else if upper.starts_with(script.bad) {
            format!("{tag} BAD unknown command\r\n")
        } else if upper.starts_with("CAPABILITY") {
            let caps = if logged_in {
                script.caps
            } else {
                "IMAP4rev1 AUTH=PLAIN"
            };
            format!("* CAPABILITY {caps}\r\n{tag} OK\r\n")
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
            logged_in = true;
            format!("{tag} OK\r\n")
        } else if upper.starts_with("SELECT") {
            let mode = if script.read_only {
                "READ-ONLY"
            } else {
                "READ-WRITE"
            };
            format!(
                "* FLAGS (\\Seen \\Flagged \\Deleted)\r\n* 3 EXISTS\r\n* OK [UIDVALIDITY 777] ok\r\n* OK [UIDNEXT 8] ok\r\n{tag} OK [{mode}] done\r\n"
            )
        } else if upper.starts_with("UID FETCH 5 (UID)") {
            format!("* 1 FETCH (UID 5)\r\n{tag} OK\r\n")
        } else if upper.starts_with("UID FETCH 9 (UID)") {
            format!("{tag} OK no such message\r\n")
        } else if upper.starts_with("UID FETCH 5:7 (UID FLAGS)") {
            // Attribute order differs between servers; a literal must not derail the reader.
            format!(
                "* 1 FETCH (UID 5 FLAGS (\\Seen))\r\n* 2 FETCH (FLAGS (\\Flagged \\Seen $Label) UID 6)\r\n* OK {{4}}\r\nnote\r\n* 3 FETCH (UID 7 FLAGS ())\r\n{tag} OK\r\n"
            )
        } else if upper.starts_with("UID FETCH 1:2 (UID FLAGS)") {
            format!("* 1 FETCH (FLAGS (\\Seen))\r\n{tag} OK\r\n")
        } else if upper.starts_with("UID STORE") {
            format!("* 1 FETCH (UID 5 FLAGS (\\Seen))\r\n{tag} OK\r\n")
        } else if upper.starts_with("UID MOVE") {
            format!(
                "* OK{} moved\r\n* 1 EXPUNGE\r\n{tag} OK{} done\r\n",
                copyuid("untagged"),
                copyuid("tagged")
            )
        } else if upper.starts_with("UID COPY") {
            format!("{tag} OK{} copied\r\n", copyuid("tagged"))
        } else if upper.starts_with("UID EXPUNGE") {
            format!("* 1 EXPUNGE\r\n{tag} OK\r\n")
        } else if upper.starts_with("LIST") {
            format!(
                "* LIST (\\HasNoChildren) \".\" INBOX\r\n* LIST (\\HasNoChildren \\Trash) \".\" \"INBOX.Trash\"\r\n* LIST (\\Noselect) \".\" \"INBOX.Parent\"\r\n* LIST () \".\" \"INBOX.L&AbA-u tr&Hu8-\"\r\n{tag} OK\r\n"
            )
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

async fn open(
    script: Script,
) -> (
    MutableSession<DuplexStream>,
    tokio::task::JoinHandle<Vec<String>>,
) {
    let (client, srv) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(server(srv, script));
    let session = Session::login_stream(client, "user@example.com", &Secret::new("pw"))
        .await
        .unwrap();
    (MutableSession::upgrade(session).await.unwrap(), task)
}

/// Commands after login, tags removed.
async fn finish(
    rw: MutableSession<DuplexStream>,
    task: tokio::task::JoinHandle<Vec<String>>,
) -> Vec<String> {
    rw.into_reader().logout().await;
    let seen = task.await.unwrap();
    let start = seen.iter().position(|c| c == "CAPABILITY").unwrap();
    seen[start + 1..]
        .iter()
        .filter(|c| !c.starts_with("LOGIN") && *c != "CAPABILITY" && *c != "LOGOUT")
        .cloned()
        .collect()
}

fn assert_no_plain_expunge(commands: &[String]) {
    assert!(
        !commands
            .iter()
            .any(|c| c.to_ascii_uppercase().starts_with("EXPUNGE")),
        "a plain EXPUNGE would delete other people's \\Deleted messages: {commands:?}"
    );
}

#[tokio::test]
async fn capabilities_are_read_again_after_login_and_select_opens_read_write() {
    let (mut rw, task) = open(Script::default()).await;
    assert!(rw.can_move() && rw.has_uidplus());
    let selected = rw.select("INBOX").await.unwrap();
    assert_eq!(
        (selected.uid_validity, selected.read_only),
        (Some(777), false)
    );
    assert_eq!(finish(rw, task).await, vec!["SELECT \"INBOX\""]);

    let (mut rw, task) = open(Script {
        read_only: true,
        caps: "IMAP4rev1",
        ..Script::default()
    })
    .await;
    assert!(!rw.can_move() && !rw.has_uidplus());
    assert!(rw.select("INBOX").await.unwrap().read_only);
    finish(rw, task).await;
}

#[tokio::test]
async fn flags_are_stored_by_uid_with_plus_and_minus() {
    let (mut rw, task) = open(Script::default()).await;
    rw.select("INBOX").await.unwrap();
    rw.store_flags(5, true, &[MessageFlag::Seen]).await.unwrap();
    rw.store_flags(5, false, &[MessageFlag::Flagged])
        .await
        .unwrap();
    rw.store_flags(5, true, &[MessageFlag::Seen, MessageFlag::Flagged])
        .await
        .unwrap();
    assert!(rw.uid_exists(5).await.unwrap());
    assert!(!rw.uid_exists(9).await.unwrap());
    assert_eq!(
        finish(rw, task).await,
        vec![
            "SELECT \"INBOX\"",
            "UID STORE 5 +FLAGS (\\Seen)",
            "UID STORE 5 -FLAGS (\\Flagged)",
            "UID STORE 5 +FLAGS (\\Seen \\Flagged)",
            "UID FETCH 5 (UID)",
            "UID FETCH 9 (UID)",
        ]
    );
}

#[tokio::test]
async fn mutations_before_select_are_refused_locally() {
    let (mut rw, task) = open(Script::default()).await;
    assert_eq!(
        rw.store_flags(5, true, &[MessageFlag::Seen]).await,
        Err(RemoteFailure::Protocol)
    );
    assert_eq!(
        rw.move_to(5, "INBOX.Trash").await,
        Err(RemoteFailure::Protocol)
    );
    assert_eq!(rw.delete_forever(5).await, Err(RemoteFailure::Protocol));
    assert!(finish(rw, task).await.is_empty(), "nothing was sent");
}

#[tokio::test]
async fn move_uses_uid_move_and_reads_copyuid_from_either_response() {
    for place in ["tagged", "untagged"] {
        let (mut rw, task) = open(Script {
            copyuid: place,
            ..Script::default()
        })
        .await;
        rw.select("INBOX").await.unwrap();
        assert_eq!(
            rw.move_to(5, "INBOX.Trash").await.unwrap(),
            Moved {
                new_uid: Some((77, 42)),
                source_left_deleted: false
            },
            "{place}"
        );
        let commands = finish(rw, task).await;
        assert_eq!(
            commands,
            vec!["SELECT \"INBOX\"", "UID MOVE 5 \"INBOX.Trash\""]
        );
    }
    // No COPYUID at all: the move happened, the new UID is simply unknown.
    let (mut rw, task) = open(Script {
        copyuid: "none",
        ..Script::default()
    })
    .await;
    rw.select("INBOX").await.unwrap();
    assert_eq!(rw.move_to(5, "INBOX.Trash").await.unwrap().new_uid, None);
    finish(rw, task).await;
}

#[tokio::test]
async fn without_move_it_copies_flags_deleted_and_expunges_only_that_uid() {
    let (mut rw, task) = open(Script {
        caps: "IMAP4rev1 UIDPLUS",
        ..Script::default()
    })
    .await;
    rw.select("INBOX").await.unwrap();
    assert_eq!(
        rw.move_to(5, "INBOX.Trash").await.unwrap(),
        Moved {
            new_uid: Some((77, 42)),
            source_left_deleted: false
        }
    );
    let commands = finish(rw, task).await;
    assert_eq!(
        commands,
        vec![
            "SELECT \"INBOX\"",
            "UID COPY 5 \"INBOX.Trash\"",
            "UID STORE 5 +FLAGS.SILENT (\\Deleted)",
            "UID EXPUNGE 5",
        ]
    );
    assert_no_plain_expunge(&commands);
}

#[tokio::test]
async fn without_move_and_uidplus_the_original_stays_flagged_deleted() {
    let (mut rw, task) = open(Script {
        caps: "IMAP4rev1",
        copyuid: "none",
        ..Script::default()
    })
    .await;
    rw.select("INBOX").await.unwrap();
    assert_eq!(
        rw.move_to(5, "INBOX.Trash").await.unwrap(),
        Moved {
            new_uid: None,
            source_left_deleted: true
        }
    );
    assert_eq!(rw.delete_forever(5).await.unwrap(), Expunged::LeftDeleted);
    let commands = finish(rw, task).await;
    assert_eq!(
        commands,
        vec![
            "SELECT \"INBOX\"",
            "UID COPY 5 \"INBOX.Trash\"",
            "UID STORE 5 +FLAGS.SILENT (\\Deleted)",
            "UID STORE 5 +FLAGS.SILENT (\\Deleted)",
        ]
    );
    assert_no_plain_expunge(&commands);
}

#[tokio::test]
async fn permanent_delete_with_uidplus_expunges_one_uid() {
    let (mut rw, task) = open(Script::default()).await;
    rw.select("INBOX.Trash").await.unwrap();
    assert_eq!(rw.delete_forever(5).await.unwrap(), Expunged::Removed);
    let commands = finish(rw, task).await;
    assert_eq!(
        commands,
        vec![
            "SELECT \"INBOX.Trash\"",
            "UID STORE 5 +FLAGS.SILENT (\\Deleted)",
            "UID EXPUNGE 5",
        ]
    );
    assert_no_plain_expunge(&commands);
}

#[tokio::test]
async fn no_and_bad_answers_are_refusals_and_the_session_stays_usable() {
    for (refuse, bad) in [("UID STORE", "-"), ("-", "UID STORE")] {
        let (mut rw, task) = open(Script {
            refuse,
            bad,
            ..Script::default()
        })
        .await;
        rw.select("INBOX").await.unwrap();
        assert_eq!(
            rw.store_flags(5, true, &[MessageFlag::Seen]).await,
            Err(RemoteFailure::Refused)
        );
        assert!(rw.uid_exists(5).await.unwrap(), "next command still works");
        finish(rw, task).await;
    }
    for refuse in ["UID MOVE", "SELECT"] {
        let (mut rw, task) = open(Script {
            refuse,
            ..Script::default()
        })
        .await;
        let selected = rw.select("INBOX").await;
        if refuse == "SELECT" {
            assert_eq!(selected.err(), Some(RemoteFailure::Refused));
        } else {
            assert_eq!(
                rw.move_to(5, "INBOX.Trash").await,
                Err(RemoteFailure::Refused)
            );
        }
        finish(rw, task).await;
    }
    // Copy refused (e.g. destination missing): the original is never flagged or expunged.
    let (mut rw, task) = open(Script {
        caps: "IMAP4rev1 UIDPLUS",
        refuse: "UID COPY",
        ..Script::default()
    })
    .await;
    rw.select("INBOX").await.unwrap();
    assert_eq!(
        rw.move_to(5, "INBOX.Nope").await,
        Err(RemoteFailure::Refused)
    );
    let commands = finish(rw, task).await;
    assert_eq!(
        commands,
        vec!["SELECT \"INBOX\"", "UID COPY 5 \"INBOX.Nope\""]
    );
}

#[tokio::test]
async fn a_dropped_connection_is_reported_as_lost_not_as_a_refusal() {
    for drop_on in ["UID STORE", "UID MOVE", "SELECT"] {
        let (mut rw, _task) = open(Script {
            drop_on,
            ..Script::default()
        })
        .await;
        let result = match drop_on {
            "SELECT" => rw.select("INBOX").await.map(|_| ()),
            "UID STORE" => {
                rw.select("INBOX").await.unwrap();
                rw.store_flags(5, true, &[MessageFlag::Seen]).await
            }
            _ => {
                rw.select("INBOX").await.unwrap();
                rw.move_to(5, "INBOX.Trash").await.map(|_| ())
            }
        };
        assert_eq!(result, Err(RemoteFailure::ConnectionLost), "{drop_on}");
    }
}

#[tokio::test]
async fn folder_names_that_could_inject_commands_are_never_sent() {
    let (mut rw, task) = open(Script::default()).await;
    assert_eq!(
        rw.select("INBOX\r\na9 DELETE INBOX").await.err(),
        Some(RemoteFailure::Protocol)
    );
    rw.select("INBOX").await.unwrap();
    assert_eq!(
        rw.move_to(5, "Trash\r\nx LOGOUT").await,
        Err(RemoteFailure::Protocol)
    );
    assert_eq!(finish(rw, task).await, vec!["SELECT \"INBOX\""]);
}

#[tokio::test]
async fn flags_are_read_without_touching_the_mailbox_and_incomplete_answers_are_errors() {
    let (rw, task) = open(Script::default()).await;
    let mut session = rw.into_reader();
    let flags = session.fetch_flags(5, 7).await.unwrap();
    assert_eq!(
        flags
            .iter()
            .map(|f| (f.uid, f.seen, f.flagged))
            .collect::<Vec<_>>(),
        vec![(5, true, false), (6, true, true), (7, false, false)]
    );
    // A FETCH line without a UID cannot be attributed: fail instead of guessing, so the caller
    // never treats the missing messages as deleted.
    assert_eq!(
        session.fetch_flags(1, 2).await.err(),
        Some(RemoteFailure::Protocol)
    );
    assert!(
        session.fetch_flags(9, 3).await.unwrap().is_empty(),
        "empty range"
    );
    session.logout().await;
    let seen = task.await.unwrap();
    assert!(seen.iter().any(|c| c == "UID FETCH 5:7 (UID FLAGS)"));
    assert!(
        !seen.iter().any(|c| c.contains("BODY[")),
        "no \\Seen side effect"
    );
}

#[tokio::test]
async fn folders_are_listed_with_attributes_and_decoded_names() {
    let (rw, task) = open(Script::default()).await;
    let mut session = rw.into_reader();
    let folders = session.list_detailed().await.unwrap();
    let got: Vec<_> = folders
        .iter()
        .map(|f| (f.name.as_str(), f.display_name.as_str(), f.selectable))
        .collect();
    assert_eq!(
        got,
        vec![
            ("INBOX", "INBOX", true),
            ("INBOX.Trash", "INBOX.Trash", true),
            ("INBOX.Parent", "INBOX.Parent", false),
            ("INBOX.L&AbA-u tr&Hu8-", "INBOX.Lưu trữ", true),
        ]
    );
    assert!(folders[1].attributes.iter().any(|a| a == "\\Trash"));
    session.logout().await;
    task.await.unwrap();
}
