//! Contract for keyset paging, local search, stored headers and thread ids (MB-200, MB-201).
//! Runs against SQLite always and PostgreSQL when `MAILBOX_TEST_PG_URL` is set.
use mailbox_core::MailboxError;
use mailbox_core::body::{BodyRecord, BodyStore};
use mailbox_core::search::{
    Narrowing, PageKey, PageStore, Scope, SearchEngine, SearchQuery, SearchStore,
};
use mailbox_core::store::{AccountRecord, FolderRecord, MailStore, MessageRecord};
use mailbox_core::threading::{EnvelopeHeaders, HeaderRow, ThreadStore, rethread_account};
use mailbox_store::{PgStore, SqliteStore};

fn account(id: &str, owner: &str) -> AccountRecord {
    AccountRecord {
        id: id.into(),
        owner_id: owner.into(),
        display_name: id.into(),
        email_address: format!("me@{id}.example"),
        imap_host: "imap.example.com".into(),
        imap_port: 993,
        smtp_host: "smtp.example.com".into(),
        smtp_port: 465,
        auth_type: "password".into(),
        credential_ref: format!("keychain:{id}"),
        status: "active".into(),
    }
}

fn folder(id: &str, account: &str, name: &str, role: Option<&str>) -> FolderRecord {
    FolderRecord {
        id: id.into(),
        account_id: account.into(),
        remote_name: name.into(),
        delimiter: Some(".".into()),
        special_use: role.map(str::to_string),
        uid_validity: Some(10),
        uid_next: None,
    }
}

fn message(id: &str, account: &str, folder: &str, uid: u32, date: i64) -> MessageRecord {
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
        internal_date: date,
        size_bytes: 1,
        seen: false,
        flagged: false,
    }
}

fn ids(list: &[MessageRecord]) -> Vec<String> {
    list.iter().map(|m| m.id.clone()).collect()
}

fn query(text: &str, scope: Scope) -> SearchQuery {
    SearchQuery {
        text: text.into(),
        scope,
        narrowing: Narrowing::default(),
        limit: 50,
    }
}

async fn search_ids<S: SearchStore>(store: &S, owner: &str, q: &SearchQuery) -> Vec<String> {
    store.refresh_search_index(owner, 10_000).await.unwrap();
    let mut out: Vec<String> = store
        .search(owner, q)
        .await
        .unwrap()
        .hits
        .into_iter()
        .map(|h| h.message.id)
        .collect();
    out.sort();
    out
}

