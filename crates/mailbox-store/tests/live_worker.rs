//! Live sync worker (MB-140) over the real IMAP session code and the real SQLite store, against
//! a scripted server that keeps its mailbox between connections.
//!
//! Timers run on the real clock with millisecond settings: a paused tokio clock cannot be used
//! here because SQLite work runs on its own thread, which tokio counts as idle time and skips
//! (pool timeouts fire). The production 25-minute re-issue is asserted on a paused clock in
//! `mailbox-imap/tests/idle.rs`; here the worker is shown to use whatever its config says.
use mailbox_core::MailboxError;
use mailbox_core::actions::{MessageFlag, MoveTarget, PendingOpStore, queue_flag, queue_move};
use mailbox_core::folders::FolderRole;
use mailbox_core::live::{AccountLive, Attention, LinkState, LiveMode};
use mailbox_core::secrets::Secret;
use mailbox_core::store::{AccountRecord, MailStore};
use mailbox_imap::Session;
use mailbox_imap::live::{Command, Connector, LiveConfig, LiveHandle, worker};
use mailbox_store::SqliteStore;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tokio::sync::{Notify, watch};
use tokio::time::Instant;

const OWNER: &str = "local";
const ACC: &str = "acc";

#[derive(Debug, Clone)]
struct Msg {
    uid: u32,
    seen: bool,
    flagged: bool,
}

#[derive(Debug)]
struct Folder {
    name: &'static str,
    attr: &'static str,
    uid_validity: u64,
    uid_next: u32,
    msgs: Vec<Msg>,
}

#[derive(Debug)]
struct Remote {
    caps: &'static str,
    password_ok: bool,
    folders: Vec<Folder>,
    /// Commands received (without tag; LOGIN arguments are not recorded).
    log: Vec<String>,
    /// When each connection attempt started.
    connects: Vec<Instant>,
    /// Connection attempts to fail before the next one succeeds.
    refuse_connects: usize,
    /// Perform the first command starting with this, then close without answering (one-shot).
    vanish_after: Option<&'static str>,
    /// Lines to push to a client that is idling.
    push: Vec<String>,
    /// Close the connection at the next poke.
    kill: bool,
    /// Messages stored by APPEND: (folder, Message-ID, UID).
    appended: Vec<(String, String, u32)>,
}

type Shared = Arc<(Mutex<Remote>, Notify)>;

fn remote(caps: &'static str) -> Shared {
    let msg = |uid| Msg {
        uid,
        seen: false,
        flagged: false,
    };
    Arc::new((
        Mutex::new(Remote {
            caps,
            password_ok: true,
            folders: vec![
                Folder {
                    name: "INBOX",
                    attr: "\\HasNoChildren",
                    uid_validity: 777,
                    uid_next: 7,
                    msgs: vec![msg(5), msg(6)],
                },
                Folder {
                    name: "Trash",
                    attr: "\\HasNoChildren \\Trash",
                    uid_validity: 900,
                    uid_next: 40,
                    msgs: vec![],
                },
            ],
            log: Vec::new(),
            connects: Vec::new(),
            refuse_connects: 0,
            vanish_after: None,
            push: Vec::new(),
            kill: false,
            appended: Vec::new(),
        }),
        Notify::new(),
    ))
}

fn with<T>(r: &Shared, f: impl FnOnce(&mut Remote) -> T) -> T {
    f(&mut r.0.lock().unwrap())
}

/// New mail in INBOX, announced to an idling client.
fn deliver(r: &Shared) -> u32 {
    let uid = with(r, |s| {
        let inbox = &mut s.folders[0];
        let uid = inbox.uid_next;
        inbox.uid_next += 1;
        inbox.msgs.push(Msg {
            uid,
            seen: false,
            flagged: false,
        });
        let n = inbox.msgs.len();
        s.push.push(format!("* {n} EXISTS"));
        uid
    });
    r.1.notify_one();
    uid
}

