//! Owner-only end-to-end check of conversations, local search and "load older" against a real
//! IMAP/SMTP provider (MB-200, MB-201).
//!
//! What it does, and nothing else: sends TWO messages from the test mailbox to itself, the
//! second answering the first (`In-Reply-To` / `References`), both with a unique marker in the
//! subject. It syncs, checks they form one conversation of two, finds it by a search word (with
//! and without Vietnamese diacritics), runs one read-only "load older" step on the inbox, then
//! moves exactly those two messages to the trash folder and permanently deletes them. Every
//! mutation is preceded by a check that the cached row carries the marker. Prints counts only.
//!
//!   MAILBOX_SMTP_ENV_FILE=/abs/path/.env.smtp MAILBOX_SMTP_SEND_TO=self \
//!   MAILBOX_TEST_IMAP_HOST=<imap host> \
//!     cargo test -p mailbox-store --test real_threads -- --ignored --nocapture
use mailbox_compose::address::Mailbox;
use mailbox_compose::message::{Limits, Message, Transfer, new_message_id};
use mailbox_core::actions::{
    MoveTarget, OpResult, PendingOpStore, queue_expunge, queue_move, replay,
};
use mailbox_core::folders::{FolderRole, role_of, sync_all_folders};
use mailbox_core::history::{backfill_headers, fetch_older};
use mailbox_core::search::{Narrowing, Scope, SearchQuery, SearchStore};
use mailbox_core::secrets::Secret;
use mailbox_core::store::{AccountRecord, MailStore, MessageRecord};
use mailbox_core::threading::{ThreadStore, rethread_account};
use mailbox_core::{MailServerEndpoint, TlsMode};
use mailbox_imap::Session;
use mailbox_imap::mutate::MutableSession;
use mailbox_smtp::SendOutcome;
use mailbox_smtp::submit::{PartialPolicy, Submission, submit};
use mailbox_store::SqliteStore;
use std::time::Duration;

const OWNER: &str = "local";
const ACC: &str = "acc";

/// Value from the environment, else from the git-ignored env file. Values are never printed.
fn env(key: &str) -> String {
    if let Ok(v) = std::env::var(key) {
        return v;
    }
    let path = std::env::var("MAILBOX_SMTP_ENV_FILE").unwrap_or_else(|_| "../../.env.smtp".into());
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{key} is not set and the env file is not readable"));
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
        .unwrap_or_else(|| panic!("{key} is missing from the env file"))
}

fn bare_address(from: &str) -> String {
    match (from.find('<'), from.rfind('>')) {
        (Some(a), Some(b)) if b > a => from[a + 1..b].trim().to_string(),
        _ => from.trim().to_string(),
    }
}

/// Every cached row whose subject carries the marker.
async fn marked(store: &SqliteStore, marker: &str) -> Vec<MessageRecord> {
    let mut found = Vec::new();
    for folder in store.list_folders(OWNER, ACC).await.unwrap() {
        for m in store.folder_messages(OWNER, ACC, &folder.id).await.unwrap() {
            if m.subject.contains(marker) {
                found.push(m);
            }
        }
    }
    found
}

async fn assert_is_ours(store: &SqliteStore, message_id: &str, marker: &str) {
    let row = store.get_message(OWNER, message_id).await.unwrap();
    assert!(
        row.subject.contains(marker),
        "refusing to touch a message this test did not create"
    );
}

struct Ctx<'a> {
    smtp: &'a MailServerEndpoint,
    user: &'a str,
    pass: &'a Secret,
    from: &'a str,
    date: u64,
}

/// Submit one message to the mailbox itself. Exactly one submission, never repeated.
async fn send_one(ctx: &Ctx<'_>, subject: String, id: String, reply_to: Option<String>) {
    let message = Message {
        from: Mailbox {
            name: "MailBox tự kiểm".into(),
            address: ctx.from.to_string(),
        },
        to: vec![Mailbox {
            name: String::new(),
            address: ctx.user.to_string(),
        }],
        cc: vec![],
        bcc: vec![],
        subject,
        text: "Thư do bộ kiểm thử MailBox (MB-201) tự gửi cho chính hộp thư thử; sẽ bị xóa ngay.\n"
            .into(),
        html: None,
        message_id: id,
        in_reply_to: reply_to.clone(),
        references: reply_to.into_iter().collect(),
        date_epoch: ctx.date,
        attachments: vec![],
    };
    let built = message
        .build(Transfer::EightBit, &Limits::default())
        .unwrap();
    let fallback = message
        .build(Transfer::SevenBit, &Limits::default())
        .unwrap();
    assert_eq!(
        built.recipients,
        vec![ctx.user.to_string()],
        "only the mailbox itself"
    );
    let report = submit(
        ctx.smtp,
        ctx.user,
        ctx.pass,
        &Submission {
            from: built.envelope_from.clone(),
            recipients: built.recipients.clone(),
            data: built.bytes.clone(),
            fallback_7bit: Some(fallback.bytes),
            smtputf8: built.needs_smtputf8,
        },
        PartialPolicy::AbortIfAnyRejected,
    )
    .await
    .expect("SMTP session over verified TLS");
    assert_eq!(report.outcome, SendOutcome::Accepted);
}