async fn contract<S>(store: &S, p: &str)
where
    S: MailStore + PageStore + SearchStore + ThreadStore + BodyStore,
{
    let owner = format!("{p}owner");
    let other = format!("{p}other");
    let (a, b) = (format!("{p}a"), format!("{p}b"));
    store.upsert_account(&account(&a, &owner)).await.unwrap();
    store.upsert_account(&account(&b, &owner)).await.unwrap();
    store
        .upsert_account(&account(&format!("{p}x"), &other))
        .await
        .unwrap();
    let (a_in, a_sent, a_proj) = (
        format!("{a}/INBOX"),
        format!("{a}/Sent"),
        format!("{a}/Proj"),
    );
    let b_in = format!("{b}/INBOX");
    for f in [
        folder(&a_in, &a, "INBOX", Some("inbox")),
        folder(&a_sent, &a, "Sent", Some("sent")),
        folder(&a_proj, &a, "Proj", None),
        folder(&b_in, &b, "INBOX", Some("inbox")),
    ] {
        store.upsert_folder(&owner, &f).await.unwrap();
    }

    // --- Keyset paging: 25 inbox messages of A with equal dates in pairs, 5 of B.
    let mut batch = Vec::new();
    for uid in 1..=25u32 {
        batch.push(message(
            &format!("{p}a{uid:02}"),
            &a,
            &a_in,
            uid,
            i64::from(uid / 2) * 100,
        ));
    }
    store.upsert_messages(&owner, &batch).await.unwrap();
    let b_batch: Vec<MessageRecord> = (1..=5u32)
        .map(|uid| {
            message(
                &format!("{p}b{uid:02}"),
                &b,
                &b_in,
                uid,
                i64::from(uid) * 100 + 50,
            )
        })
        .collect();
    store.upsert_messages(&owner, &b_batch).await.unwrap();

    let scope = Scope::Folder {
        account_id: a.clone(),
        folder_id: a_in.clone(),
    };
    let none = Narrowing::default();
    let mut seen: Vec<String> = Vec::new();
    let mut before: Option<PageKey> = None;
    loop {
        let page = store
            .page(&owner, &scope, &none, before.as_ref(), 7)
            .await
            .unwrap();
        if page.is_empty() {
            break;
        }
        assert!(page.len() <= 7);
        // A message arriving between pages must not shift what the next page returns.
        if seen.is_empty() {
            store
                .upsert_messages(
                    &owner,
                    &[message(&format!("{p}a99"), &a, &a_in, 99, 99_999)],
                )
                .await
                .unwrap();
        }
        before = page.last().map(PageKey::of);
        seen.extend(ids(&page));
    }
    assert_eq!(seen.len(), 25, "every message exactly once");
    let mut sorted = seen.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 25, "no duplicates across pages");
    assert!(
        !seen.contains(&format!("{p}a99")),
        "newer arrival does not shift older pages"
    );
    // Order: date desc, then id asc for equal dates.
    assert_eq!(seen[0], format!("{p}a24"));
    assert_eq!(seen[1], format!("{p}a25"));

    // Unified inbox across accounts, newest first, ties by id.
    let unified = Scope::Role {
        role: "inbox".into(),
        account_id: None,
    };
    let first = store.page(&owner, &unified, &none, None, 4).await.unwrap();
    assert_eq!(
        ids(&first),
        vec![
            format!("{p}a99"),
            format!("{p}a24"),
            format!("{p}a25"),
            format!("{p}a22")
        ]
    );
    let all_unified = store
        .page(&owner, &unified, &none, None, 500)
        .await
        .unwrap();
    assert_eq!(all_unified.len(), 31);
    // Another owner sees nothing, not even with a role scope.
    assert!(
        store
            .page(&other, &unified, &none, None, 50)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        store.page(&other, &scope, &none, None, 50).await,
        Ok(v) if v.is_empty()
    ));

    // `since` returns everything at or above a floor.
    let floor = PageKey::of(&first[2]);
    let since = store.since(&owner, &unified, &floor, 1000).await.unwrap();
    assert_eq!(ids(&since), ids(&first[..3]));
    assert_eq!(
        store.lowest_uid(&owner, &a, &a_in, 10).await.unwrap(),
        Some(1)
    );
    assert_eq!(store.lowest_uid(&owner, &a, &a_in, 11).await.unwrap(), None);
    assert_eq!(store.lowest_uid(&other, &a, &a_in, 10).await.unwrap(), None);

    // Narrowing applied in the query: unread only.
    let mut read = message(&format!("{p}a01"), &a, &a_in, 1, 0);
    read.seen = true;
    store.upsert_messages(&owner, &[read]).await.unwrap();
    let unread = Narrowing {
        unread_only: true,
        ..Narrowing::default()
    };
    let page = store
        .page(&owner, &scope, &unread, None, 500)
        .await
        .unwrap();
    assert_eq!(page.len(), 25, "26 in folder, one read");

    // --- Search: subject, sender, recipients and body, Vietnamese with and without accents.
    let mut invoice = message(&format!("{p}inv"), &a, &a_proj, 1, 5_000);
    invoice.subject = "Hóa đơn tháng 11".into();
    invoice.from_display = "Nguyễn Văn Ánh".into();
    invoice.from_address = "anh@ketoan.vn".into();
    let mut report = message(&format!("{p}rep"), &b, &b_in, 50, 6_000);
    report.subject = "Báo cáo 100% xong".into();
    store
        .upsert_messages(&owner, &[invoice.clone(), report.clone()])
        .await
        .unwrap();
    store
        .store_headers(
            &owner,
            &[HeaderRow {
                message_id: report.id.clone(),
                account_id: b.clone(),
                headers: EnvelopeHeaders {
                    to: "Phòng Kỹ thuật <kythuat@congty.vn>".into(),
                    ..EnvelopeHeaders::default()
                },
            }],
        )
        .await
        .unwrap();
    store
        .save_body(
            &owner,
            &BodyRecord {
                message_id: invoice.id.clone(),
                text: "Kính gửi anh, số tiền thanh toán là 12.500.000 đồng.".into(),
                html: None,
                html_remote: None,
                remote_blocked: false,
                truncated: false,
                attachments: Vec::new(),
            },
        )
        .await
        .unwrap();

    let all = Scope::All;
    for text in [
        "hoa don",
        "HÓA ĐƠN",
        "hóa",
        "nguyen van",
        "ketoan.vn",
        "thanh toan",
        "dong",
    ] {
        assert_eq!(
            search_ids(store, &owner, &query(text, all.clone())).await,
            vec![invoice.id.clone()],
            "query {text:?}"
        );
    }
    assert_eq!(
        search_ids(store, &owner, &query("ky thuat", all.clone())).await,
        vec![report.id.clone()],
        "recipients are searchable"
    );
    assert_eq!(
        search_ids(store, &owner, &query("hoa don thanh", all.clone())).await,
        vec![invoice.id.clone()],
        "all terms must match"
    );
    assert!(
        search_ids(store, &owner, &query("hoa bao", all.clone()))
            .await
            .is_empty()
    );

    // Scope: folder, account, all; and another owner never sees a hit.
    let folder_scope = Scope::Folder {
        account_id: a.clone(),
        folder_id: a_in.clone(),
    };
    assert!(
        search_ids(store, &owner, &query("hoa don", folder_scope))
            .await
            .is_empty()
    );
    assert_eq!(
        search_ids(store, &owner, &query("hoa don", Scope::Account(a.clone())))
            .await
            .len(),
        1
    );
    assert!(
        search_ids(store, &owner, &query("hoa don", Scope::Account(b.clone())))
            .await
            .is_empty()
    );
    assert!(
        search_ids(store, &other, &query("hoa don", all.clone()))
            .await
            .is_empty()
    );

    // Narrowing in the query: today (since), unread, account.
    let mut q = query("hoa don", all.clone());
    q.narrowing.since = Some(5_001);
    assert!(search_ids(store, &owner, &q).await.is_empty());
    q.narrowing.since = Some(5_000);
    assert_eq!(search_ids(store, &owner, &q).await.len(), 1);
    q.narrowing.account_id = Some(b.clone());
    assert!(search_ids(store, &owner, &q).await.is_empty());

    // Total counts beyond the limit; results newest first.
    let mut many = query("s1", Scope::Account(a.clone()));
    many.limit = 3;
    store.refresh_search_index(&owner, 10_000).await.unwrap();
    let page = store.search(&owner, &many).await.unwrap();
    assert_eq!(page.hits.len(), 3);
    assert!(page.total >= 11, "S1, S10..S19 at least: {}", page.total);
    assert!(
        page.hits
            .windows(2)
            .all(|w| w[0].message.internal_date >= w[1].message.internal_date)
    );
    if cfg!(test) {
        assert!(matches!(
            store.search_engine(),
            SearchEngine::Fts5 | SearchEngine::Like
        ));
    }

    // Injection attempts are plain text: no error, no extra rows, data intact.
    for evil in [
        "'; DROP TABLE messages; --",
        "\" OR 1=1 --",
        "hoa\" OR \"",
        "NEAR(hoa don)",
        "subject:hoa",
        "*",
        "%",
        "_",
        "\\",
        "hoa)",
        "\u{0}",
        &"x".repeat(5_000),
    ] {
        let hits = search_ids(store, &owner, &query(evil, all.clone())).await;
        assert!(hits.len() <= 1, "{evil:?} -> {hits:?}");
    }
    assert_eq!(
        search_ids(store, &owner, &query("100%", all.clone())).await,
        vec![report.id.clone()],
        "a % in the query is no wildcard"
    );
    assert_eq!(
        store
            .page(&owner, &scope, &none, None, 500)
            .await
            .unwrap()
            .len(),
        26
    );
    assert!(
        search_ids(store, &owner, &query("   ", all.clone()))
            .await
            .is_empty()
    );

    // A changed subject is re-indexed.
    let mut renamed = invoice.clone();
    renamed.subject = "Biên bản họp".into();
    store.upsert_messages(&owner, &[renamed]).await.unwrap();
    store
        .store_headers(
            &owner,
            &[HeaderRow {
                message_id: invoice.id.clone(),
                account_id: a.clone(),
                headers: EnvelopeHeaders::default(),
            }],
        )
        .await
        .unwrap();
    assert_eq!(
        search_ids(store, &owner, &query("bien ban", all.clone()))
            .await
            .len(),
        1
    );

    // --- Headers and threads.
    let root = message(&format!("{p}t1"), &a, &a_in, 201, 10_000);
    let mut reply = message(&format!("{p}t2"), &a, &a_sent, 5, 11_000);
    reply.subject = "Re: S201".into();
    let mut reply_in = reply.clone();
    reply_in.id = format!("{p}t2in");
    reply_in.folder_id = a_in.clone();
    reply_in.imap_uid = 202;
    let stranger = message(&format!("{p}t3"), &a, &a_in, 203, 10_500);
    store
        .upsert_messages(
            &owner,
            &[
                root.clone(),
                reply.clone(),
                reply_in.clone(),
                stranger.clone(),
            ],
        )
        .await
        .unwrap();
    let answer = EnvelopeHeaders {
        in_reply_to: root.message_id_header.clone(),
        references: vec![root.message_id_header.clone().unwrap()],
        ..EnvelopeHeaders::default()
    };
    store
        .store_headers(
            &owner,
            &[
                HeaderRow {
                    message_id: reply.id.clone(),
                    account_id: a.clone(),
                    headers: answer.clone(),
                },
                HeaderRow {
                    message_id: reply_in.id.clone(),
                    account_id: a.clone(),
                    headers: answer.clone(),
                },
            ],
        )
        .await
        .unwrap();
    assert_eq!(
        store.headers_of(&owner, &reply.id).await.unwrap(),
        Some(answer.clone())
    );
    assert_eq!(store.headers_of(&other, &reply.id).await.unwrap(), None);
    assert_eq!(store.headers_of(&owner, &root.id).await.unwrap(), None);
    // Messages without headers are listed for backfill (current UIDVALIDITY only).
    let missing = store
        .uids_missing_headers(&owner, &a, &a_in, 10, 1000)
        .await
        .unwrap();
    assert!(missing.contains(&201) && !missing.contains(&202));
    assert!(
        store
            .uids_missing_headers(&owner, &a, &a_in, 11, 1000)
            .await
            .unwrap()
            .is_empty()
    );
    // Storing headers for a message of another owner is refused.
    assert_eq!(
        store
            .store_headers(
                &other,
                &[HeaderRow {
                    message_id: root.id.clone(),
                    account_id: a.clone(),
                    headers: answer.clone()
                }]
            )
            .await,
        Err(MailboxError::NotFound)
    );

    let changed = rethread_account(store, &owner, &a, &[format!("me@{a}.example")])
        .await
        .unwrap();
    assert!(changed > 0);
    assert_eq!(
        rethread_account(store, &owner, &a, &[]).await.unwrap(),
        0,
        "recomputing without new mail changes nothing"
    );
    let map: std::collections::HashMap<String, String> = store
        .thread_ids(&owner, &a)
        .await
        .unwrap()
        .into_iter()
        .collect();
    let t = map.get(&root.id).cloned().expect("root has a thread");
    assert_eq!(map.get(&reply.id), Some(&t));
    assert_eq!(map.get(&reply_in.id), Some(&t));
    assert_ne!(map.get(&stranger.id), Some(&t));
    let sizes: std::collections::HashMap<String, usize> = store
        .thread_sizes(&owner, &a)
        .await
        .unwrap()
        .into_iter()
        .collect();
    assert_eq!(
        sizes.get(&t),
        Some(&2),
        "the Sent copy and the inbox copy are one message"
    );
    let convo = store.conversation(&owner, &a, &t).await.unwrap();
    assert_eq!(
        convo.first().map(|m| m.id.clone()),
        Some(root.id.clone()),
        "oldest first"
    );
    assert_eq!(convo.len(), 3);
    assert!(store.conversation(&other, &a, &t).await.unwrap().is_empty());
    assert!(store.thread_ids(&other, &a).await.unwrap().is_empty());
    // Search hits carry the thread id.
    store.refresh_search_index(&owner, 10_000).await.unwrap();
    let hit = store
        .search(&owner, &query("S201", Scope::Account(a.clone())))
        .await
        .unwrap();
    assert!(
        hit.hits
            .iter()
            .all(|h| h.thread_id.as_deref() == Some(t.as_str()))
    );

    // Deleting the account removes headers, index rows and threads with it.
    store.delete_account(&owner, &a).await.unwrap();
    assert!(
        search_ids(store, &owner, &query("hoa don", all.clone()))
            .await
            .is_empty()
    );
    assert_eq!(store.headers_of(&owner, &reply.id).await.unwrap(), None);
}

#[tokio::test]
async fn sqlite_meets_the_search_contract() {
    let store = SqliteStore::in_memory().await.unwrap();
    assert_eq!(
        store.search_engine(),
        SearchEngine::Fts5,
        "bundled SQLite has FTS5"
    );
    contract(&store, "s").await;
}

#[tokio::test]
async fn sqlite_like_fallback_meets_the_search_contract() {
    let store = SqliteStore::in_memory().await.unwrap().without_fts();
    assert_eq!(store.search_engine(), SearchEngine::Like);
    contract(&store, "l").await;
}

#[tokio::test]
#[ignore = "needs MAILBOX_TEST_PG_URL"]
async fn postgres_meets_the_search_contract() {
    let url =
        std::env::var("MAILBOX_TEST_PG_URL").expect("set MAILBOX_TEST_PG_URL to run this test");
    let store = PgStore::connect(&url).await.unwrap();
    assert_eq!(store.search_engine(), SearchEngine::Like);
    let run = format!(
        "q{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    contract(&store, &run).await;
}