fn envelope(seq: usize, m: &Msg) -> String {
    let headers = format!("From: a@example.org\r\nSubject: S{}\r\n\r\n", m.uid);
    let mut flags = Vec::new();
    if m.seen {
        flags.push("\\Seen");
    }
    if m.flagged {
        flags.push("\\Flagged");
    }
    format!(
        "* {seq} FETCH (UID {} FLAGS ({}) INTERNALDATE \"14-Nov-2026 10:05:00 +0700\" RFC822.SIZE 100 BODY[HEADER.FIELDS (FROM SUBJECT DATE MESSAGE-ID)] {{{}}}\r\n{headers})\r\n",
        m.uid,
        flags.join(" "),
        headers.len()
    )
}

fn range(spec: &str) -> (u32, u32) {
    match spec.split_once(':') {
        Some((a, "*")) => (a.parse().unwrap(), u32::MAX),
        Some((a, b)) => (a.parse().unwrap(), b.parse().unwrap()),
        None => {
            let n = spec.parse().unwrap();
            (n, n)
        }
    }
}

fn unquote(s: &str) -> &str {
    s.trim_matches('"')
}

/// Answer one command against the shared mailbox. `selected` is this connection's folder.
fn answer(s: &mut Remote, tag: &str, body: &str, selected: &mut usize, logged_in: bool) -> String {
    let upper = body.to_ascii_uppercase();
    let words: Vec<&str> = body.split(' ').collect();
    if upper.starts_with("CAPABILITY") {
        let caps = if logged_in { s.caps } else { "IMAP4rev1" };
        return format!("* CAPABILITY {caps}\r\n{tag} OK\r\n");
    }
    if upper.starts_with("LIST") {
        let mut out = String::new();
        for f in &s.folders {
            out.push_str(&format!("* LIST ({}) \"/\" {}\r\n", f.attr, f.name));
        }
        return out + &format!("{tag} OK\r\n");
    }
    if upper.starts_with("EXAMINE") || upper.starts_with("SELECT") {
        let name = unquote(words[1]);
        let Some(i) = s.folders.iter().position(|f| f.name == name) else {
            return format!("{tag} NO no such folder\r\n");
        };
        *selected = i;
        let f = &s.folders[i];
        let mode = if upper.starts_with("SELECT") {
            "READ-WRITE"
        } else {
            "READ-ONLY"
        };
        return format!(
            "* {} EXISTS\r\n* OK [UIDVALIDITY {}] ok\r\n* OK [UIDNEXT {}] ok\r\n{tag} OK [{mode}] done\r\n",
            f.msgs.len(),
            f.uid_validity,
            f.uid_next
        );
    }
    if upper.starts_with("UID SEARCH HEADER MESSAGE-ID") {
        let wanted = body.rsplit('"').nth(1).unwrap_or("").to_string();
        let name = s.folders[*selected].name;
        let uids: Vec<String> = s
            .appended
            .iter()
            .filter(|(f, mid, _)| f == name && *mid == wanted)
            .map(|(_, _, uid)| uid.to_string())
            .collect();
        return format!("* SEARCH {}\r\n{tag} OK\r\n", uids.join(" "))
            .replace("SEARCH \r", "SEARCH\r");
    }
    let folder = &mut s.folders[*selected];
    if upper.starts_with("FETCH") {
        let (a, b) = range(words[1]);
        let mut out = String::new();
        for (i, m) in folder.msgs.iter().enumerate() {
            let seq = i as u32 + 1;
            if seq >= a && seq <= b {
                out.push_str(&envelope(i + 1, m));
            }
        }
        return out + &format!("{tag} OK\r\n");
    }
    if upper.starts_with("UID FETCH") {
        let (a, b) = range(words[2]);
        let mut out = String::new();
        for (i, m) in folder.msgs.iter().enumerate() {
            if m.uid < a || m.uid > b {
                continue;
            }
            if upper.contains("INTERNALDATE") {
                out.push_str(&envelope(i + 1, m));
            } else if upper.ends_with("(UID FLAGS)") {
                let mut flags = Vec::new();
                if m.seen {
                    flags.push("\\Seen");
                }
                if m.flagged {
                    flags.push("\\Flagged");
                }
                out.push_str(&format!(
                    "* {} FETCH (UID {} FLAGS ({}))\r\n",
                    i + 1,
                    m.uid,
                    flags.join(" ")
                ));
            } else {
                out.push_str(&format!("* {} FETCH (UID {})\r\n", i + 1, m.uid));
            }
        }
        return out + &format!("{tag} OK\r\n");
    }
    if upper.starts_with("UID STORE") {
        let uid: u32 = words[2].parse().unwrap();
        let on = words[3].starts_with('+');
        if let Some(m) = folder.msgs.iter_mut().find(|m| m.uid == uid) {
            if upper.contains("\\FLAGGED") {
                m.flagged = on;
            }
            if upper.contains("\\SEEN") {
                m.seen = on;
            }
        }
        return format!("{tag} OK\r\n");
    }
    if upper.starts_with("UID MOVE") {
        let uid: u32 = words[2].parse().unwrap();
        let Some(pos) = folder.msgs.iter().position(|m| m.uid == uid) else {
            return format!("{tag} OK\r\n");
        };
        let mut m = folder.msgs.remove(pos);
        let dest = unquote(words[3]).to_string();
        let d = s.folders.iter_mut().find(|f| f.name == dest).unwrap();
        m.uid = d.uid_next;
        d.uid_next += 1;
        let (v, new) = (d.uid_validity, m.uid);
        d.msgs.push(m);
        return format!("* OK [COPYUID {v} {uid} {new}] moved\r\n{tag} OK\r\n");
    }
    format!("{tag} OK\r\n")
}

