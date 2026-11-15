//! Replay of pending operations over the real IMAP session code and the real SQLite store,
//! against a scripted server that keeps its mailbox state between connections (MB-132, MB-144).
use mailbox_core::actions::{
    MessageFlag, MoveTarget, OpResult, OpState, PendingOpStore, queue_flag, queue_move, replay,
};
use mailbox_core::secrets::Secret;
use mailbox_core::store::{AccountRecord, FolderRecord, MailStore, MessageRecord};
use mailbox_imap::Session;
use mailbox_imap::mutate::MutableSession;
use mailbox_store::SqliteStore;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};

#[derive(Debug)]
struct Remote {
    uid_validity: u64,
    inbox: Vec<u32>,
    trash: Vec<u32>,
    flagged: Vec<u32>,
    /// Perform this command, then close the connection without answering.
    vanish_after: &'static str,
    /// Answer NO to commands starting with this.
    refuse: &'static str,
    log: Vec<String>,
}

type Shared = Arc<Mutex<Remote>>;

fn remote() -> Shared {
    Arc::new(Mutex::new(Remote {
        uid_validity: 777,
        inbox: vec![5, 6],
        trash: vec![],
        flagged: vec![],
        vanish_after: "-",
        refuse: "-",
        log: Vec::new(),
    }))
}

async fn serve(io: DuplexStream, state: Shared) {
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
        let upper = body.to_ascii_uppercase();
        let uid: u32 = body
            .split(' ')
            .nth(2)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if upper.starts_with("LOGIN") {
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
            w.write_all(format!("{tag} OK\r\n").as_bytes())
                .await
                .unwrap();
            continue;
        }
        let (reply, vanish) = {
            let mut s = state.lock().unwrap();
            if upper.starts_with("UID") || upper.starts_with("SELECT") {
                s.log.push(body.clone());
            }
            let vanish = upper.starts_with(s.vanish_after);
            let reply = if upper.starts_with(s.refuse) {
                format!("{tag} NO refused\r\n")
            } else if upper.starts_with("CAPABILITY") {
                format!("* CAPABILITY IMAP4rev1 MOVE UIDPLUS\r\n{tag} OK\r\n")
            } else if upper.starts_with("SELECT") {
                format!(
                    "* {} EXISTS\r\n* OK [UIDVALIDITY {}] ok\r\n{tag} OK [READ-WRITE] done\r\n",
                    s.inbox.len(),
                    s.uid_validity
                )
            } else if upper.starts_with("UID FETCH") {
                if s.inbox.contains(&uid) {
                    format!("* 1 FETCH (UID {uid})\r\n{tag} OK\r\n")
                } else {
                    format!("{tag} OK\r\n")
                }
            } else if upper.starts_with("UID STORE") {
                if upper.contains("+FLAGS") {
                    s.flagged.push(uid);
                } else {
                    s.flagged.retain(|u| *u != uid);
                }
                format!("{tag} OK\r\n")
            } else if upper.starts_with("UID MOVE") {
                if s.inbox.contains(&uid) {
                    s.inbox.retain(|u| *u != uid);
                    let new = 40 + s.trash.len() as u32;
                    s.trash.push(new);
                    format!("* OK [COPYUID 900 {uid} {new}] moved\r\n{tag} OK\r\n")
                } else {
                    format!("{tag} OK\r\n")
                }
            } else if upper.starts_with("LOGOUT") {
                format!("* BYE\r\n{tag} OK\r\n")
            } else {
                format!("{tag} BAD unknown\r\n")
            };
            (reply, vanish)
        };
        if vanish {
            break;
        }
        w.write_all(reply.as_bytes()).await.unwrap();
        if upper.starts_with("LOGOUT") {
            break;
        }
    }
}

/// One connection: log in, replay, log out.
async fn session_replay(
    store: &SqliteStore,
    state: &Shared,
) -> mailbox_core::actions::ReplayReport {
    let (client, srv) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(serve(srv, state.clone()));
    let session = Session::login_stream(client, "user@example.com", &Secret::new("pw"))
        .await
        .unwrap();
    let mut rw = MutableSession::upgrade(session).await.unwrap();
    let report = replay(store, &mut rw, "local", "acc").await.unwrap();
    rw.into_reader().logout().await;
    task.await.unwrap();
    report
}

async fn store() -> SqliteStore {
    let store = SqliteStore::in_memory().await.unwrap();
    store
        .upsert_account(&AccountRecord {
            id: "acc".into(),
            owner_id: "local".into(),
            display_name: "Test".into(),
            email_address: "t@example.com".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 465,
            auth_type: "password".into(),
            credential_ref: "imap.acc".into(),
            status: "active".into(),
        })
        .await
        .unwrap();
    for (name, role) in [("INBOX", "inbox"), ("Trash", "trash")] {
        store
            .upsert_folder(
                "local",
                &FolderRecord {
                    id: format!("acc/{name}"),
                    account_id: "acc".into(),
                    remote_name: name.into(),
                    delimiter: Some("/".into()),
                    special_use: Some(role.into()),
                    uid_validity: Some(777),
                    uid_next: None,
                },
            )
            .await
            .unwrap();
    }
    let rows: Vec<MessageRecord> = [5u32, 6]
        .into_iter()
        .map(|uid| MessageRecord {
            id: format!("acc/INBOX#777#{uid}"),
            account_id: "acc".into(),
            folder_id: "acc/INBOX".into(),
            uid_validity: 777,
            imap_uid: uid,
            message_id_header: None,
            subject: format!("S{uid}"),
            from_display: String::new(),
            from_address: "a@example.com".into(),
            preview: String::new(),
            internal_date: i64::from(uid),
            size_bytes: 1,
            seen: false,
            flagged: false,
        })
        .collect();
    store.upsert_messages("local", &rows).await.unwrap();
    store
}

