//! Behavioural contract of `DraftStore` (MB-133, MB-151). The same checks run against SQLite
//! (always) and PostgreSQL (when `MAILBOX_TEST_PG_URL` points at a throwaway database).
use mailbox_core::MailboxError;
use mailbox_core::outbox::{
    AttachmentRef, DraftRecord, DraftStore, FailReason, OutboxState, SentCopyState, recover,
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
        version: 0,
        message_id: format!("<{id}@company.vn>"),
        from_identity: "minh@company.vn".into(),
        to: vec!["Trần Linh <linh@company.vn>".into(), "an@company.vn".into()],
        cc: vec![],
        bcc: vec!["\"Sếp, lớn\" <boss@company.vn>".into()],
        subject: "Kế hoạch \"quý 4\"".into(),
        body_text: "Xin chào,\n\ndòng có 'nháy' và \\ gạch chéo.\n".into(),
        body_html: None,
        in_reply_to: Some("<orig@x.org>".into()),
        references: vec!["<root@x.org>".into(), "<orig@x.org>".into()],
        attachments: vec![AttachmentRef {
            path: "/Users/minh/Tài liệu/báo cáo.pdf".into(),
            name: "báo cáo.pdf".into(),
            size_bytes: 1_250_000,
            forwarded: None,
        }],
        state: OutboxState::Draft,
        sent_copy: SentCopyState::None,
        updated_at: 0,
        scheduled_at: None,
    }
}

const CHANGED: MailboxError = MailboxError::InconsistentData("outbox state changed");