async fn serve(io: DuplexStream, r: Shared) {
    let (rd, mut w) = tokio::io::split(io);
    let mut rd = BufReader::new(rd);
    let mut selected = 0;
    let mut logged_in = false;
    w.write_all(b"* OK ready\r\n").await.unwrap();
    loop {
        let mut line = String::new();
        if rd.read_line(&mut line).await.unwrap_or(0) == 0 {
            return;
        }
        let cmd = line.trim_end().to_string();
        let tag = cmd.split(' ').next().unwrap_or("").to_string();
        let body = cmd[tag.len()..].trim().to_string();
        let upper = body.to_ascii_uppercase();
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
                rd.read_exact(&mut buf).await.unwrap();
                line.clear();
                rd.read_line(&mut line).await.unwrap();
            }
            let ok = with(&r, |s| {
                s.log.push("LOGIN".into());
                s.password_ok
            });
            let reply = if ok {
                logged_in = true;
                format!("{tag} OK in\r\n")
            } else {
                format!("{tag} NO [AUTHENTICATIONFAILED] invalid\r\n")
            };
            w.write_all(reply.as_bytes()).await.unwrap();
            continue;
        }
        if upper.starts_with("IDLE") {
            with(&r, |s| s.log.push("IDLE".into()));
            w.write_all(b"+ idling\r\n").await.unwrap();
            loop {
                let mut done = String::new();
                tokio::select! {
                    n = rd.read_line(&mut done) => {
                        if n.unwrap_or(0) == 0 {
                            return;
                        }
                        with(&r, |s| s.log.push(done.trim_end().to_string()));
                        w.write_all(format!("{tag} OK idle done\r\n").as_bytes()).await.unwrap();
                        break;
                    }
                    () = r.1.notified() => {
                        let (lines, kill) = with(&r, |s| (std::mem::take(&mut s.push), s.kill));
                        if kill {
                            with(&r, |s| s.kill = false);
                            return;
                        }
                        for l in lines {
                            w.write_all(format!("{l}\r\n").as_bytes()).await.unwrap();
                        }
                    }
                }
            }
            continue;
        }
        if upper.starts_with("APPEND") {
            // `APPEND "folder" (flags) {n}`: take the literal, keep its Message-ID.
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
            rd.read_exact(&mut buf).await.unwrap();
            let mut end = String::new();
            rd.read_line(&mut end).await.unwrap();
            let text = String::from_utf8_lossy(&buf).to_string();
            let mid = text
                .lines()
                .find_map(|l| l.strip_prefix("Message-ID: "))
                .unwrap_or("")
                .trim()
                .to_string();
            let folder = unquote(body.split(' ').nth(1).unwrap()).to_string();
            let (vanish, reply) = with(&r, |s| {
                s.log.push(format!("APPEND {folder}"));
                let f = s.folders.iter_mut().find(|f| f.name == folder).unwrap();
                let (v, uid) = (f.uid_validity, f.uid_next);
                f.uid_next += 1;
                s.appended.push((folder.clone(), mid, uid));
                let vanish = s.vanish_after == Some("APPEND");
                if vanish {
                    s.vanish_after = None;
                }
                (vanish, format!("{tag} OK [APPENDUID {v} {uid}] done\r\n"))
            });
            if vanish {
                return;
            }
            w.write_all(reply.as_bytes()).await.unwrap();
            continue;
        }
        let (reply, vanish) = with(&r, |s| {
            s.log.push(body.clone());
            let vanish = s
                .vanish_after
                .is_some_and(|p| upper.starts_with(&p.to_ascii_uppercase()));
            if vanish {
                s.vanish_after = None;
            }
            (answer(s, &tag, &body, &mut selected, logged_in), vanish)
        });
        if vanish {
            return;
        }
        if upper.starts_with("LOGOUT") {
            let _ = w
                .write_all(format!("* BYE\r\n{tag} OK bye\r\n").as_bytes())
                .await;
            return;
        }
        w.write_all(reply.as_bytes()).await.unwrap();
    }
}

