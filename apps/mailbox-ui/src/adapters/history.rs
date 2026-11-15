//! Desktop Real mode: older mail, local search and conversations (MB-200, MB-201).
//!
//! - How much of each folder the message list shows is a per-folder depth (default `WINDOW`);
//!   "load older" lowers a keyset floor for the current view, first from the local cache, then by
//!   fetching an older UID window from the server, and reports when the folder start is reached.
//! - Search runs on the local cache through `SearchStore` (FTS5, `LIKE` fallback). Search words
//!   are never logged or stored.
//! - Thread ids are computed per account after a sync and kept in a small in-process map that
//!   the list uses to group rows; the message snapshot itself is unchanged.
use super::account::{OWNER, WINDOW, format_time, open_store};
use mailbox_contracts::MessageSummary;
use mailbox_core::folders::role_of;
use mailbox_core::history::{backfill_headers, fetch_older};
use mailbox_core::search::{
    Narrowing, PageKey, PageStore, Scope, SearchEngine, SearchQuery, SearchStore,
};
use mailbox_core::secrets::SecretStore;
use mailbox_core::store::{AccountRecord, FolderRecord, MailStore, MessageRecord};
use mailbox_core::threading::{ThreadStore, rethread_account};
use mailbox_core::{MailServerEndpoint, MailboxError, TlsMode};
use mailbox_secrets::KeychainStore;
use mailbox_store::SqliteStore;
use std::collections::{BTreeMap, HashMap};
use std::sync::RwLock;

/// Envelopes added per "load older".
pub const PAGE: u32 = 100;
/// Results shown per search.
pub const SEARCH_LIMIT: u32 = 200;
/// Messages indexed for search per refresh (more follow on the next one).
const INDEX_BATCH: u32 = 5_000;
/// Headers backfilled per folder and sync.
const BACKFILL_PER_SYNC: u32 = 300;
/// Upper bound of what one folder shows, however far the user scrolls.
const MAX_DEPTH: u32 = 20_000;

#[derive(Default)]
struct Memory {
    /// Folder id → number of newest messages the snapshot loads.
    depth: HashMap<String, u32>,
    /// View key → keyset floor reached by "load older".
    floor: HashMap<String, PageKey>,
    /// Message id → thread id.
    threads: HashMap<String, String>,
    /// Thread id → distinct messages in the conversation (all folders).
    sizes: HashMap<String, usize>,
}

static MEMORY: RwLock<Option<Memory>> = RwLock::new(None);

fn with<R>(f: impl FnOnce(&mut Memory) -> R) -> R {
    let mut guard = MEMORY.write().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Memory::default))
}

/// How many newest messages of a folder the snapshot shows.
pub fn depth(folder_id: &str) -> u32 {
    with(|m| {
        m.depth
            .get(folder_id)
            .copied()
            .unwrap_or(WINDOW)
            .max(WINDOW)
    })
}

/// Thread id of a message, when its account has been threaded.
pub fn thread_of(message_id: &str) -> Option<String> {
    with(|m| m.threads.get(message_id).cloned())
}

/// Distinct messages of a conversation across folders.
pub fn size_of(thread_id: &str) -> Option<usize> {
    with(|m| m.sizes.get(thread_id).copied())
}

/// Test hook: put a thread map in place without a store.
#[cfg(test)]
pub fn seed_threads(threads: &[(&str, &str)], sizes: &[(&str, usize)]) {
    with(|m| {
        m.threads
            .extend(threads.iter().map(|(a, b)| (a.to_string(), b.to_string())));
        m.sizes
            .extend(sizes.iter().map(|(a, b)| (a.to_string(), *b)));
    });
}

/// Reload the thread map of every account (called with each snapshot).
pub async fn remember_threads(store: &(impl MailStore + ThreadStore)) -> Result<(), MailboxError> {
    let mut threads = HashMap::new();
    let mut sizes = HashMap::new();
    for a in store.list_accounts(OWNER).await? {
        threads.extend(store.thread_ids(OWNER, &a.id).await?);
        sizes.extend(store.thread_sizes(OWNER, &a.id).await?);
    }
    with(|m| {
        m.threads = threads;
        m.sizes = sizes;
    });
    Ok(())
}

