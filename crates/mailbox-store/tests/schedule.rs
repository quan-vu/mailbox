//! Contract of `ScheduleStore` (MB-218) for SQLite (always) and PostgreSQL (opt-in, like
//! `tests/outbox.rs`): a scheduled message is frozen, survives a restart, is never sent twice and
//! is only cancelled while it is still waiting.
use mailbox_core::MailboxError;
use mailbox_core::outbox::{
    DraftRecord, DraftStore, FailReason, MISSED_GRACE_SECS, OutboxState, ScheduleStore, SendIntent,
    recover, schedule, take_due, unschedule,
};
use mailbox_core::store::{AccountRecord, MailStore};
use mailbox_store::{PgStore, SqliteStore};

fn account(id: &str, owner: &str) -> AccountRecord {
    AccountRecord {
        id: id.into(),
        owner_id: owner.into(),
        display_name: "Work".into(),
        email_address: "minh@company.vn".into(),
        imap_host: "imap.company.vn".into(),
        imap_port: 993,
        smtp_host: "smtp.company.vn".into(),
        smtp_port: 465,
        auth_type: "password".into(),
        credential_ref: format!("keychain:{id}"),
        status: "active".into(),
    }
}

fn draft(id: &str, owner: &str, account: &str) -> DraftRecord {
    DraftRecord {
        id: id.into(),
        owner_id: owner.into(),
        account_id: account.into(),
        message_id: format!("<{id}@company.vn>"),
        from_identity: "minh@company.vn".into(),
        to: vec!["an@company.vn".into()],
        subject: "Hẹn giờ".into(),
        ..DraftRecord::default()
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

const CHANGED: MailboxError = MailboxError::InconsistentData("outbox state changed");

struct Counting(u32);

impl mailbox_core::outbox::Transport for Counting {
    async fn deliver(&mut self, _: &DraftRecord) -> mailbox_core::outbox::Delivery {
        self.0 += 1;
        mailbox_core::outbox::Delivery::Accepted
    }
}

async fn contract<S: MailStore + DraftStore + ScheduleStore>(store: &S, p: &str) {
    let (me, other) = (format!("{p}-me"), format!("{p}-other"));
    let acc = format!("{p}-acc");
    store.upsert_account(&account(&acc, &me)).await.unwrap();
    let id = format!("{p}-s1");
    store.save_draft(&draft(&id, &me, &acc)).await.unwrap();
    let t = now();

    schedule(store, &me, &id, t + 3600, t).await.unwrap();
    let got = store.get_draft(&me, &id).await.unwrap();
    assert_eq!(
        (got.state, got.scheduled_at),
        (OutboxState::Scheduled, Some(t + 3600))
    );
    // Content is frozen while scheduled.
    let mut edit = got.clone();
    edit.subject = "đổi".into();
    assert!(store.save_draft(&edit).await.is_err());
    // Another owner sees nothing.
    assert_eq!(
        store.schedule_draft(&other, &id, t + 99).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        store.unschedule_draft(&other, &id).await,
        Err(MailboxError::NotFound)
    );
    // Reschedule, cancel, schedule again.
    schedule(store, &me, &id, t + 7200, t).await.unwrap();
    unschedule(store, &me, &id).await.unwrap();
    let back = store.get_draft(&me, &id).await.unwrap();
    assert_eq!((back.state, back.scheduled_at), (OutboxState::Draft, None));
    assert_eq!(store.unschedule_draft(&me, &id).await, Err(CHANGED));

    // Due: sent once, then no longer scheduled; cancel after the start is refused.
    store.schedule_draft(&me, &id, t - 5).await.unwrap();
    let (due, _) = take_due(store, &me, t).await.unwrap();
    assert_eq!(due.len(), 1);
    let mut transport = Counting(0);
    let state =
        mailbox_core::outbox::submit(store, &mut transport, &me, &id, SendIntent::Scheduled)
            .await
            .unwrap();
    assert_eq!(state, OutboxState::Sent);
    assert!(
        mailbox_core::outbox::submit(store, &mut transport, &me, &id, SendIntent::Scheduled)
            .await
            .is_err()
    );
    assert_eq!(transport.0, 1);
    assert_eq!(store.unschedule_draft(&me, &id).await, Err(CHANGED));
    assert_eq!(store.schedule_draft(&me, &id, t + 600).await, Err(CHANGED));
    let sent = store.get_draft(&me, &id).await.unwrap();
    assert_eq!(
        sent.scheduled_at,
        Some(t - 5),
        "the time stays for the record"
    );

    // Missed: past the grace period it waits for the user, nothing is sent.
    let late = format!("{p}-late");
    store.save_draft(&draft(&late, &me, &acc)).await.unwrap();
    store
        .schedule_draft(&me, &late, t - MISSED_GRACE_SECS - 1)
        .await
        .unwrap();
    assert!(take_due(store, &me, t).await.unwrap().0.is_empty());
    assert_eq!(
        store.get_draft(&me, &late).await.unwrap().state,
        OutboxState::Failed {
            reason: FailReason::MissedSchedule,
            retryable: true
        }
    );
    // A failed message taken back to editing loses its old time.
    store
        .set_state(
            &me,
            &late,
            OutboxState::Failed {
                reason: FailReason::MissedSchedule,
                retryable: true,
            },
            OutboxState::Draft,
        )
        .await
        .unwrap();
    assert_eq!(
        store.get_draft(&me, &late).await.unwrap().scheduled_at,
        None
    );
}

#[tokio::test]
async fn sqlite_meets_the_schedule_contract() {
    contract(&SqliteStore::in_memory().await.unwrap(), "t").await;
}

#[tokio::test]
async fn a_scheduled_message_survives_a_restart_and_is_not_touched_by_recovery() {
    let dir = std::env::temp_dir().join(format!("mailbox-schedule-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("schedule.sqlite3");
    let _ = std::fs::remove_file(&path);
    let t = now();
    {
        let store = SqliteStore::open(&path).await.unwrap();
        store.upsert_account(&account("acc", "me")).await.unwrap();
        store
            .save_draft(&draft("later", "me", "acc"))
            .await
            .unwrap();
        schedule(&store, "me", "later", t + 86_400, t)
            .await
            .unwrap();
    }
    let store = SqliteStore::open(&path).await.unwrap();
    assert_eq!(recover(&store, "me").await, Ok(0));
    let got = store.get_draft("me", "later").await.unwrap();
    assert_eq!(
        (got.state, got.scheduled_at),
        (OutboxState::Scheduled, Some(t + 86_400))
    );
    let (due, next) = take_due(&store, "me", t).await.unwrap();
    assert!(due.is_empty());
    assert_eq!(next, Some(t + 86_400));
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

/// See `tests/contract.rs` for how to start the throwaway PostgreSQL.
#[tokio::test]
#[ignore = "needs MAILBOX_TEST_PG_URL"]
async fn postgres_meets_the_schedule_contract() {
    let url =
        std::env::var("MAILBOX_TEST_PG_URL").expect("set MAILBOX_TEST_PG_URL to run this test");
    let store = PgStore::connect(&url).await.unwrap();
    contract(&store, &format!("s{}", now())).await;
}

/// Track G: a forwarded attachment is stored as a reference to the original message's part.
#[tokio::test]
async fn forwarded_attachment_references_survive_the_store() {
    use mailbox_core::outbox::{AttachmentRef, ForwardedPart};
    let store = SqliteStore::in_memory().await.unwrap();
    store.upsert_account(&account("acc", "me")).await.unwrap();
    let mut d = draft("fwd", "me", "acc");
    d.attachments = vec![
        AttachmentRef {
            path: "/tmp/local.pdf".into(),
            name: "local.pdf".into(),
            size_bytes: 10,
            forwarded: None,
        },
        AttachmentRef {
            path: String::new(),
            name: "hợp đồng.pdf".into(),
            size_bytes: 2048,
            forwarded: Some(ForwardedPart {
                message_id: "acc/INBOX#777#5".into(),
                part: 1,
            }),
        },
    ];
    store.save_draft(&d).await.unwrap();
    let got = store.get_draft("me", "fwd").await.unwrap();
    assert_eq!(got.attachments, d.attachments);
}