struct TestConnector(Shared);

impl Connector for TestConnector {
    type Stream = DuplexStream;
    fn connect(
        &mut self,
    ) -> impl Future<Output = Result<Session<DuplexStream>, MailboxError>> + Send {
        let r = self.0.clone();
        async move {
            let refused = with(&r, |s| {
                s.connects.push(Instant::now());
                if s.refuse_connects > 0 {
                    s.refuse_connects -= 1;
                    true
                } else {
                    false
                }
            });
            if refused {
                return Err(MailboxError::TransientConnectionFailure);
            }
            let (client, srv) = tokio::io::duplex(1 << 16);
            tokio::spawn(serve(srv, r.clone()));
            Session::login_stream(client, "user@example.org", &Secret::new("pw"))
                .await
                .map_err(Into::into)
        }
    }
}

async fn store() -> SqliteStore {
    let store = SqliteStore::in_memory().await.unwrap();
    store
        .upsert_account(&AccountRecord {
            id: ACC.into(),
            owner_id: OWNER.into(),
            display_name: "Test".into(),
            email_address: "user@example.org".into(),
            imap_host: "imap.example.org".into(),
            imap_port: 993,
            smtp_host: "smtp.example.org".into(),
            smtp_port: 465,
            auth_type: "password".into(),
            credential_ref: "imap.acc".into(),
            status: "active".into(),
        })
        .await
        .unwrap();
    store
}

const MS: Duration = Duration::from_millis(1);
/// Scheduling slack allowed on top of a timer.
const SLACK: Duration = Duration::from_millis(150);

fn config() -> LiveConfig {
    LiveConfig {
        idle_reissue: MS * 400,
        poll: MS * 300,
        other_folders_every: Duration::from_secs(3600),
        backoff_base: MS * 100,
        backoff_cap: Duration::from_secs(5),
        jitter: || 0.0,
        now: || 1_000,
        ..LiveConfig::default()
    }
}

async fn settle() {
    tokio::time::sleep(MS * 5).await;
}

fn start(store: &SqliteStore, r: &Shared, config: LiveConfig) -> LiveHandle<DuplexStream> {
    let (handle, run) = worker(
        store.clone(),
        OWNER.into(),
        ACC.into(),
        TestConnector(r.clone()),
        config,
    );
    tokio::spawn(run);
    handle
}

async fn until(
    rx: &mut watch::Receiver<AccountLive>,
    f: impl Fn(&AccountLive) -> bool,
) -> AccountLive {
    tokio::time::timeout(Duration::from_secs(10), rx.wait_for(|s| f(s)))
        .await
        .expect("state reached")
        .expect("worker alive")
        .clone()
}