/// `p` makes ids unique per run so the PostgreSQL database can be reused between runs.
async fn contract<S: MailStore + DraftStore>(store: &S, p: &str) {
    let (alice, bob) = (format!("{p}-alice"), format!("{p}-bob"));
    let (acc, bob_acc) = (format!("{p}-acc"), format!("{p}-bobacc"));
    store.upsert_account(&account(&acc, &alice)).await.unwrap();
    store
        .upsert_account(&account(&bob_acc, &bob))
        .await
        .unwrap();
    let id = format!("{p}-d1");

    // Create and read back, including non-ASCII text, quotes and every list.
    let mut d = draft(&id, &alice, &acc);
    assert_eq!(store.save_draft(&d).await.unwrap(), 1);
    let stored = store.get_draft(&alice, &id).await.unwrap();
    assert!(stored.updated_at > 0);
    d.version = 1;
    d.updated_at = stored.updated_at;
    assert_eq!(stored, d);

    // Autosave: the version moves forward; a stale writer is refused and changes nothing.
    d.subject = "Kế hoạch quý 4 (sửa)".into();
    d.body_html = Some("<p>Xin chào</p>".into());
    d.attachments.clear();
    assert_eq!(store.save_draft(&d).await.unwrap(), 2);
    let mut stale = d.clone();
    stale.subject = "ghi đè cũ".into();
    assert_eq!(
        store.save_draft(&stale).await,
        Err(MailboxError::InconsistentData("draft version"))
    );
    let stored = store.get_draft(&alice, &id).await.unwrap();
    assert_eq!(
        (stored.version, stored.subject.as_str()),
        (2, "Kế hoạch quý 4 (sửa)")
    );
    assert_eq!(stored.body_html.as_deref(), Some("<p>Xin chào</p>"));
    assert!(stored.attachments.is_empty());

    // The Message-ID is fixed at creation: a later save cannot change it.
    d.version = 2;
    d.message_id = "<another@company.vn>".into();
    assert_eq!(store.save_draft(&d).await.unwrap(), 3);
    assert_eq!(
        store.get_draft(&alice, &id).await.unwrap().message_id,
        format!("<{id}@company.vn>")
    );
    let mut no_id = draft(&format!("{p}-noid"), &alice, &acc);
    no_id.message_id.clear();
    assert_eq!(
        store.save_draft(&no_id).await,
        Err(MailboxError::InconsistentData("draft message id"))
    );

    // Owner isolation: someone else's draft does not exist, for every operation.
    assert_eq!(
        store.get_draft(&bob, &id).await,
        Err(MailboxError::NotFound)
    );
    assert!(store.list_drafts(&bob).await.unwrap().is_empty());
    assert_eq!(
        store.delete_draft(&bob, &id).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        store
            .set_state(&bob, &id, OutboxState::Draft, OutboxState::Queued)
            .await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        store.set_sent_copy(&bob, &id, SentCopyState::Saved).await,
        Err(MailboxError::NotFound)
    );
    // Bob cannot take over the id, nor file a draft under Alice's account.
    assert_eq!(
        store.save_draft(&draft(&id, &bob, &bob_acc)).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        store
            .save_draft(&draft(&format!("{p}-x"), &bob, &acc))
            .await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(store.get_draft(&alice, &id).await.unwrap().owner_id, alice);

    // State changes are compare-and-set.
    store
        .set_state(&alice, &id, OutboxState::Draft, OutboxState::Queued)
        .await
        .unwrap();
    assert_eq!(
        store
            .set_state(&alice, &id, OutboxState::Draft, OutboxState::Queued)
            .await,
        Err(CHANGED),
        "a second click cannot queue it again"
    );
    // Content is frozen once the user pressed Send.
    d.version = 3;
    assert_eq!(
        store.save_draft(&d).await,
        Err(MailboxError::InconsistentData("draft not editable"))
    );
    // In flight: cannot be deleted from under the sender.
    assert_eq!(
        store.delete_draft(&alice, &id).await,
        Err(MailboxError::InconsistentData("draft in flight"))
    );
    store
        .set_state(&alice, &id, OutboxState::Queued, OutboxState::Sending)
        .await
        .unwrap();
    assert_eq!(
        store.delete_draft(&alice, &id).await,
        Err(MailboxError::InconsistentData("draft in flight"))
    );
    let failed = OutboxState::Failed {
        reason: FailReason::RecipientsRejected,
        retryable: false,
    };
    store
        .set_state(&alice, &id, OutboxState::Sending, failed)
        .await
        .unwrap();
    assert_eq!(store.get_draft(&alice, &id).await.unwrap().state, failed);
    let other_failure = OutboxState::Failed {
        reason: FailReason::Network,
        retryable: true,
    };
    assert_eq!(
        store
            .set_state(&alice, &id, other_failure, OutboxState::Queued)
            .await,
        Err(CHANGED),
        "the expected failure must match, not just the word 'failed'"
    );
    store
        .set_state(&alice, &id, failed, OutboxState::Draft)
        .await
        .unwrap();
    let back = store.get_draft(&alice, &id).await.unwrap();
    assert_eq!((back.state, back.version), (OutboxState::Draft, 3));

    // Sent marks the Sent-folder copy as still to be reconciled.
    for (from, to) in [
        (OutboxState::Draft, OutboxState::Queued),
        (OutboxState::Queued, OutboxState::Sending),
        (OutboxState::Sending, OutboxState::Sent),
    ] {
        store.set_state(&alice, &id, from, to).await.unwrap();
    }
    let sent = store.get_draft(&alice, &id).await.unwrap();
    assert_eq!(
        (sent.state, sent.sent_copy),
        (OutboxState::Sent, SentCopyState::Pending)
    );
    store
        .set_sent_copy(&alice, &id, SentCopyState::AlreadyThere)
        .await
        .unwrap();
    assert_eq!(
        store.get_draft(&alice, &id).await.unwrap().sent_copy,
        SentCopyState::AlreadyThere
    );

    // Start-up recovery on real rows: mid-flight becomes unknown, queued becomes a failure.
    let (flying, waiting) = (format!("{p}-d2"), format!("{p}-d3"));
    for (draft_id, upto) in [(&flying, 2), (&waiting, 1)] {
        store
            .save_draft(&draft(draft_id, &alice, &acc))
            .await
            .unwrap();
        let path = [
            OutboxState::Draft,
            OutboxState::Queued,
            OutboxState::Sending,
        ];
        for i in 0..upto {
            store
                .set_state(&alice, draft_id, path[i], path[i + 1])
                .await
                .unwrap();
        }
    }
    assert_eq!(
        recover(store, &bob).await,
        Ok(0),
        "another owner's outbox is untouched"
    );
    assert_eq!(recover(store, &alice).await, Ok(2));
    assert_eq!(
        store.get_draft(&alice, &flying).await.unwrap().state,
        OutboxState::Unknown
    );
    assert_eq!(
        store.get_draft(&alice, &waiting).await.unwrap().state,
        OutboxState::Failed {
            reason: FailReason::Interrupted,
            retryable: true
        }
    );
    assert_eq!(recover(store, &alice).await, Ok(0));

    // Listing shows all of the owner's rows; delete removes exactly one.
    let listed = store.list_drafts(&alice).await.unwrap();
    assert_eq!(listed.len(), 3);
    assert!(
        listed
            .windows(2)
            .all(|w| w[0].updated_at >= w[1].updated_at),
        "newest first"
    );
    store.delete_draft(&alice, &flying).await.unwrap();
    assert_eq!(
        store.get_draft(&alice, &flying).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        store.delete_draft(&alice, &flying).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(store.list_drafts(&alice).await.unwrap().len(), 2);

    // Removing the account removes its drafts (local data only).
    store.delete_account(&alice, &acc).await.unwrap();
    assert!(store.list_drafts(&alice).await.unwrap().is_empty());
}

#[tokio::test]
async fn sqlite_meets_the_draft_contract() {
    contract(&SqliteStore::in_memory().await.unwrap(), "t").await;
}

#[tokio::test]
async fn drafts_and_an_interrupted_send_survive_a_restart() {
    let dir = std::env::temp_dir().join(format!("mailbox-outbox-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("outbox.sqlite3");
    let _ = std::fs::remove_file(&path);
    {
        let store = SqliteStore::open(&path).await.unwrap();
        store.upsert_account(&account("acc", "me")).await.unwrap();
        store.save_draft(&draft("keep", "me", "acc")).await.unwrap();
        store
            .save_draft(&draft("flying", "me", "acc"))
            .await
            .unwrap();
        store
            .set_state("me", "flying", OutboxState::Draft, OutboxState::Queued)
            .await
            .unwrap();
        store
            .set_state("me", "flying", OutboxState::Queued, OutboxState::Sending)
            .await
            .unwrap();
        // The process stops here, mid-send.
    }
    let store = SqliteStore::open(&path).await.unwrap();
    let kept = store.get_draft("me", "keep").await.unwrap();
    assert_eq!(
        (kept.state, kept.subject.as_str()),
        (OutboxState::Draft, "Kế hoạch \"quý 4\"")
    );
    assert_eq!(kept.attachments.len(), 1);
    assert_eq!(
        store.get_draft("me", "flying").await.unwrap().state,
        OutboxState::Sending
    );
    assert_eq!(recover(&store, "me").await, Ok(1));
    assert_eq!(
        store.get_draft("me", "flying").await.unwrap().state,
        OutboxState::Unknown
    );
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

/// See `tests/contract.rs` for how to start the throwaway PostgreSQL.
#[tokio::test]
#[ignore = "needs MAILBOX_TEST_PG_URL"]
async fn postgres_meets_the_draft_contract() {
    let url =
        std::env::var("MAILBOX_TEST_PG_URL").expect("set MAILBOX_TEST_PG_URL to run this test");
    let store = PgStore::connect(&url).await.unwrap();
    let run = format!(
        "o{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    contract(&store, &run).await;
}
