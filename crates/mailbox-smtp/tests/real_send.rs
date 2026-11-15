//! Opt-in end-to-end check against the real test provider (MB-150/MB-151): build a marked
//! message with a small attachment, send it to the test mailbox itself, see it arrive, keep
//! exactly one copy in Sent, then remove only the messages this test created.
//!
//! Needs the git-ignored `.env.smtp` (read inside this process; nothing from it is printed):
//!   MAILBOX_SMTP_ENV_FILE=/path/to/.env.smtp MAILBOX_SMTP_SEND_TO=self \
//!     cargo test -p mailbox-smtp --test real_send -- --ignored --nocapture
//! `MAILBOX_SMTP_SEND_TO` must be `self`: this test never writes to anybody else.
//! IMAP host defaults to the SMTP host; override with `MAILBOX_TEST_IMAP_HOST`.
use mailbox_compose::address::Mailbox;
use mailbox_compose::message::{Attachment, Limits, Message, Transfer, new_message_id};
use mailbox_core::secrets::Secret;
use mailbox_core::{MailServerEndpoint, TlsMode};
use mailbox_imap::Session;
use mailbox_imap::append::{FolderRole, SentCopy, find_folder, save_sent_copy};
use mailbox_smtp::SendOutcome;
use mailbox_smtp::submit::{PartialPolicy, RcptStatus, Submission, submit};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Value from the environment, else from the env file. Values are never printed.
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

#[tokio::test]
#[ignore = "owner only: needs .env.smtp and MAILBOX_SMTP_SEND_TO=self; sends one real email to the test mailbox itself"]
async fn self_send_arrives_and_sent_holds_exactly_one_copy() {
    assert_eq!(
        env("MAILBOX_SMTP_SEND_TO").trim(),
        "self",
        "this test only ever sends to the test mailbox itself"
    );
    let user = env("SMTP_USER_1").trim().to_string();
    let password = Secret::new(env("SMTP_PASS_1"));
    let from = bare_address(&env("SMTP_FROM_1"));
    let smtp_host = env("SMTP_HOST_1").trim().to_string();
    let smtp_port: u16 = env("SMTP_PORT_1").trim().parse().expect("SMTP_PORT_1");
    let imap_host = std::env::var("MAILBOX_TEST_IMAP_HOST").unwrap_or_else(|_| smtp_host.clone());
    let smtp = MailServerEndpoint {
        hostname: smtp_host,
        port: smtp_port,
        tls_mode: if smtp_port == 465 {
            TlsMode::ImplicitTls
        } else {
            TlsMode::StartTls
        },
    };
    let imap = MailServerEndpoint {
        hostname: imap_host,
        port: 993,
        tls_mode: TlsMode::ImplicitTls,
    };

    // A marker nobody else can have produced: it scopes everything this test touches.
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let token = format!("mb151-{:x}-{:x}", now.as_secs(), now.subsec_nanos());
    let message_id = new_message_id(&token, &from).expect("message id");
    let attachment = format!("Tệp thử {token}\n").into_bytes();
    let message = Message {
        from: Mailbox { name: "MailBox tự kiểm".into(), address: from.clone() },
        // `self`: the login mailbox, which is the only recipient.
        to: vec![Mailbox { name: String::new(), address: user.clone() }],
        cc: vec![],
        bcc: vec![],
        subject: format!("MailBox tự kiểm gửi thư — {token}"),
        text: "Đây là thư do bộ kiểm thử MailBox (MB-151) tự gửi cho chính hộp thư thử.\nThư sẽ được xóa ngay sau khi kiểm tra.\n".into(),
        html: None,
        message_id: message_id.clone(),
        in_reply_to: None,
        references: vec![],
        date_epoch: now.as_secs(),
        attachments: vec![Attachment {
            filename: format!("ghi-chú-{token}.txt"),
            content_type: None,
            data: attachment,
        }],
    };
    let built = message
        .build(Transfer::EightBit, &Limits::default())
        .unwrap();
    let fallback = message
        .build(Transfer::SevenBit, &Limits::default())
        .unwrap();
    assert_eq!(
        built.recipients,
        vec![user.clone()],
        "exactly one recipient: the mailbox itself"
    );
    let submission = Submission {
        from: built.envelope_from.clone(),
        recipients: built.recipients.clone(),
        data: built.bytes.clone(),
        fallback_7bit: Some(fallback.bytes.clone()),
        smtputf8: built.needs_smtputf8,
    };

    // One submission, never repeated.
    let report = submit(
        &smtp,
        &user,
        &password,
        &submission,
        PartialPolicy::AbortIfAnyRejected,
    )
    .await
    .expect("SMTP session");
    println!(
        "smtp outcome: {:?}, 8-bit transfer: {}",
        report.outcome, report.used_8bit
    );
    assert_eq!(report.outcome, SendOutcome::Accepted);
    assert!(
        report
            .recipients
            .iter()
            .all(|(_, s)| *s == RcptStatus::Accepted)
    );
    let sent_bytes = if report.used_8bit {
        &built.bytes
    } else {
        &fallback.bytes
    };

    let mut session = Session::connect(&imap, &user, &password)
        .await
        .expect("IMAP login");
    session.refresh_capabilities().await.unwrap();
    let uidplus = session.info.has("UIDPLUS");
    let folders = session.list_folders().await.unwrap();
    let sent = find_folder(&folders, FolderRole::Sent)
        .expect("a Sent folder")
        .name
        .clone();

    // Sent reconciliation: one copy, and a second pass adds nothing.
    let first = save_sent_copy(&mut session, &sent, &message_id, sent_bytes)
        .await
        .unwrap();
    println!(
        "sent copy: {}",
        match first {
            SentCopy::Appended { .. } => "appended by MailBox",
            SentCopy::AlreadyThere => "already saved by the provider",
        }
    );
    let second = save_sent_copy(&mut session, &sent, &message_id, sent_bytes)
        .await
        .unwrap();
    assert_eq!(second, SentCopy::AlreadyThere, "no duplicate in Sent");
    let in_sent = session
        .find_by_message_id(&sent, &message_id)
        .await
        .unwrap();
    assert_eq!(in_sent.len(), 1, "Sent holds exactly one copy");

    // Delivery to the mailbox itself.
    let mut in_inbox = Vec::new();
    for _ in 0..30 {
        in_inbox = session
            .find_by_message_id("INBOX", &message_id)
            .await
            .unwrap();
        if !in_inbox.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    println!("arrived in INBOX: {} message(s)", in_inbox.len());

    // Clean up only what carries this run's Message-ID.
    if uidplus {
        session.delete_uids(&sent, &in_sent).await.unwrap();
        session.delete_uids("INBOX", &in_inbox).await.unwrap();
        assert!(
            session
                .find_by_message_id(&sent, &message_id)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            session
                .find_by_message_id("INBOX", &message_id)
                .await
                .unwrap()
                .is_empty()
        );
        println!(
            "cleanup: removed {} test message(s)",
            in_sent.len() + in_inbox.len()
        );
    } else {
        println!("cleanup skipped: server has no UIDPLUS, test messages were left in place");
    }
    session.logout().await;
    assert_eq!(in_inbox.len(), 1, "the message arrived exactly once");
}