/// Wait until the worker has ended (its status sender is gone).
async fn ended(rx: &mut watch::Receiver<AccountLive>) {
    let gone = tokio::time::timeout(Duration::from_secs(10), rx.wait_for(|_| false)).await;
    assert!(matches!(gone, Ok(Err(_))), "worker ended");
}

async fn inbox_uids(store: &SqliteStore) -> Vec<(u64, u32)> {
    let mut uids: Vec<(u64, u32)> = store
        .folder_messages(OWNER, ACC, "acc/INBOX")
        .await
        .unwrap()
        .into_iter()
        .map(|m| (m.uid_validity, m.imap_uid))
        .collect();
    uids.sort();
    uids
}

fn log(r: &Shared) -> Vec<String> {
    with(r, |s| s.log.clone())
}

fn count(r: &Shared, prefix: &str) -> usize {
    log(r).iter().filter(|l| l.starts_with(prefix)).count()
}

const IDLE_CAPS: &str = "IMAP4rev1 IDLE MOVE UIDPLUS";

#[tokio::test]
async fn connects_syncs_idles_and_picks_up_new_mail_without_a_refresh() {
    let (store, r) = (store().await, remote(IDLE_CAPS));
    let h = start(&store, &r, config());
    let mut rx = h.status();
    let online = until(&mut rx, |s| s.state == LinkState::Online(LiveMode::Idle)).await;
    assert_eq!(online.last_synced, Some(1_000));
    assert_eq!(inbox_uids(&store).await, vec![(777, 5), (777, 6)]);
    // Wait until the worker is really idling, then the server announces new mail.
    while count(&r, "IDLE") == 0 {
        settle().await;
    }
    let generation = online.generation;
    let uid = deliver(&r);
    until(&mut rx, |s| {
        s.generation > generation && s.state == LinkState::Online(LiveMode::Idle)
    })
    .await;
    assert_eq!(
        inbox_uids(&store).await,
        vec![(777, 5), (777, 6), (777, uid)]
    );
    let l = log(&r);
    let idle = l.iter().position(|c| c == "IDLE").unwrap();
    assert_eq!(
        l[idle + 1],
        "DONE",
        "DONE is the first thing sent after IDLE"
    );
    assert!(l[idle + 2..].iter().any(|c| c.starts_with("UID FETCH 7:*")));
    assert_eq!(count(&r, "LOGIN"), 1, "one connection for everything");
    h.send(Command::Stop);
    ended(&mut rx).await;
}

#[tokio::test]
async fn idle_is_reissued_on_the_configured_timer() {
    let (store, r) = (store().await, remote(IDLE_CAPS));
    let h = start(&store, &r, config());
    let mut rx = h.status();
    until(&mut rx, |s| s.state == LinkState::Online(LiveMode::Idle)).await;
    while count(&r, "IDLE") == 0 {
        settle().await;
    }
    let t0 = Instant::now();
    while count(&r, "IDLE") < 2 {
        settle().await;
    }
    let waited = t0.elapsed();
    assert!(
        waited >= MS * 400 - SLACK && waited < MS * 400 + SLACK,
        "re-issued after {waited:?}"
    );
    assert_eq!(count(&r, "DONE"), 1);
    assert_eq!(count(&r, "LOGIN"), 1);
}

#[tokio::test]
async fn a_server_without_idle_is_polled_on_the_interval() {
    let (store, r) = (store().await, remote("IMAP4rev1 MOVE UIDPLUS"));
    let h = start(&store, &r, config());
    let mut rx = h.status();
    let online = until(&mut rx, |s| s.state == LinkState::Online(LiveMode::Polling)).await;
    let t0 = Instant::now();
    let uid = deliver(&r);
    until(&mut rx, |s| s.generation > online.generation).await;
    let waited = t0.elapsed();
    assert!(
        waited < MS * 300 + SLACK,
        "picked up by the poll after {waited:?}"
    );
    assert!(inbox_uids(&store).await.contains(&(777, uid)));
    assert_eq!(count(&r, "IDLE"), 0);
}

