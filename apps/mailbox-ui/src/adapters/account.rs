//! Real accounts on Desktop (MB-121): account metadata in SQLite, the password in the OS
//! keychain, envelopes mirrored read-only from IMAP. Nothing here exists in the Web build.
use super::DataError;
use super::connect::CheckFailure;
use crate::i18n::tr;
use mailbox_contracts::{AccountStatus, AccountSummary, BootstrapResponse, MessageSummary};
use mailbox_core::actions::replay;
use mailbox_core::folders::{FolderRole, sync_all_folders};
use mailbox_core::secrets::{Secret, SecretStore};
use mailbox_core::store::{AccountRecord, MailStore};
use mailbox_core::{MailServerEndpoint, MailboxError, TlsMode};
use mailbox_secrets::KeychainStore;
use mailbox_store::SqliteStore;
use std::path::PathBuf;

/// Single local user on Desktop.
pub const OWNER: &str = "local";
/// Envelopes fetched on first sync and shown per account.
pub(super) const WINDOW: u32 = 100;
/// Folders synced per account; more than this is reported, not silently ignored.
const MAX_FOLDERS: usize = 64;
const COLORS: [&str; 4] = ["#2672F6", "#26B55C", "#8B5CF6", "#E8A317"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAccount {
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SyncSummary {
    pub folders: usize,
    pub new_messages: usize,
    /// Folders that could not be synced this time (the others were).
    pub failed_folders: usize,
}

/// Stable, filesystem- and keychain-safe id derived from the address.
pub fn account_id(email: &str) -> String {
    let slug: String = email
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("acc-{}", slug.trim_matches('-'))
}

fn secret_ref(account_id: &str) -> String {
    format!("imap.{account_id}")
}

/// Per-user application data directory (overridable with `MAILBOX_DATA_DIR`).
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("MAILBOX_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let home = PathBuf::from(
        std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".into()),
    );
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/app.mailbox.client")
    } else if cfg!(target_os = "windows") {
        std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or(home)
            .join("MailBox")
    } else {
        std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home.join(".local/share"))
            .join("mailbox")
    }
}

pub(super) async fn open_store() -> Result<SqliteStore, MailboxError> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|_| MailboxError::Storage("data_dir"))?;
    SqliteStore::open(&dir.join("mailbox.sqlite3")).await
}

pub(super) fn failure(e: MailboxError) -> CheckFailure {
    match e {
        MailboxError::ReauthenticationRequired | MailboxError::NotFound => CheckFailure::AuthFailed,
        MailboxError::TransientConnectionFailure => CheckFailure::Unreachable,
        MailboxError::Storage("certificate_invalid") => CheckFailure::CertificateInvalid,
        MailboxError::Storage("tls_failed") => CheckFailure::TlsFailed,
        MailboxError::Storage("invalid_endpoint") => CheckFailure::InvalidEndpoint,
        MailboxError::InconsistentData(_) => CheckFailure::NotImap,
        _ => CheckFailure::Storage,
    }
}

fn two(n: i64) -> String {
    format!("{n:02}")
}

/// (short label, full label) for a UTC timestamp. Time zones arrive with localisation (MB-105).
pub fn format_time(epoch: i64, now: i64) -> (String, String) {
    if epoch <= 0 {
        return (String::new(), String::new());
    }
    let (days, secs) = (epoch.div_euclid(86_400), epoch.rem_euclid(86_400));
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (day, month) = (
        doy - (153 * mp + 2) / 5 + 1,
        if mp < 10 { mp + 3 } else { mp - 9 },
    );
    let year = yoe + era * 400 + i64::from(month <= 2);
    let clock = format!("{}:{}", two(secs / 3600), two((secs % 3600) / 60));
    let short = if now.div_euclid(86_400) == days {
        clock.clone()
    } else {
        format!("{}/{}", two(day), two(month))
    };
    (
        short,
        format!("{}/{}/{year} {clock} UTC", two(day), two(month)),
    )
}

