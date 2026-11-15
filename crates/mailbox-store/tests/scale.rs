//! MB-200 timing with 50,000 cached envelopes (opt-in: `cargo test -p mailbox-store --test scale
//! -- --ignored --nocapture`). Prints the measured numbers; asserts generous upper bounds only, so
//! a slow machine does not fail it but a quadratic regression does. Synthetic data only.
use mailbox_core::body::{BodyRecord, BodyStore};
use mailbox_core::search::{Narrowing, PageKey, PageStore, Scope, SearchQuery, SearchStore};
use mailbox_core::store::{AccountRecord, FolderRecord, MailStore, MessageRecord};
use mailbox_core::threading::{EnvelopeHeaders, HeaderRow, ThreadStore, rethread_account};
use mailbox_store::SqliteStore;
use std::time::{Duration, Instant};

const OWNER: &str = "local";
const PER_ACCOUNT: u32 = 25_000;
const WORDS: [&str; 12] = [
    "báo cáo",
    "hóa đơn",
    "kế hoạch",
    "họp",
    "dự án",
    "khách hàng",
    "invoice",
    "meeting",
    "release",
    "thiết kế",
    "hợp đồng",
    "tuần",
];

fn ms(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1000.0)
}

async fn seed(store: &SqliteStore, account: &str) {
    store
        .upsert_account(&AccountRecord {
            id: account.into(),
            owner_id: OWNER.into(),
            display_name: account.into(),
            email_address: format!("me@{account}.example"),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 465,
            auth_type: "password".into(),
            credential_ref: format!("imap.{account}"),
            status: "active".into(),
        })
        .await
        .unwrap();
    for (name, role) in [("INBOX", "inbox"), ("Sent", "sent")] {
        store
            .upsert_folder(
                OWNER,
                &FolderRecord {
                    id: format!("{account}/{name}"),
                    account_id: account.into(),
                    remote_name: name.into(),
                    delimiter: None,
                    special_use: Some(role.into()),
                    uid_validity: Some(1),
                    uid_next: None,
                },
            )
            .await
            .unwrap();
    }
    let mut batch = Vec::new();
    let mut headers = Vec::new();
    for uid in 1..=PER_ACCOUNT {
        let folder = if uid % 5 == 0 { "Sent" } else { "INBOX" };
        let id = format!("{account}/{folder}#1#{uid}");
        let word = WORDS[(uid as usize) % WORDS.len()];
        // Conversations of 10 messages each.
        let root = uid - (uid - 1) % 10;
        batch.push(MessageRecord {
            id: id.clone(),
            account_id: account.into(),
            folder_id: format!("{account}/{folder}"),
            uid_validity: 1,
            imap_uid: uid,
            message_id_header: Some(format!("<{account}.{uid}@example.com>")),
            subject: format!("Re: {word} số {}", root),
            from_display: format!("Người gửi {}", uid % 97),
            from_address: format!("sender{}@example.org", uid % 97),
            preview: String::new(),
            internal_date: 1_700_000_000 + i64::from(uid) * 60,
            size_bytes: 2048,
            seen: uid % 3 == 0,
            flagged: false,
        });
        headers.push(HeaderRow {
            message_id: id,
            account_id: account.into(),
            headers: EnvelopeHeaders {
                in_reply_to: (uid != root).then(|| format!("<{account}.{}@example.com>", uid - 1)),
                references: if uid == root {
                    Vec::new()
                } else {
                    vec![format!("<{account}.{root}@example.com>")]
                },
                to: format!("Nhóm {} <team{}@example.org>", uid % 7, uid % 7),
                cc: String::new(),
            },
        });
        if batch.len() == 2_500 {
            store.upsert_messages(OWNER, &batch).await.unwrap();
            store.store_headers(OWNER, &headers).await.unwrap();
            batch.clear();
            headers.clear();
        }
    }
    // Bodies for 1 in 10 messages.
    for uid in (1..=PER_ACCOUNT).step_by(10) {
        let folder = if uid % 5 == 0 { "Sent" } else { "INBOX" };
        store
            .save_body(
                OWNER,
                &BodyRecord {
                    message_id: format!("{account}/{folder}#1#{uid}"),
                    text: format!(
                        "Kính gửi anh chị, nội dung trao đổi về {} lần {uid}. Trân trọng.",
                        WORDS[(uid as usize * 7) % WORDS.len()]
                    ),
                    html: None,
                    html_remote: None,
                    remote_blocked: false,
                    truncated: false,
                    attachments: Vec::new(),
                },
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
#[ignore = "timing run with 50,000 envelopes; run explicitly"]
async fn fifty_thousand_envelopes_stay_responsive() {
    let dir = std::env::temp_dir().join(format!("mailbox-scale-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("scale.sqlite3");
    let _ = std::fs::remove_file(&path);
    let store = SqliteStore::open(&path).await.unwrap();

    let t = Instant::now();
    seed(&store, "acc-a").await;
    seed(&store, "acc-b").await;
    println!(
        "seed 50,000 envelopes + headers + 5,000 bodies: {}",
        ms(t.elapsed())
    );

    let t = Instant::now();
    let mut indexed = 0;
    loop {
        let n = store.refresh_search_index(OWNER, 10_000).await.unwrap();
        if n == 0 {
            break;
        }
        indexed += n;
    }
    println!("index {indexed} messages (first time): {}", ms(t.elapsed()));
    assert_eq!(indexed, 50_000);
    let t = Instant::now();
    assert_eq!(store.refresh_search_index(OWNER, 10_000).await.unwrap(), 0);
    let idle = t.elapsed();
    println!("index check with nothing to do: {}", ms(idle));

    let t = Instant::now();
    for account in ["acc-a", "acc-b"] {
        rethread_account(&store, OWNER, account, &[]).await.unwrap();
    }
    println!(
        "thread 2 x 25,000 messages (first time): {}",
        ms(t.elapsed())
    );
    let t = Instant::now();
    let changed = rethread_account(&store, OWNER, "acc-a", &[]).await.unwrap();
    println!(
        "re-thread 25,000 with no change ({changed} written): {}",
        ms(t.elapsed())
    );
    assert_eq!(
        store.thread_sizes(OWNER, "acc-a").await.unwrap().len(),
        2_500
    );

    let unified = Scope::Role {
        role: "inbox".into(),
        account_id: None,
    };
    let none = Narrowing::default();
    let t = Instant::now();
    let first = store.page(OWNER, &unified, &none, None, 100).await.unwrap();
    let first_page = t.elapsed();
    println!("unified inbox first page (100): {}", ms(first_page));
    // Walk 50 pages deep with keyset paging.
    let mut before = first.last().map(PageKey::of);
    let t = Instant::now();
    let mut worst = Duration::ZERO;
    for _ in 0..50 {
        let s = Instant::now();
        let page = store
            .page(OWNER, &unified, &none, before.as_ref(), 100)
            .await
            .unwrap();
        worst = worst.max(s.elapsed());
        before = page.last().map(PageKey::of);
    }
    println!(
        "50 further pages: {} total, worst page {}",
        ms(t.elapsed()),
        ms(worst)
    );
    let folder = Scope::Folder {
        account_id: "acc-a".into(),
        folder_id: "acc-a/INBOX".into(),
    };
    let t = Instant::now();
    let unread = Narrowing {
        unread_only: true,
        ..Narrowing::default()
    };
    store
        .page(OWNER, &folder, &unread, None, 100)
        .await
        .unwrap();
    println!("one folder, unread only, first page: {}", ms(t.elapsed()));

    let mut worst_search = Duration::ZERO;
    for (text, scope) in [
        ("hoa don", Scope::All),
        ("HÓA ĐƠN", Scope::All),
        ("nguoi gui 42", Scope::Account("acc-a".into())),
        ("team3@example.org", Scope::All),
        ("trao doi thiet ke", Scope::All),
        ("khong co tu nay", Scope::All),
        ("hop", folder.clone()),
    ] {
        let q = SearchQuery {
            text: text.into(),
            scope,
            narrowing: Narrowing::default(),
            limit: 200,
        };
        let t = Instant::now();
        let page = store.search(OWNER, &q).await.unwrap();
        let took = t.elapsed();
        worst_search = worst_search.max(took);
        println!(
            "search {text:?} [{:?}]: {} hits shown, {} total, {}",
            page.engine,
            page.hits.len(),
            page.total,
            ms(took)
        );
    }
    let like = store.clone().without_fts();
    let mut worst_like = Duration::ZERO;
    for text in ["hoa don", "trao doi thiet ke", "khong co tu nay"] {
        let q = SearchQuery {
            text: text.into(),
            scope: Scope::All,
            narrowing: Narrowing::default(),
            limit: 200,
        };
        let t = Instant::now();
        let page = like.search(OWNER, &q).await.unwrap();
        let took = t.elapsed();
        worst_like = worst_like.max(took);
        println!(
            "search {text:?} [LIKE fallback]: {} total, {}",
            page.total,
            ms(took)
        );
    }

    // Generous bounds: interactive on a laptop, far from any quadratic blow-up.
    assert!(first_page < Duration::from_millis(250), "first page");
    assert!(worst < Duration::from_millis(250), "deep page");
    assert!(worst_search < Duration::from_millis(500), "FTS search");
    assert!(worst_like < Duration::from_millis(2_000), "LIKE search");
    assert!(idle < Duration::from_millis(500), "idle index check");
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}
