//! Contract for the folder-role and pending-operation stores (MB-122, MB-132, MB-144).
//! Runs against SQLite always and PostgreSQL when `MAILBOX_TEST_PG_URL` is set.
use mailbox_core::MailboxError;
use mailbox_core::actions::{MessageFlag, NewOp, OpKind, OpState, PendingOpStore};
use mailbox_core::folders::{FolderRole, FolderRoleRecord, FolderRoleStore, RoleSource};
use mailbox_core::store::{AccountRecord, FolderRecord, MailStore, MessageRecord};
use mailbox_store::{PgStore, SqliteStore};

fn account(id: &str, owner: &str) -> AccountRecord {
    AccountRecord {
        id: id.into(),
        owner_id: owner.into(),
        display_name: "Work".into(),
        email_address: "t@example.com".into(),
        imap_host: "imap.example.com".into(),
        imap_port: 993,
        smtp_host: "smtp.example.com".into(),
        smtp_port: 465,
        auth_type: "password".into(),
        credential_ref: format!("keychain:{id}"),
        status: "active".into(),
    }
}

fn folder(id: &str, account: &str, name: &str) -> FolderRecord {
    FolderRecord {
        id: id.into(),
        account_id: account.into(),
        remote_name: name.into(),
        delimiter: Some(".".into()),
        special_use: None,
        uid_validity: Some(10),
        uid_next: None,
    }
}

fn message(id: &str, account: &str, folder: &str, uid: u32) -> MessageRecord {
    MessageRecord {
        id: id.into(),
        account_id: account.into(),
        folder_id: folder.into(),
        uid_validity: 10,
        imap_uid: uid,
        message_id_header: Some(format!("<{id}@example.com>")),
        subject: format!("S{uid}"),
        from_display: "A".into(),
        from_address: "a@example.com".into(),
        preview: String::new(),
        internal_date: i64::from(uid),
        size_bytes: 1,
        seen: false,
        flagged: false,
    }
}

fn op(account: &str, m: &MessageRecord, kind: OpKind, previous: Option<bool>) -> NewOp {
    NewOp {
        account_id: account.into(),
        message_id: m.id.clone(),
        folder_id: m.folder_id.clone(),
        uid_validity: m.uid_validity,
        imap_uid: m.imap_uid,
        kind,
        previous,
    }
}