#[tokio::test]
#[ignore = "owner only: real mailbox; sends two messages to the test mailbox itself and removes them"]
async fn real_reply_forms_one_conversation_that_search_finds() {
    assert_eq!(
        env("MAILBOX_SMTP_SEND_TO").trim(),
        "self",
        "this test only ever sends to the test mailbox itself"
    );
    let user = env("SMTP_USER_1").trim().to_string();
    let pass = Secret::new(env("SMTP_PASS_1"));
    let from = bare_address(&env("SMTP_FROM_1"));
    let smtp_port: u16 = env("SMTP_PORT_1").trim().parse().expect("SMTP_PORT_1");
    let smtp = MailServerEndpoint {
        hostname: env("SMTP_HOST_1").trim().to_string(),
        port: smtp_port,
        tls_mode: if smtp_port == 465 {
            TlsMode::ImplicitTls
        } else {
            TlsMode::StartTls
        },
    };
    let imap = MailServerEndpoint {
        hostname: std::env::var("MAILBOX_TEST_IMAP_HOST")
            .unwrap_or_else(|_| smtp.hostname.clone())
            .trim()
            .to_string(),
        port: 993,
        tls_mode: TlsMode::ImplicitTls,
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let token = format!("mb201x{:x}{:x}", now.as_secs(), now.subsec_nanos());
    let first_id = new_message_id(&format!("{token}-a"), &from).unwrap();
    let second_id = new_message_id(&format!("{token}-b"), &from).unwrap();
    let ctx = Ctx {
        smtp: &smtp,
        user: &user,
        pass: &pass,
        from: &from,
        date: now.as_secs(),
    };
    send_one(
        &ctx,
        format!("Kiểm tra luồng hội thoại {token}"),
        first_id.clone(),
        None,
    )
    .await;
    send_one(
        &ctx,
        format!("Re: Kiểm tra luồng hội thoại {token}"),
        second_id.clone(),
        Some(first_id.clone()),
    )
    .await;
    println!("sent 2 marked messages (the second answers the first) to the test mailbox itself");

    let store = SqliteStore::in_memory().await.unwrap();
    store
        .upsert_account(&AccountRecord {
            id: ACC.into(),
            owner_id: OWNER.into(),
            display_name: "Test".into(),
            email_address: user.clone(),
            imap_host: imap.hostname.clone(),
            imap_port: 993,
            smtp_host: smtp.hostname.clone(),
            smtp_port,
            auth_type: "password".into(),
            credential_ref: "imap.acc".into(),
            status: "active".into(),
        })
        .await
        .unwrap();

    let mut arrived = Vec::new();
    for attempt in 1..=18 {
        let mut session = Session::connect(&imap, &user, &pass)
            .await
            .expect("IMAP login over verified TLS");
        let report = sync_all_folders(&store, &mut session, OWNER, ACC, 50, 64)
            .await
            .unwrap();
        assert_eq!(report.failed(), 0);
        arrived = marked(&store, &token).await;
        if arrived.len() >= 2 {
            // One read-only "load older" step and a header backfill on the inbox, on the real
            // server (UID FETCH ranges with BODY.PEEK).
            let folders = store.list_folders(OWNER, ACC).await.unwrap();
            let inbox = folders
                .iter()
                .find(|f| role_of(f) == Some(FolderRole::Inbox))
                .unwrap()
                .clone();
            let older = fetch_older(&store, &mut session, OWNER, &inbox, 10)
                .await
                .expect("load older on the real inbox");
            let backfilled = backfill_headers(&store, &mut session, OWNER, &inbox, 50)
                .await
                .unwrap();
            println!(
                "both arrived after {attempt} sync(s); load older stored {} (start reached: {}); backfilled {backfilled}",
                older.stored, older.reached_start
            );
            assert!(older.stored <= 80, "bounded");
            session.logout().await;
            break;
        }
        session.logout().await;
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    assert_eq!(
        arrived.len(),
        2,
        "both self-sent messages arrived, once each"
    );

    // MB-201: one conversation of two.
    rethread_account(&store, OWNER, ACC, std::slice::from_ref(&user))
        .await
        .unwrap();
    let threads: std::collections::HashMap<String, String> = store
        .thread_ids(OWNER, ACC)
        .await
        .unwrap()
        .into_iter()
        .collect();
    let t0 = threads.get(&arrived[0].id).cloned().expect("threaded");
    assert_eq!(threads.get(&arrived[1].id), Some(&t0), "same conversation");
    let sizes: std::collections::HashMap<String, usize> = store
        .thread_sizes(OWNER, ACC)
        .await
        .unwrap()
        .into_iter()
        .collect();
    assert_eq!(sizes.get(&t0), Some(&2), "a conversation of two");
    let convo = store.conversation(OWNER, ACC, &t0).await.unwrap();
    assert_eq!(convo.len(), 2);
    assert_eq!(
        convo[0].message_id_header.as_deref(),
        Some(first_id.as_str()),
        "oldest first"
    );
    let reply_headers = store
        .headers_of(OWNER, &convo[1].id)
        .await
        .unwrap()
        .expect("reply headers stored");
    assert_eq!(
        reply_headers.in_reply_to.as_deref(),
        Some(first_id.as_str())
    );
    println!("conversation of 2 confirmed from Message-ID / In-Reply-To / References");

    // MB-200: found by search, with and without diacritics; hits carry the thread id.
    store.refresh_search_index(OWNER, 10_000).await.unwrap();
    for text in [
        token.clone(),
        format!("kiem tra luong {token}"),
        format!("KIỂM TRA LUỒNG {token}"),
    ] {
        let page = store
            .search(
                OWNER,
                &SearchQuery {
                    text,
                    scope: Scope::Account(ACC.into()),
                    narrowing: Narrowing::default(),
                    limit: 50,
                },
            )
            .await
            .unwrap();
        assert_eq!(page.total, 2);
        assert!(
            page.hits
                .iter()
                .all(|h| h.thread_id.as_deref() == Some(t0.as_str()))
        );
    }
    println!(
        "search found the conversation ({:?}), with and without diacritics",
        store.search_engine()
    );

    // Clean up exactly those two messages: move to the trash folder, then delete them there.
    let mut rw = MutableSession::connect(&imap, &user, &pass)
        .await
        .expect("read-write session");
    let trash = store
        .list_folders(OWNER, ACC)
        .await
        .unwrap()
        .into_iter()
        .find(|f| role_of(f) == Some(FolderRole::Trash))
        .expect("this account has a trash folder");
    for m in &arrived {
        if m.folder_id != trash.id {
            assert_is_ours(&store, &m.id, &token).await;
            queue_move(&store, OWNER, &m.id, MoveTarget::Role(FolderRole::Trash))
                .await
                .unwrap();
        }
    }
    let moved = replay(&store, &mut rw, OWNER, ACC).await.unwrap();
    assert!(
        moved
            .done
            .iter()
            .all(|d| matches!(d.result, OpResult::Moved { .. }))
    );
    let mut reader = rw.into_reader();
    mailbox_core::sync::sync_folder(&store, &mut reader, OWNER, &trash, 50)
        .await
        .unwrap();
    let mut rw = MutableSession::upgrade(reader).await.unwrap();
    let in_trash: Vec<MessageRecord> = marked(&store, &token)
        .await
        .into_iter()
        .filter(|m| m.folder_id == trash.id)
        .collect();
    assert_eq!(
        in_trash.len(),
        2,
        "both marked messages are in the trash folder"
    );
    for m in &in_trash {
        assert_is_ours(&store, &m.id, &token).await;
        queue_expunge(&store, OWNER, &m.id).await.unwrap();
    }
    let cleaned = replay(&store, &mut rw, OWNER, ACC).await.unwrap();
    println!(
        "cleanup: {:?}",
        cleaned.done.iter().map(|d| &d.result).collect::<Vec<_>>()
    );
    assert!(
        cleaned
            .done
            .iter()
            .all(|d| matches!(d.result, OpResult::Expunged | OpResult::LeftDeleted))
    );
    rw.into_reader().logout().await;
    assert!(marked(&store, &token).await.is_empty());
}
