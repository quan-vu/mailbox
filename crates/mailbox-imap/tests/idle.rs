//! IMAP IDLE (RFC 2177) against a scripted in-memory server, on a paused tokio clock so the
//! 25-minute re-issue timer runs instantly and its exact length can be asserted (MB-140).
use mailbox_core::live::IDLE_REISSUE;
use mailbox_core::secrets::Secret;
use mailbox_imap::idle::{IdleEnd, IdleEvent};
use mailbox_imap::{ConnectError, Session};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tokio::time::Instant;

#[derive(Clone, Default)]
struct Plan {
    /// Post-login capabilities.
    caps: &'static str,
    /// Lines pushed while idling, each after the given delay.
    push: Vec<(u64, &'static str)>,
    /// Untagged lines sent right before the tagged reply to DONE.
    on_done: &'static str,
    /// Answer IDLE with a tagged BAD.
    refuse_idle: bool,
    /// Close the connection (no reply) once everything in `push` was sent.
    close_after_push: bool,
}

type Log = Arc<Mutex<Vec<String>>>;

async fn read_literal_login(
    r: &mut BufReader<tokio::io::ReadHalf<DuplexStream>>,
    w: &mut tokio::io::WriteHalf<DuplexStream>,
    first: &str,
) {
    let mut line = first.to_string();
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
}

async fn server(io: DuplexStream, plan: Plan, log: Log) {
    let (r, mut w) = tokio::io::split(io);
    let mut r = BufReader::new(r);
    let mut logged_in = false;
    w.write_all(b"* OK ready\r\n").await.unwrap();
    let mut idle_tag = String::new();
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).await.unwrap_or(0) == 0 {
            return;
        }
        let cmd = line.trim_end().to_string();
        if cmd == "DONE" {
            log.lock().unwrap().push("DONE".into());
            let reply = format!("{}{idle_tag} OK IDLE terminated\r\n", plan.on_done);
            w.write_all(reply.as_bytes()).await.unwrap();
            continue;
        }
        let tag = cmd.split(' ').next().unwrap_or("").to_string();
        let body = cmd[tag.len()..].trim().to_string();
        let upper = body.to_ascii_uppercase();
        log.lock()
            .unwrap()
            .push(upper.split(' ').next().unwrap_or("").to_string());
        if upper.starts_with("CAPABILITY") {
            let caps = if logged_in { plan.caps } else { "IMAP4rev1" };
            w.write_all(format!("* CAPABILITY {caps}\r\n{tag} OK\r\n").as_bytes())
                .await
                .unwrap();
        } else if upper.starts_with("LOGIN") {
            read_literal_login(&mut r, &mut w, &line).await;
            logged_in = true;
            w.write_all(format!("{tag} OK in\r\n").as_bytes())
                .await
                .unwrap();
        } else if upper.starts_with("EXAMINE") {
            w.write_all(
                format!("* 4 EXISTS\r\n* OK [UIDVALIDITY 9]\r\n* OK [UIDNEXT 5]\r\n{tag} OK [READ-ONLY] done\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        } else if upper.starts_with("IDLE") {
            if plan.refuse_idle {
                w.write_all(format!("{tag} BAD unknown\r\n").as_bytes())
                    .await
                    .unwrap();
                continue;
            }
            idle_tag = tag.clone();
            w.write_all(b"+ idling\r\n").await.unwrap();
            for (delay, text) in &plan.push {
                tokio::time::sleep(Duration::from_secs(*delay)).await;
                if w.write_all(format!("{text}\r\n").as_bytes()).await.is_err() {
                    return;
                }
            }
            if plan.close_after_push {
                return;
            }
        } else if upper.starts_with("LOGOUT") {
            let _ = w
                .write_all(format!("* BYE\r\n{tag} OK bye\r\n").as_bytes())
                .await;
            return;
        } else {
            w.write_all(format!("{tag} OK\r\n").as_bytes())
                .await
                .unwrap();
        }
    }
}

async fn connect(plan: Plan) -> (Session<DuplexStream>, Log) {
    let (client, server_io) = tokio::io::duplex(64 * 1024);
    let log: Log = Arc::default();
    tokio::spawn(server(server_io, plan, log.clone()));
    let mut session = Session::login_stream(client, "me@example.org", &Secret::new("pw"))
        .await
        .unwrap();
    session.refresh_capabilities().await.unwrap();
    session.examine("INBOX").await.unwrap();
    (session, log)
}

fn commands(log: &Log) -> Vec<String> {
    log.lock()
        .unwrap()
        .iter()
        .filter(|c| !matches!(c.as_str(), "CAPABILITY" | "LOGIN"))
        .cloned()
        .collect()
}

const CAPS: &str = "IMAP4rev1 IDLE MOVE UIDPLUS";

async fn never() {
    std::future::pending::<()>().await
}

#[tokio::test(start_paused = true)]
async fn new_mail_ends_idle_with_done_and_reports_the_event() {
    let (mut s, log) = connect(Plan {
        caps: CAPS,
        push: vec![(7, "* 0 RECENT"), (3, "* 5 EXISTS")],
        ..Plan::default()
    })
    .await;
    assert!(s.supports_idle());
    let started = Instant::now();
    let out = s.idle(IDLE_REISSUE, never()).await.unwrap();
    assert_eq!(
        started.elapsed(),
        Duration::from_secs(10),
        "RECENT alone is not a change"
    );
    assert_eq!(out.end, IdleEnd::Changed);
    assert!(out.events.contains(&IdleEvent::Exists(5)));
    assert_eq!(commands(&log), vec!["EXAMINE", "IDLE", "DONE"]);
}

#[tokio::test(start_paused = true)]
async fn expunge_and_flag_changes_are_changes_and_done_replies_are_collected() {
    let (mut s, _) = connect(Plan {
        caps: CAPS,
        push: vec![(1, "* 3 EXPUNGE")],
        on_done: "* 2 FETCH (FLAGS (\\Seen))\r\n",
        ..Plan::default()
    })
    .await;
    let out = s.idle(IDLE_REISSUE, never()).await.unwrap();
    assert_eq!(out.end, IdleEnd::Changed);
    assert_eq!(
        out.events,
        vec![IdleEvent::Expunge(3), IdleEvent::FlagsChanged]
    );
}

#[tokio::test(start_paused = true)]
async fn idle_is_reissued_on_the_timer_and_done_precedes_the_next_command() {
    let (mut s, log) = connect(Plan {
        caps: CAPS,
        ..Plan::default()
    })
    .await;
    let started = Instant::now();
    let first = s.idle(IDLE_REISSUE, never()).await.unwrap();
    assert_eq!(first.end, IdleEnd::TimedOut);
    assert_eq!(started.elapsed(), IDLE_REISSUE);
    assert!(IDLE_REISSUE < Duration::from_secs(29 * 60));
    let second = s.idle(IDLE_REISSUE, never()).await.unwrap();
    assert_eq!(second.end, IdleEnd::TimedOut);
    assert_eq!(started.elapsed(), IDLE_REISSUE * 2);
    s.examine("INBOX").await.unwrap();
    assert_eq!(
        commands(&log),
        vec!["EXAMINE", "IDLE", "DONE", "IDLE", "DONE", "EXAMINE"]
    );
}

#[tokio::test(start_paused = true)]
async fn an_interrupt_ends_idle_cleanly_and_returns_its_value() {
    let (mut s, log) = connect(Plan {
        caps: CAPS,
        ..Plan::default()
    })
    .await;
    let out = s
        .idle(IDLE_REISSUE, async {
            tokio::time::sleep(Duration::from_secs(3)).await;
            "borrow"
        })
        .await
        .unwrap();
    assert_eq!(out.end, IdleEnd::Interrupted("borrow"));
    assert_eq!(commands(&log), vec!["EXAMINE", "IDLE", "DONE"]);
}

#[tokio::test(start_paused = true)]
async fn bye_or_a_closed_connection_while_idling_is_connection_loss() {
    let (mut s, _) = connect(Plan {
        caps: CAPS,
        push: vec![(60, "* BYE server shutting down")],
        ..Plan::default()
    })
    .await;
    assert_eq!(
        s.idle(IDLE_REISSUE, never()).await.unwrap_err(),
        ConnectError::Unreachable
    );

    let (mut s, _) = connect(Plan {
        caps: CAPS,
        push: vec![(60, "* 0 RECENT")],
        close_after_push: true,
        ..Plan::default()
    })
    .await;
    assert_eq!(
        s.idle(IDLE_REISSUE, never()).await.unwrap_err(),
        ConnectError::Unreachable
    );
}

#[tokio::test(start_paused = true)]
async fn a_server_without_idle_is_detected_and_a_refused_idle_is_not_an_error() {
    let (s, _) = connect(Plan {
        caps: "IMAP4rev1 UIDPLUS",
        ..Plan::default()
    })
    .await;
    assert!(!s.supports_idle());

    let (mut s, log) = connect(Plan {
        caps: CAPS,
        refuse_idle: true,
        ..Plan::default()
    })
    .await;
    let out = s.idle(IDLE_REISSUE, never()).await.unwrap();
    assert_eq!(out.end, IdleEnd::Refused);
    assert_eq!(commands(&log), vec!["EXAMINE", "IDLE"], "no DONE without +");
}