/// Which list the user is looking at, in store terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewSpec {
    /// A role (`inbox`, `sent`, ...) across accounts or for one account.
    Role {
        role: String,
        account: Option<String>,
    },
    /// One of the user's own folders.
    Folder(String),
}

impl ViewSpec {
    fn key(&self) -> String {
        match self {
            ViewSpec::Role { role, account } => {
                format!("role:{role}:{}", account.as_deref().unwrap_or("*"))
            }
            ViewSpec::Folder(id) => format!("folder:{id}"),
        }
    }

    fn includes(&self, folder: &FolderRecord) -> bool {
        match self {
            ViewSpec::Role { role, account } => {
                role_of(folder).is_some_and(|r| r.key() == role)
                    && account.as_deref().is_none_or(|a| a == folder.account_id)
            }
            ViewSpec::Folder(id) => &folder.id == id,
        }
    }

    fn scope(&self, folders: &[FolderRecord]) -> Scope {
        match self {
            ViewSpec::Role { role, account } => Scope::Role {
                role: role.clone(),
                account_id: account.clone(),
            },
            ViewSpec::Folder(id) => Scope::Folder {
                account_id: folders
                    .iter()
                    .find(|f| &f.id == id)
                    .map(|f| f.account_id.clone())
                    .unwrap_or_default(),
                folder_id: id.clone(),
            },
        }
    }
}

/// What "load older" achieved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Older {
    /// Messages newly shown from the local cache.
    pub shown: usize,
    /// Messages fetched from the server this time.
    pub fetched: usize,
    /// Every folder of the view is cached down to its first message.
    pub at_start: bool,
    /// Some folder could not be reached; what is cached is still shown.
    pub offline: bool,
}

async fn folders_in(
    store: &SqliteStore,
    view: &ViewSpec,
) -> Result<Vec<(AccountRecord, FolderRecord)>, MailboxError> {
    let mut out = Vec::new();
    for a in store.list_accounts(OWNER).await? {
        for f in store.list_folders(OWNER, &a.id).await? {
            if view.includes(&f) {
                out.push((a.clone(), f));
            }
        }
    }
    Ok(out)
}

/// The oldest key the view currently shows completely: the newest of the per-folder tails.
async fn current_floor(
    store: &SqliteStore,
    view: &ViewSpec,
    folders: &[(AccountRecord, FolderRecord)],
) -> Result<Option<PageKey>, MailboxError> {
    if let Some(f) = with(|m| m.floor.get(&view.key()).cloned()) {
        return Ok(Some(f));
    }
    let mut floor: Option<PageKey> = None;
    for (a, f) in folders {
        let limit = depth(&f.id);
        let shown = store.list_messages(OWNER, &a.id, &f.id, limit).await?;
        // A folder shown completely says nothing about how far the others go.
        if (shown.len() as u32) < limit {
            continue;
        }
        if let Some(last) = shown.last() {
            let key = PageKey::of(last);
            let newer = floor.as_ref().is_none_or(|cur| {
                (key.internal_date, std::cmp::Reverse(&key.id))
                    > (cur.internal_date, std::cmp::Reverse(&cur.id))
            });
            if newer {
                floor = Some(key);
            }
        }
    }
    Ok(floor)
}

/// Lower the view's floor by one page of the cache; returns how many messages it added.
async fn page_down(
    store: &SqliteStore,
    view: &ViewSpec,
    folders: &[(AccountRecord, FolderRecord)],
) -> Result<usize, MailboxError> {
    let all: Vec<FolderRecord> = folders.iter().map(|(_, f)| f.clone()).collect();
    let scope = view.scope(&all);
    let floor = current_floor(store, view, folders).await?;
    let page = store
        .page(OWNER, &scope, &Narrowing::default(), floor.as_ref(), PAGE)
        .await?;
    let Some(last) = page.last() else {
        return Ok(0);
    };
    let new_floor = PageKey::of(last);
    // Each folder now shows every message at or above the new floor.
    let mut depths = Vec::new();
    for (a, f) in folders {
        let scope = Scope::Folder {
            account_id: a.id.clone(),
            folder_id: f.id.clone(),
        };
        let n = store
            .since(OWNER, &scope, &new_floor, MAX_DEPTH)
            .await?
            .len() as u32;
        depths.push((f.id.clone(), n.clamp(WINDOW, MAX_DEPTH)));
    }
    with(|m| {
        m.floor.insert(view.key(), new_floor);
        m.depth.extend(depths);
    });
    Ok(page.len())
}