#[tokio::test]
async fn a_drop_reconnects_with_growing_delays_and_replays_queued_ops_once() {
    let (store, r) = (store().await, remote(IDLE_CAPS));
    let h = start(&store, &r, config());
    let mut rx = h.status();
    until(&mut rx, |s| s.state == LinkState::Online(LiveMode::Idle)).await;
    while count(&r, "IDLE") == 0 {
        settle().await;
    }
    // A change made while connected but not yet sent, then the network goes away.
    queue_flag(&store, OWNER, "acc/INBOX#777#5", MessageFlag::Flagged, true)
        .await
        .unwrap();
    with(&r, |s| {
        s.kill = true;
        s.refuse_connects = 2;
    });
    r.1.notify_one();
    let offline = until(&mut rx, |s| matches!(s.state, LinkState::Offline { .. })).await;
    assert_eq!(offline.state, LinkState::Offline { retry_at: 1_000 });
    until(&mut rx, |s| s.state == LinkState::Online(LiveMode::Idle)).await;
    let connects = with(&r, |s| s.connects.clone());
    assert_eq!(connects.len(), 4, "first, two refused, one accepted");
    let gaps: Vec<Duration> = connects.windows(2).skip(1).map(|w| w[1] - w[0]).collect();
    // Jitter 0 gives the lower bound, half of base * 2^n: 50, 100, 200 ms.
    for (gap, want) in gaps.iter().zip([MS * 100, MS * 200]) {
        assert!(*gap >= want && *gap < want + SLACK, "gaps {gaps:?}");
    }
    assert_eq!(
        count(&r, "UID STORE 5 +FLAGS (\\Flagged)"),
        1,
        "replayed exactly once"
    );
    assert!(store.list_ops(OWNER, ACC).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_move_cut_off_mid_command_is_not_sent_again_after_reconnect() {
    let (store, r) = (store().await, remote(IDLE_CAPS));
    // First a normal connect so the folders and messages are known.
    let h = start(&store, &r, config());
    let mut rx = h.status();
    until(&mut rx, |s| s.state == LinkState::Online(LiveMode::Idle)).await;
    h.send(Command::Stop);
    ended(&mut rx).await;

    queue_move(
        &store,
        OWNER,
        "acc/INBOX#777#6",
        MoveTarget::Role(FolderRole::Trash),
    )
    .await
    .unwrap();
    // The server performs the move, then the connection dies before the answer.
    with(&r, |s| s.vanish_after = Some("UID MOVE"));
    let h = start(&store, &r, config());
    let mut rx = h.status();
    until(&mut rx, |s| matches!(s.state, LinkState::Offline { .. })).await;
    until(&mut rx, |s| s.state == LinkState::Online(LiveMode::Idle)).await;
    assert_eq!(count(&r, "UID MOVE"), 1, "a move is never sent twice");
    assert!(store.list_ops(OWNER, ACC).await.unwrap().is_empty());
    assert_eq!(inbox_uids(&store).await, vec![(777, 5)]);
}

#[tokio::test]
async fn auth_failure_waits_for_the_user_without_retrying() {
    let (store, r) = (store().await, remote(IDLE_CAPS));
    with(&r, |s| s.password_ok = false);
    let h = start(&store, &r, config());
    let mut rx = h.status();
    until(&mut rx, |s| {
        s.state == LinkState::NeedsAttention(Attention::AuthFailed)
    })
    .await;
    // With a 100 ms base, a retrying worker would have tried about ten times by now.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(with(&r, |s| s.connects.len()), 1, "no retry storm");
    // The user re-entered the password and pressed "retry now".
    with(&r, |s| s.password_ok = true);
    assert!(h.send(Command::RetryNow));
    until(&mut rx, |s| s.state == LinkState::Online(LiveMode::Idle)).await;
    assert_eq!(with(&r, |s| s.connects.len()), 2);
}

#[tokio::test]
async fn a_uidvalidity_change_mid_session_rebuilds_the_folder() {
    let (store, r) = (store().await, remote(IDLE_CAPS));
    let h = start(&store, &r, config());
    let mut rx = h.status();
    let online = until(&mut rx, |s| s.state == LinkState::Online(LiveMode::Idle)).await;
    while count(&r, "IDLE") == 0 {
        settle().await;
    }
    with(&r, |s| {
        let inbox = &mut s.folders[0];
        inbox.uid_validity = 778;
        inbox.msgs = vec![Msg {
            uid: 1,
            seen: true,
            flagged: false,
        }];
        inbox.uid_next = 2;
        s.push.push("* 1 EXISTS".into());
    });
    r.1.notify_one();
    until(&mut rx, |s| {
        s.generation > online.generation && s.state == LinkState::Online(LiveMode::Idle)
    })
    .await;
    assert_eq!(
        inbox_uids(&store).await,
        vec![(778, 1)],
        "old epoch discarded"
    );
}

#[tokio::test]
async fn a_borrower_gets_the_connection_after_done_and_a_pending_flag_survives_sync() {
    let (store, r) = (store().await, remote(IDLE_CAPS));
    let h = start(&store, &r, config());
    let mut rx = h.status();
    let online = until(&mut rx, |s| s.state == LinkState::Online(LiveMode::Idle)).await;
    while count(&r, "IDLE") == 0 {
        settle().await;
    }
    {
        let mut session = h.session().await.expect("connected");
        session.reader_mut().examine("Trash").await.unwrap();
    }
    let l = log(&r);
    let idle = l.iter().position(|c| c == "IDLE").unwrap();
    assert_eq!(&l[idle + 1..idle + 3], ["DONE", "EXAMINE \"Trash\""]);

    // Starred here, not yet sent; meanwhile the server reports other flag changes.
    queue_flag(&store, OWNER, "acc/INBOX#777#5", MessageFlag::Flagged, true)
        .await
        .unwrap();
    while count(&r, "IDLE") < 2 {
        settle().await;
    }
    with(&r, |s| {
        s.folders[0].msgs[1].seen = true;
        s.push.push("* 2 FETCH (FLAGS (\\Seen))".into());
    });
    r.1.notify_one();
    until(&mut rx, |s| s.generation > online.generation).await;
    let m5 = store.get_message(OWNER, "acc/INBOX#777#5").await.unwrap();
    let m6 = store.get_message(OWNER, "acc/INBOX#777#6").await.unwrap();
    assert!(
        m5.flagged,
        "the older server snapshot does not undo a pending change"
    );
    assert!(m6.seen, "server changes to other messages arrive");
    assert_eq!(count(&r, "LOGIN"), 1);
    h.send(Command::Stop);
    ended(&mut rx).await;
    assert_eq!(count(&r, "LOGOUT"), 1);
}

#[tokio::test]
async fn other_folders_follow_on_the_slower_rotation_while_idling() {
    let (store, r) = (store().await, remote(IDLE_CAPS));
    let h = start(
        &store,
        &r,
        LiveConfig {
            idle_reissue: Duration::from_secs(60),
            other_folders_every: MS * 300,
            ..config()
        },
    );
    let mut rx = h.status();
    let online = until(&mut rx, |s| s.state == LinkState::Online(LiveMode::Idle)).await;
    // Mail filed into another folder by the server: no IDLE event for INBOX.
    with(&r, |s| {
        let trash = &mut s.folders[1];
        trash.msgs.push(Msg {
            uid: 40,
            seen: true,
            flagged: false,
        });
        trash.uid_next = 41;
    });
    let t0 = Instant::now();
    until(&mut rx, |s| s.generation > online.generation).await;
    let waited = t0.elapsed();
    assert!(
        waited < MS * 300 + SLACK,
        "rotation after {waited:?}, not the 60 s re-issue"
    );
    assert!(store.get_message(OWNER, "acc/Trash#900#40").await.is_ok());
    assert_eq!(count(&r, "LOGIN"), 1);
}

// ---- Track G: Sent copy through the held session (MB-151 on MB-140) ----

struct TrackGCopyJob {
    folder: &'static str,
    message_id: String,
    bytes: Vec<u8>,
}

impl mailbox_imap::borrow::RepeatableJob for TrackGCopyJob {
    type Output = mailbox_imap::append::SentCopy;
    fn run<'a, S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'a>(
        &'a self,
        session: &'a mut Session<S>,
    ) -> impl Future<Output = Result<Self::Output, mailbox_imap::append::AppendError>> + Send + 'a
    {
        mailbox_imap::append::save_sent_copy(session, self.folder, &self.message_id, &self.bytes)
    }
}

fn track_g_job(id: &str) -> TrackGCopyJob {
    let message_id = format!("<{id}@example.org>");
    TrackGCopyJob {
        folder: "Trash",
        bytes: format!("Message-ID: {message_id}\r\nSubject: t\r\n\r\nbody\r\n").into_bytes(),
        message_id,
    }
}

async fn track_g_fresh(
    r: Shared,
) -> Result<Session<DuplexStream>, mailbox_imap::append::AppendError> {
    let (client, srv) = tokio::io::duplex(1 << 16);
    tokio::spawn(serve(srv, r));
    Session::login_stream(client, "user@example.org", &Secret::new("pw"))
        .await
        .map_err(mailbox_imap::append::AppendError::Connection)
}

fn track_g_copies(r: &Shared, message_id: &str) -> usize {
    with(r, |s| {
        s.appended
            .iter()
            .filter(|(_, m, _)| m == message_id)
            .count()
    })
}

#[tokio::test]
async fn track_g_sent_copy_uses_the_held_session_and_falls_back_without_duplicates() {
    use mailbox_imap::append::SentCopy;
    use mailbox_imap::borrow::{Route, live_or_fresh};
    let (store, r) = (store().await, remote(IDLE_CAPS));
    let h = start(&store, &r, config());
    let mut rx = h.status();
    until(&mut rx, |s| s.state == LinkState::Online(LiveMode::Idle)).await;
    while count(&r, "IDLE") == 0 {
        settle().await;
    }

    // With a connected worker: no new login, one copy; a second run finds it and adds none.
    let job = track_g_job("live");
    let (result, route) = live_or_fresh(Some(&h), &job, || track_g_fresh(r.clone())).await;
    assert!(
        matches!(result, Ok(SentCopy::Appended { uid: Some(40) })),
        "{result:?}"
    );
    assert_eq!(route, Route::Live);
    let (again, _) = live_or_fresh(Some(&h), &job, || track_g_fresh(r.clone())).await;
    assert_eq!(again, Ok(SentCopy::AlreadyThere));
    assert_eq!(count(&r, "LOGIN"), 1, "the held session did the work");
    assert_eq!(track_g_copies(&r, &job.message_id), 1);

    // No worker: a connection of its own, logged out afterwards.
    let alone = track_g_job("alone");
    let (result, route) = live_or_fresh(None::<&LiveHandle<DuplexStream>>, &alone, || {
        track_g_fresh(r.clone())
    })
    .await;
    assert!(matches!(result, Ok(SentCopy::Appended { .. })));
    assert_eq!(route, Route::Fresh);
    assert_eq!(count(&r, "LOGIN"), 2);
    assert_eq!(count(&r, "LOGOUT"), 1);

    // The held connection dies right after the server stored the copy: the job runs again on a
    // fresh connection, finds the copy by Message-ID and does not make a second one.
    while count(&r, "IDLE") < 2 {
        settle().await;
    }
    with(&r, |s| s.vanish_after = Some("APPEND"));
    let cut = track_g_job("cut");
    let (result, route) = live_or_fresh(Some(&h), &cut, || track_g_fresh(r.clone())).await;
    assert_eq!(result, Ok(SentCopy::AlreadyThere));
    assert_eq!(route, Route::LiveThenFresh);
    assert_eq!(track_g_copies(&r, &cut.message_id), 1, "no duplicate copy");
    h.send(Command::Stop);
    ended(&mut rx).await;
}