/// Build the UI snapshot from the local store: envelopes, plus the text of bodies already cached.
#[cfg_attr(not(test), allow(dead_code))]
pub async fn snapshot(
    store: &(impl MailStore + mailbox_core::body::BodyStore + mailbox_core::threading::ThreadStore),
    now: i64,
) -> Result<BootstrapResponse, MailboxError> {
    snapshot_hiding(store, now, &std::collections::BTreeSet::new()).await
}

/// Snapshot of every synced folder. `MessageSummary::folder_id` is the folder's role key
/// (`inbox`, `sent`, ...) or, for the user's own folders, the folder id. Messages in `hidden`
/// (on their way out of their folder) are left out.
pub async fn snapshot_hiding(
    store: &(impl MailStore + mailbox_core::body::BodyStore + mailbox_core::threading::ThreadStore),
    now: i64,
    hidden: &std::collections::BTreeSet<String>,
) -> Result<BootstrapResponse, MailboxError> {
    // Conversations for the list (MB-201); the snapshot rows themselves stay one per message.
    super::history::remember_threads(store).await?;
    let mut accounts = Vec::new();
    let mut messages = Vec::new();
    for (i, a) in store.list_accounts(OWNER).await?.into_iter().enumerate() {
        let folders = store.list_folders(OWNER, &a.id).await?;
        let mut unread = 0;
        for folder in folders.iter() {
            let view = super::actions::view_key(folder);
            for m in store
                .list_messages(OWNER, &a.id, &folder.id, super::history::depth(&folder.id))
                .await?
            {
                if hidden.contains(&m.id) {
                    continue;
                }
                // The account badge counts the inbox only, like the unified view.
                unread += u32::from(!m.seen && view == "inbox");
                let (short, full) = format_time(m.internal_date, now);
                // Text of an already downloaded body, so a reply can quote it. The reader itself
                // loads bodies through `adapters::body`.
                let cached_text = store
                    .load_body(OWNER, &m.id)
                    .await
                    .ok()
                    .flatten()
                    .map(|b| b.text)
                    .unwrap_or_default();
                messages.push((
                    m.internal_date,
                    MessageSummary {
                        id: m.id,
                        account_id: a.id.clone(),
                        folder_id: view.clone(),
                        from_name: if m.from_display.is_empty() {
                            m.from_address.clone()
                        } else {
                            m.from_display
                        },
                        from_address: m.from_address,
                        subject: if m.subject.is_empty() {
                            tr("(Không có tiêu đề)").into()
                        } else {
                            m.subject
                        },
                        preview: String::new(),
                        body_plain: cached_text,
                        received_at: short,
                        unread: !m.seen,
                        starred: m.flagged,
                        attachments: Vec::new(),
                        thread_id: String::new(),
                        to_line: a.email_address.clone(),
                        received_at_full: full,
                        labels: Vec::new(),
                    },
                ));
            }
        }
        accounts.push(AccountSummary {
            id: a.id,
            name: a.display_name,
            address: a.email_address,
            // Stored accounts passed a real IMAP login; "active" is the only state saved so far.
            status: if a.status == "active" {
                AccountStatus::Connected
            } else {
                AccountStatus::NeedsAttention
            },
            unread,
            color: COLORS[i % COLORS.len()].into(),
            last_synced_at: None,
        });
    }
    messages.sort_by_key(|m| std::cmp::Reverse(m.0));
    Ok(BootstrapResponse {
        mode: "real".into(),
        accounts,
        messages: messages.into_iter().map(|(_, m)| m).collect(),
    })
}