fn endpoint(a: &AccountRecord) -> MailServerEndpoint {
    MailServerEndpoint {
        hostname: a.imap_host.clone(),
        port: a.imap_port,
        tls_mode: TlsMode::ImplicitTls,
    }
}

/// Fetch older envelopes of every folder of the view from the server, one connection per account.
async fn fetch_view_from_server(
    store: &SqliteStore,
    folders: &[(AccountRecord, FolderRecord)],
) -> (usize, bool, bool) {
    let mut by_account: BTreeMap<String, (AccountRecord, Vec<FolderRecord>)> = BTreeMap::new();
    for (a, f) in folders {
        by_account
            .entry(a.id.clone())
            .or_insert_with(|| (a.clone(), Vec::new()))
            .1
            .push(f.clone());
    }
    let (mut fetched, mut at_start, mut offline) = (0, true, false);
    let _gate = super::actions::gate().await;
    for (account, list) in by_account.into_values() {
        let Ok(password) = KeychainStore::default().get(&account.credential_ref) else {
            offline = true;
            at_start = false;
            continue;
        };
        let Ok(mut session) =
            mailbox_imap::Session::connect(&endpoint(&account), &account.email_address, &password)
                .await
        else {
            offline = true;
            at_start = false;
            continue;
        };
        let mut stored = 0;
        for folder in &list {
            match fetch_older(store, &mut session, OWNER, folder, PAGE).await {
                Ok(report) => {
                    stored += report.stored;
                    at_start &= report.reached_start;
                }
                // Never synced: nothing to extend yet.
                Err(MailboxError::NotFound) => at_start = false,
                // UIDVALIDITY changed (the next sync rebuilds it) or the connection failed.
                Err(_) => {
                    at_start = false;
                    offline = true;
                }
            }
        }
        session.logout().await;
        if stored > 0 {
            let _ = rethread_account(
                store,
                OWNER,
                &account.id,
                std::slice::from_ref(&account.email_address),
            )
            .await;
        }
        fetched += stored;
    }
    (fetched, at_start, offline)
}

/// "Load older" for the view: cache first, then the server, then the cache again.
pub async fn load_older(view: ViewSpec) -> Result<Older, MailboxError> {
    let store = open_store().await?;
    let folders = folders_in(&store, &view).await?;
    if folders.is_empty() {
        return Ok(Older {
            at_start: true,
            ..Older::default()
        });
    }
    let shown = page_down(&store, &view, &folders).await?;
    if shown > 0 {
        return Ok(Older {
            shown,
            ..Older::default()
        });
    }
    let (fetched, at_start, offline) = fetch_view_from_server(&store, &folders).await;
    let shown = page_down(&store, &view, &folders).await?;
    let _ = store.refresh_search_index(OWNER, INDEX_BATCH).await;
    Ok(Older {
        shown,
        fetched,
        at_start: at_start && shown == 0,
        offline,
    })
}

/// After a sync, on the same connection: backfill reply headers of mail cached before they were
/// kept and recompute the account's conversations when anything changed. Best effort:
/// failures leave the previous state, they never undo the sync.
pub async fn after_sync<R>(store: &SqliteStore, remote: &mut R, account: &AccountRecord, new: usize)
where
    R: mailbox_core::sync::RemoteMailbox + mailbox_core::history::RemoteHistory,
{
    let mut changed = new;
    if let Ok(folders) = store.list_folders(OWNER, &account.id).await {
        for folder in &folders {
            if let Ok(n) = backfill_headers(store, remote, OWNER, folder, BACKFILL_PER_SYNC).await {
                changed += n;
            }
        }
    }
    let never_threaded = store
        .thread_ids(OWNER, &account.id)
        .await
        .map(|t| t.is_empty())
        .unwrap_or(true);
    if changed > 0 || never_threaded {
        let _ = rethread_account(
            store,
            OWNER,
            &account.id,
            std::slice::from_ref(&account.email_address),
        )
        .await;
    }
}