fn log(state: &Shared) -> Vec<String> {
    state.lock().unwrap().log.clone()
}

#[tokio::test]
async fn queued_flag_and_move_reach_the_server_once_in_order() {
    let (store, state) = (store().await, remote());
    queue_flag(
        &store,
        "local",
        "acc/INBOX#777#5",
        MessageFlag::Flagged,
        true,
    )
    .await
    .unwrap();
    queue_move(
        &store,
        "local",
        "acc/INBOX#777#6",
        MoveTarget::Role(mailbox_core::folders::FolderRole::Trash),
    )
    .await
    .unwrap();
    let report = session_replay(&store, &state).await;
    assert_eq!(report.done.len(), 2);
    assert_eq!(report.done[0].result, OpResult::Confirmed);
    assert_eq!(
        report.done[1].result,
        OpResult::Moved {
            new_message_id: Some("acc/Trash#900#40".into()),
            source_left_deleted: false
        }
    );
    assert_eq!(
        log(&state),
        vec![
            "SELECT \"INBOX\"",
            "UID STORE 5 +FLAGS (\\Flagged)",
            "UID FETCH 6 (UID)",
            "UID MOVE 6 \"Trash\"",
        ],
        "one SELECT for both operations of the folder"
    );
    // A second connection has nothing to do.
    let again = session_replay(&store, &state).await;
    assert!(again.done.is_empty());
    assert_eq!(log(&state).len(), 4, "no duplicate replay");
    assert!(store.get_message("local", "acc/Trash#900#40").await.is_ok());
}

#[tokio::test]
async fn uidvalidity_changed_between_queue_and_replay_drops_the_operation() {
    let (store, state) = (store().await, remote());
    queue_flag(&store, "local", "acc/INBOX#777#5", MessageFlag::Seen, true)
        .await
        .unwrap();
    state.lock().unwrap().uid_validity = 778;
    let report = session_replay(&store, &state).await;
    assert_eq!(report.done[0].result, OpResult::DroppedEpochChanged);
    assert_eq!(
        log(&state),
        vec!["SELECT \"INBOX\""],
        "no UID command was sent"
    );
    assert!(store.list_ops("local", "acc").await.unwrap().is_empty());
    assert!(
        store
            .folder_messages("local", "acc", "acc/INBOX")
            .await
            .unwrap()
            .is_empty(),
        "the stale epoch is discarded for a rebuild"
    );
    assert!(
        store
            .load_cursor("local", "acc", "acc/INBOX")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn connection_lost_before_the_answer_keeps_the_flag_queued_and_it_is_sent_again() {
    let (store, state) = (store().await, remote());
    queue_flag(
        &store,
        "local",
        "acc/INBOX#777#5",
        MessageFlag::Flagged,
        true,
    )
    .await
    .unwrap();
    state.lock().unwrap().vanish_after = "UID STORE";
    let report = session_replay(&store, &state).await;
    assert!(report.interrupted);
    assert_eq!((report.done.len(), report.still_queued), (0, 1));
    let ops = store.list_ops("local", "acc").await.unwrap();
    assert_eq!(ops[0].state, OpState::Queued);
    assert!(
        store
            .get_message("local", "acc/INBOX#777#5")
            .await
            .unwrap()
            .flagged
    );

    // Storing a flag twice is harmless, so the retry simply repeats it.
    state.lock().unwrap().vanish_after = "-";
    let report = session_replay(&store, &state).await;
    assert_eq!(report.done[0].result, OpResult::Confirmed);
    assert!(store.list_ops("local", "acc").await.unwrap().is_empty());
}

#[tokio::test]
async fn a_move_that_reached_the_server_before_the_drop_is_not_sent_twice() {
    let (store, state) = (store().await, remote());
    queue_move(
        &store,
        "local",
        "acc/INBOX#777#5",
        MoveTarget::Role(mailbox_core::folders::FolderRole::Trash),
    )
    .await
    .unwrap();
    state.lock().unwrap().vanish_after = "UID MOVE";
    let report = session_replay(&store, &state).await;
    assert!(report.interrupted);
    assert_eq!(store.list_ops("local", "acc").await.unwrap().len(), 1);

    state.lock().unwrap().vanish_after = "-";
    let report = session_replay(&store, &state).await;
    assert_eq!(report.done[0].result, OpResult::Gone);
    let moves = log(&state)
        .iter()
        .filter(|c| c.starts_with("UID MOVE"))
        .count();
    assert_eq!(moves, 1, "the move is never repeated");
    assert_eq!(state.lock().unwrap().trash.len(), 1);
    assert!(store.list_ops("local", "acc").await.unwrap().is_empty());
}

#[tokio::test]
async fn a_no_answer_rolls_the_flag_back_and_leaves_a_failed_operation() {
    let (store, state) = (store().await, remote());
    queue_flag(&store, "local", "acc/INBOX#777#6", MessageFlag::Seen, true)
        .await
        .unwrap();
    state.lock().unwrap().refuse = "UID STORE";
    let report = session_replay(&store, &state).await;
    assert_eq!(report.done[0].result, OpResult::Rejected("refused".into()));
    assert!(
        !store
            .get_message("local", "acc/INBOX#777#6")
            .await
            .unwrap()
            .seen
    );
    assert_eq!(
        store.list_ops("local", "acc").await.unwrap()[0].state,
        OpState::Failed("refused".into())
    );
}