async fn sync_account(
    store: &SqliteStore,
    account: &AccountRecord,
    password: &Secret,
) -> Result<SyncSummary, MailboxError> {
    let endpoint = MailServerEndpoint {
        hostname: account.imap_host.clone(),
        port: account.imap_port,
        tls_mode: TlsMode::ImplicitTls,
    };
    // One connection at a time per device: a sync never interleaves with a queued action.
    let _gate = super::actions::gate().await;
    let mut rw =
        mailbox_imap::mutate::MutableSession::connect(&endpoint, &account.email_address, password)
            .await?;
    // Local changes first, so the flag scan below sees them confirmed and the server wins after.
    let replayed = replay(store, &mut rw, OWNER, &account.id).await;
    let mut session = rw.into_reader();
    let report = match replayed {
        Ok(replayed) => {
            super::actions::remember(&replayed);
            sync_all_folders(store, &mut session, OWNER, &account.id, WINDOW, MAX_FOLDERS).await
        }
        Err(e) => Err(e),
    };
    if let Ok(r) = &report {
        // Reply headers, conversations and the search index (MB-200/201), best effort.
        super::history::after_sync(store, &mut session, account, r.stored()).await;
    }
    session.logout().await;
    drop(_gate);
    super::history::index_after_sync(store).await;
    let report = report?;
    let summary = SyncSummary {
        folders: report.folders.len() + report.skipped,
        new_messages: report.stored(),
        failed_folders: report.failed(),
    };
    // The inbox is the one folder an account cannot do without.
    for folder in report.folders {
        if folder.role == Some(FolderRole::Inbox) {
            folder.result?;
        }
    }
    Ok(summary)
}

/// Save a verified account: password to the OS keychain, metadata to SQLite, then first sync.
/// If the first sync fails the account and its secret are removed again (no half-saved state).
pub async fn save_and_sync(new: NewAccount, password: String) -> Result<SyncSummary, CheckFailure> {
    let password = Secret::new(password);
    let id = account_id(&new.email);
    let record = AccountRecord {
        id: id.clone(),
        owner_id: OWNER.into(),
        display_name: new.email.split('@').nth(1).unwrap_or("Email").to_string(),
        email_address: new.email.trim().to_string(),
        imap_host: new.imap_host.trim().to_string(),
        imap_port: new.imap_port,
        smtp_host: new.smtp_host.trim().to_string(),
        smtp_port: new.smtp_port,
        auth_type: "password".into(),
        credential_ref: secret_ref(&id),
        status: "active".into(),
    };
    let store = open_store().await.map_err(failure)?;
    let keychain = KeychainStore::default();
    keychain
        .put(&record.credential_ref, &password)
        .map_err(|_| CheckFailure::Storage)?;
    store.upsert_account(&record).await.map_err(failure)?;
    match sync_account(&store, &record, &password).await {
        Ok(summary) => Ok(summary),
        Err(e) => {
            let _ = store.delete_account(OWNER, &id).await;
            let _ = keychain.delete(&record.credential_ref);
            Err(failure(e))
        }
    }
}

/// Sync every stored account using its keychain secret. Returns the number of new messages.
pub async fn resync_all() -> Result<usize, CheckFailure> {
    let store = open_store().await.map_err(failure)?;
    let keychain = KeychainStore::default();
    let mut total = 0;
    for account in store.list_accounts(OWNER).await.map_err(failure)? {
        // A missing keychain entry means the user removed it: the account needs re-authentication.
        let password = keychain.get(&account.credential_ref).map_err(failure)?;
        let before = super::notify::snapshot(&store, &account.id).await;
        total += sync_account(&store, &account, &password)
            .await
            .map_err(failure)?
            .new_messages;
        // MB-211: one batched, policy-checked notification for what just arrived.
        super::notify::after_sync(&store, &account.id, &before).await;
    }
    Ok(total)
}

/// Remove an account from this device: local cache and keychain entry. The server is untouched.
pub async fn remove_account(account_id: &str) -> Result<(), CheckFailure> {
    let store = open_store().await.map_err(failure)?;
    let reference = store
        .get_account(OWNER, account_id)
        .await
        .map_err(failure)?
        .credential_ref;
    store
        .delete_account(OWNER, account_id)
        .await
        .map_err(failure)?;
    // A separate SMTP password exists only when the server refused the account password; a
    // missing entry is the normal case, so this is best effort.
    let _ = super::send::forget_smtp_password(account_id);
    KeychainStore::default()
        .delete(&reference)
        .map_err(|_| CheckFailure::Storage)
}