/// Index new mail for search after a sync, outside the IMAP gate (a first large backlog takes
/// seconds; later runs only touch new rows). Best effort.
pub async fn index_after_sync(store: &SqliteStore) {
    let _ = store.refresh_search_index(OWNER, INDEX_BATCH).await;
}

/// UI form of a cached message (same rules as the snapshot).
fn summary(m: MessageRecord, view: String, now: i64, account_email: &str) -> MessageSummary {
    let (short, full) = format_time(m.internal_date, now);
    MessageSummary {
        id: m.id,
        account_id: m.account_id,
        folder_id: view,
        from_name: if m.from_display.is_empty() {
            m.from_address.clone()
        } else {
            m.from_display
        },
        from_address: m.from_address,
        subject: if m.subject.is_empty() {
            "(Không có tiêu đề)".into()
        } else {
            m.subject
        },
        preview: String::new(),
        body_plain: String::new(),
        received_at: short,
        unread: !m.seen,
        starred: m.flagged,
        attachments: Vec::new(),
        thread_id: String::new(),
        to_line: account_email.to_string(),
        received_at_full: full,
        labels: Vec::new(),
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn view_keys(store: &SqliteStore) -> Result<HashMap<String, (String, String)>, MailboxError> {
    let mut out = HashMap::new();
    for a in store.list_accounts(OWNER).await? {
        for f in store.list_folders(OWNER, &a.id).await? {
            out.insert(
                f.id.clone(),
                (super::actions::view_key(&f), a.email_address.clone()),
            );
        }
    }
    Ok(out)
}

/// Search scope chosen in the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchIn {
    View(ViewSpec),
    Account(String),
    Everywhere,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub messages: Vec<MessageSummary>,
    pub total: u64,
    pub engine: SearchEngine,
}

/// Local search. `today_since` is the Unix time of today's start when "today" is on.
pub async fn search(
    text: String,
    scope: SearchIn,
    narrowing: Narrowing,
) -> Result<Found, MailboxError> {
    let store = open_store().await?;
    // Keep typing responsive: a large backlog is indexed after syncs, not in front of a search.
    store.refresh_search_index(OWNER, 2_000).await?;
    let scope = match scope {
        SearchIn::Everywhere => Scope::All,
        SearchIn::Account(a) => Scope::Account(a),
        SearchIn::View(view) => {
            let folders: Vec<FolderRecord> = folders_in(&store, &view)
                .await?
                .into_iter()
                .map(|(_, f)| f)
                .collect();
            view.scope(&folders)
        }
    };
    let page = store
        .search(
            OWNER,
            &SearchQuery {
                text,
                scope,
                narrowing,
                limit: SEARCH_LIMIT,
            },
        )
        .await?;
    let views = view_keys(&store).await?;
    let now = now();
    let messages = page
        .hits
        .into_iter()
        .map(|h| {
            let (view, email) = views.get(&h.message.folder_id).cloned().unwrap_or_default();
            summary(h.message, view, now, &email)
        })
        .collect();
    Ok(Found {
        messages,
        total: page.total,
        engine: page.engine,
    })
}

/// The conversation of a message, oldest first, one entry per distinct `Message-ID` (a copy in
/// Sent and in the inbox shows once), with cached body text for quoting.
pub async fn conversation_of(message_id: &str) -> Result<Vec<MessageSummary>, MailboxError> {
    let store = open_store().await?;
    let Some(thread) = thread_of(message_id) else {
        return Ok(Vec::new());
    };
    let account = thread.split(':').next().unwrap_or_default().to_string();
    let views = view_keys(&store).await?;
    let now = now();
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for m in store.conversation(OWNER, &account, &thread).await? {
        let key = m.message_id_header.clone().unwrap_or_else(|| m.id.clone());
        // Prefer the copy the user opened, else the first one found.
        if seen.contains(&key) {
            if m.id == message_id
                && let Some(pos) = seen.iter().position(|k| k == &key)
            {
                let (view, email) = views.get(&m.folder_id).cloned().unwrap_or_default();
                out[pos] = summary(m, view, now, &email);
            }
            continue;
        }
        seen.push(key);
        let (view, email) = views.get(&m.folder_id).cloned().unwrap_or_default();
        out.push(summary(m, view, now, &email));
    }
    for s in out.iter_mut() {
        if let Ok(Some(body)) = mailbox_core::body::BodyStore::load_body(&store, OWNER, &s.id).await
        {
            s.body_plain = body.text;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_keys_are_distinct_per_role_account_and_folder() {
        let a = ViewSpec::Role {
            role: "inbox".into(),
            account: None,
        };
        let b = ViewSpec::Role {
            role: "inbox".into(),
            account: Some("acc".into()),
        };
        let c = ViewSpec::Folder("acc/inbox".into());
        assert_ne!(a.key(), b.key());
        assert_ne!(b.key(), c.key());
    }

    /// The cache half of "load older": each step lowers the view's floor by one page and makes
    /// the snapshot show that many more messages of the folder, until the cache is exhausted.
    #[test]
    fn load_older_pages_down_the_cache_and_raises_the_folder_depth() {
        use mailbox_core::store::FolderRecord;
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let store = SqliteStore::in_memory().await.unwrap();
            let account = AccountRecord {
                id: "acc-depth".into(),
                owner_id: OWNER.into(),
                display_name: "x".into(),
                email_address: "me@x.org".into(),
                imap_host: "imap.x.org".into(),
                imap_port: 993,
                smtp_host: "smtp.x.org".into(),
                smtp_port: 465,
                auth_type: "password".into(),
                credential_ref: "imap.acc-depth".into(),
                status: "active".into(),
            };
            store.upsert_account(&account).await.unwrap();
            let inbox = FolderRecord {
                id: "acc-depth/INBOX".into(),
                account_id: account.id.clone(),
                remote_name: "INBOX".into(),
                delimiter: None,
                special_use: Some("inbox".into()),
                uid_validity: Some(1),
                uid_next: None,
            };
            store.upsert_folder(OWNER, &inbox).await.unwrap();
            let rows: Vec<MessageRecord> = (1..=250u32)
                .map(|uid| MessageRecord {
                    id: format!("acc-depth/INBOX#1#{uid}"),
                    account_id: account.id.clone(),
                    folder_id: inbox.id.clone(),
                    uid_validity: 1,
                    imap_uid: uid,
                    message_id_header: None,
                    subject: format!("S{uid}"),
                    from_display: String::new(),
                    from_address: "a@x.org".into(),
                    preview: String::new(),
                    internal_date: i64::from(uid),
                    size_bytes: 1,
                    seen: true,
                    flagged: false,
                })
                .collect();
            store.upsert_messages(OWNER, &rows).await.unwrap();
            let view = ViewSpec::Role {
                role: "inbox".into(),
                account: Some(account.id.clone()),
            };
            let folders = vec![(account.clone(), inbox.clone())];
            assert_eq!(depth(&inbox.id), WINDOW);
            assert_eq!(page_down(&store, &view, &folders).await.unwrap(), 100);
            assert_eq!(depth(&inbox.id), 200);
            assert_eq!(page_down(&store, &view, &folders).await.unwrap(), 50);
            assert_eq!(depth(&inbox.id), 250);
            assert_eq!(
                page_down(&store, &view, &folders).await.unwrap(),
                0,
                "cache exhausted: the server is next"
            );
        });
    }

    #[test]
    fn depth_defaults_to_the_sync_window() {
        assert_eq!(depth("never-seen-folder"), WINDOW);
    }
}