async fn contract(store: &(impl MailStore + FolderRoleStore + PendingOpStore), p: &str) {
    let (alice, bob) = (format!("{p}-alice"), format!("{p}-bob"));
    let acc = format!("{p}-acc");
    let (inbox, trash) = (format!("{p}-inbox"), format!("{p}-trash"));
    store.upsert_account(&account(&acc, &alice)).await.unwrap();
    store
        .upsert_folder(&alice, &folder(&inbox, &acc, "INBOX"))
        .await
        .unwrap();
    store
        .upsert_folder(&alice, &folder(&trash, &acc, "INBOX.Trash"))
        .await
        .unwrap();

    // Roles are data: stored per folder with where they came from, and can be changed.
    let role = |folder_id: &str, role, source| FolderRoleRecord {
        folder_id: folder_id.into(),
        account_id: acc.clone(),
        role,
        source,
    };
    store
        .save_folder_role(
            &alice,
            &role(&inbox, Some(FolderRole::Inbox), RoleSource::Name),
        )
        .await
        .unwrap();
    store
        .save_folder_role(&alice, &role(&trash, None, RoleSource::Name))
        .await
        .unwrap();
    store
        .save_folder_role(
            &alice,
            &role(&trash, Some(FolderRole::Trash), RoleSource::User),
        )
        .await
        .unwrap();
    let mut roles = store.list_folder_roles(&alice, &acc).await.unwrap();
    roles.sort_by(|a, b| a.folder_id.cmp(&b.folder_id));
    assert_eq!(
        roles,
        vec![
            role(&inbox, Some(FolderRole::Inbox), RoleSource::Name),
            role(&trash, Some(FolderRole::Trash), RoleSource::User),
        ]
    );
    assert_eq!(
        store.list_folder_roles(&bob, &acc).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        store
            .save_folder_role(&bob, &role(&inbox, None, RoleSource::User))
            .await,
        Err(MailboxError::NotFound)
    );

    // Messages: single-row access scoped by owner.
    let (m1, m2) = (
        message(&format!("{p}-m1"), &acc, &inbox, 1),
        message(&format!("{p}-m2"), &acc, &inbox, 2),
    );
    store
        .upsert_messages(&alice, &[m1.clone(), m2.clone()])
        .await
        .unwrap();
    assert_eq!(store.get_message(&alice, &m1.id).await.unwrap(), m1);
    assert_eq!(
        store.get_message(&bob, &m1.id).await,
        Err(MailboxError::NotFound)
    );
    store
        .set_local_flags(&alice, &m1.id, Some(true), None)
        .await
        .unwrap();
    store
        .set_local_flags(&alice, &m1.id, None, Some(true))
        .await
        .unwrap();
    let got = store.get_message(&alice, &m1.id).await.unwrap();
    assert!(got.seen && got.flagged);
    assert_eq!(
        store.set_local_flags(&bob, &m1.id, Some(false), None).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        store
            .folder_messages(&alice, &acc, &inbox)
            .await
            .unwrap()
            .len(),
        2
    );

    // Pending operations: ordered, owner-scoped, editable, removable.
    let flag = OpKind::SetFlag {
        flag: MessageFlag::Flagged,
        on: true,
    };
    let a = store
        .enqueue_op(&alice, &op(&acc, &m1, flag.clone(), Some(false)))
        .await
        .unwrap();
    let b = store
        .enqueue_op(
            &alice,
            &op(
                &acc,
                &m2,
                OpKind::Move {
                    dest_folder_id: trash.clone(),
                },
                None,
            ),
        )
        .await
        .unwrap();
    let c = store
        .enqueue_op(&alice, &op(&acc, &m2, OpKind::Expunge, None))
        .await
        .unwrap();
    assert!(a.seq < b.seq && b.seq < c.seq);
    assert!(a.id != b.id && b.id != c.id);
    assert_eq!(
        (a.kind.clone(), a.previous, a.state.clone()),
        (flag, Some(false), OpState::Queued)
    );
    assert_eq!(
        store.list_ops(&alice, &acc).await.unwrap(),
        vec![a.clone(), b.clone(), c.clone()]
    );
    assert_eq!(
        store.list_ops(&bob, &acc).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(
        store
            .enqueue_op(&bob, &op(&acc, &m1, OpKind::Expunge, None))
            .await,
        Err(MailboxError::NotFound)
    );

    let mut failed = b.clone();
    failed.state = OpState::Failed("refused".into());
    store.update_op(&alice, &failed).await.unwrap();
    assert_eq!(
        store.update_op(&bob, &failed).await,
        Err(MailboxError::NotFound)
    );
    assert_eq!(store.list_ops(&alice, &acc).await.unwrap()[1], failed);

    assert!(!store.remove_op(&bob, &a.id).await.unwrap());
    assert!(store.remove_op(&alice, &a.id).await.unwrap());
    assert!(
        !store.remove_op(&alice, &a.id).await.unwrap(),
        "already gone"
    );

    // Removing a message removes the operations that referred to it.
    assert_eq!(
        store.remove_message(&bob, &m2.id).await,
        Err(MailboxError::NotFound)
    );
    store.remove_message(&alice, &m2.id).await.unwrap();
    assert!(store.list_ops(&alice, &acc).await.unwrap().is_empty());

    // So does a folder reset (UIDVALIDITY change): a stale UID can never be replayed.
    store
        .enqueue_op(&alice, &op(&acc, &m1, OpKind::Expunge, None))
        .await
        .unwrap();
    store.reset_folder(&alice, &acc, &inbox).await.unwrap();
    assert!(store.list_ops(&alice, &acc).await.unwrap().is_empty());

    // Removing a folder removes its role, messages and operations; other owners cannot.
    assert_eq!(
        store.remove_folder(&bob, &acc, &trash).await,
        Err(MailboxError::NotFound)
    );
    store.remove_folder(&alice, &acc, &trash).await.unwrap();
    assert_eq!(
        store
            .list_folders(&alice, &acc)
            .await
            .unwrap()
            .iter()
            .map(|f| f.id.clone())
            .collect::<Vec<_>>(),
        vec![inbox.clone()]
    );
    assert_eq!(
        store.list_folder_roles(&alice, &acc).await.unwrap().len(),
        1
    );
    store.delete_account(&alice, &acc).await.unwrap();
}

#[tokio::test]
async fn sqlite_meets_the_actions_contract() {
    let store = SqliteStore::in_memory().await.unwrap();
    contract(&store, "t").await;
}

#[tokio::test]
#[ignore = "needs MAILBOX_TEST_PG_URL"]
async fn postgres_meets_the_actions_contract() {
    let url =
        std::env::var("MAILBOX_TEST_PG_URL").expect("set MAILBOX_TEST_PG_URL to run this test");
    let store = PgStore::connect(&url).await.unwrap();
    let run = format!(
        "a{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    contract(&store, &run).await;
}

#[test]
fn migration_0004_exists_for_both_dialects_with_portable_types() {
    for (file, big) in [
        ("../../migrations/sqlite/0004_actions.sql", "integer"),
        ("../../migrations/postgres/0004_actions.sql", "bigint"),
    ] {
        let sql = std::fs::read_to_string(file).unwrap().to_lowercase();
        assert!(sql.contains("folder_roles") && sql.contains("pending_ops"));
        assert!(sql.contains(big));
        for forbidden in [
            "password",
            "secret",
            "token",
            "uuid",
            "timestamptz",
            "boolean",
            "serial",
        ] {
            assert!(!sql.contains(forbidden), "{file} uses `{forbidden}`");
        }
    }
}