/// Snapshot of the real local store for the UI.
pub async fn load_real() -> Result<BootstrapResponse, DataError> {
    let store = open_store().await.map_err(|_| DataError::Unavailable)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (_, hidden) = super::actions::op_view(&store)
        .await
        .map_err(|_| DataError::BadResponse)?;
    snapshot_hiding(&store, now, &hidden)
        .await
        .map_err(|_| DataError::BadResponse)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailbox_core::store::{FolderRecord, MessageRecord};

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn account_ids_are_safe_and_stable() {
        assert_eq!(
            account_id(" No-Reply@Example.org "),
            "acc-no-reply-example-org"
        );
        assert!(mailbox_core::secrets::is_valid_reference(&secret_ref(
            &account_id("a/b c@x.y")
        )));
    }

    #[test]
    fn times_are_short_today_and_dated_otherwise() {
        let now = 1_794_625_500; // 14/11/2026 03:05 UTC
        assert_eq!(
            format_time(now - 60, now),
            ("03:04".into(), "14/11/2026 03:04 UTC".into())
        );
        assert_eq!(format_time(now - 86_400 * 2, now).0, "12/11");
        assert_eq!(format_time(0, now), (String::new(), String::new()));
    }

    #[test]
    fn snapshot_shows_real_envelopes_without_secrets_or_demo_data() {
        rt().block_on(async {
            let store = SqliteStore::in_memory().await.unwrap();
            let acc = AccountRecord {
                id: "acc-t".into(),
                owner_id: OWNER.into(),
                display_name: "example.com".into(),
                email_address: "t@example.com".into(),
                imap_host: "imap.example.com".into(),
                imap_port: 993,
                smtp_host: "smtp.example.com".into(),
                smtp_port: 465,
                auth_type: "password".into(),
                credential_ref: "imap.acc-t".into(),
                status: "active".into(),
            };
            store.upsert_account(&acc).await.unwrap();
            let inbox = FolderRecord {
                id: "acc-t/INBOX".into(),
                account_id: "acc-t".into(),
                remote_name: "INBOX".into(),
                delimiter: None,
                special_use: Some("inbox".into()),
                uid_validity: Some(1),
                uid_next: None,
            };
            store.upsert_folder(OWNER, &inbox).await.unwrap();
            let msg = |uid: u32, date: i64, seen: bool, subject: &str| MessageRecord {
                id: format!("m{uid}"),
                account_id: "acc-t".into(),
                folder_id: inbox.id.clone(),
                uid_validity: 1,
                imap_uid: uid,
                message_id_header: None,
                subject: subject.into(),
                from_display: String::new(),
                from_address: "a@example.org".into(),
                preview: String::new(),
                internal_date: date,
                size_bytes: 1,
                seen,
                flagged: uid == 2,
            };
            store
                .upsert_messages(
                    OWNER,
                    &[
                        msg(1, 100, true, "Cũ"),
                        msg(2, 300, false, ""),
                        msg(3, 200, false, "Giữa"),
                    ],
                )
                .await
                .unwrap();

            let snap = snapshot(&store, 400).await.unwrap();
            assert_eq!(snap.mode, "real");
            assert_eq!(snap.accounts.len(), 1);
            assert_eq!(
                (snap.accounts[0].status.clone(), snap.accounts[0].unread),
                (AccountStatus::Connected, 2)
            );
            assert_eq!(
                snap.messages
                    .iter()
                    .map(|m| m.id.as_str())
                    .collect::<Vec<_>>(),
                vec!["m2", "m3", "m1"],
                "newest first"
            );
            assert_eq!(snap.messages[0].subject, "(Không có tiêu đề)");
            assert_eq!(snap.messages[0].from_name, "a@example.org");
            assert!(snap.messages[0].starred && snap.messages[0].unread);
            let dump = format!("{snap:?}");
            assert!(
                !dump.contains("imap.acc-t"),
                "credential reference must not reach the UI"
            );
            assert!(!dump.contains("Trần Linh"), "no demo data");
        });
    }
}
