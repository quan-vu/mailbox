//! Desktop notifications for new mail (MB-211). `notify_new_mail` is the one entry point: the
//! manual sync path calls it today and the live sync worker calls it for mail it finds.
//!
//! Policy lives in `mailbox_core::notify` (count-only by default, sender + subject on opt-in,
//! never the body, batched, silent for Sent/Drafts/Trash/Junk and for the folder in view). This
//! file reads the stored choice, knows what is on screen, and talks to the operating system.
use super::account::{OWNER, open_store};
use mailbox_core::folders::{FolderRole, FolderRoleStore};
use mailbox_core::notify::{self, NewMail, Notice, Notifier, NotifyError, Viewing};
use mailbox_core::prefs::{Lang, PreferencesStore};
use mailbox_core::store::MailStore;
use std::collections::BTreeSet;
use std::sync::Mutex;

static VIEWING: Mutex<Viewing> = Mutex::new(Viewing {
    window_focused: false,
    folder_key: None,
    account_id: None,
});

/// Called by the UI whenever the visible list or the window focus changes.
pub fn set_viewing(v: Viewing) {
    if let Ok(mut g) = VIEWING.lock() {
        *g = v;
    }
}

fn viewing() -> Viewing {
    VIEWING.lock().map(|g| g.clone()).unwrap_or_default()
}

/// The real notifier. On macOS an unsigned or unbundled app may be refused by the system; that
/// shows up as `Unavailable` and is reported, never retried in a loop.
pub struct OsNotifier;

impl Notifier for OsNotifier {
    fn show(&self, notice: &Notice, sound: bool) -> Result<(), NotifyError> {
        let mut n = notify_rust::Notification::new();
        n.appname("MailBox")
            .summary(&notice.title)
            .body(&notice.body);
        if sound {
            n.sound_name("default");
        }
        n.show().map(|_| ()).map_err(|_| NotifyError::Unavailable)
    }
}

/// Trigger: `new` are messages that just arrived for `account_id`, newest first.
/// Returns whether a notification was shown. Reads the stored notification choice itself, so any
/// caller (manual sync, live worker) gets the same behaviour.
pub async fn notify_new_mail(account_id: &str, new: &[NewMail]) -> bool {
    notify_with(&OsNotifier, account_id, new).await
}

/// The text of a notice in the stored interface language. The policy in `mailbox_core::notify`
/// writes Vietnamese; only its fixed phrases change, the sender and subject never do. Reads no
/// UI state, so it is right on any thread.
pub fn localize_notice(notice: &Notice, lang: Lang) -> Notice {
    if lang == Lang::Vi {
        return notice.clone();
    }
    let text = |s: &str| -> String {
        if s == "Có 1 thư mới" {
            return "1 new message".into();
        }
        if let Some(n) = s
            .strip_prefix("Có ")
            .and_then(|r| r.strip_suffix(" thư mới"))
            .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        {
            return format!("{n} new messages");
        }
        if let Some(rest) = s.strip_prefix("Mới nhất từ ") {
            return format!("Latest from {rest}");
        }
        s.to_string()
    };
    Notice {
        title: text(&notice.title),
        body: text(&notice.body),
        count: notice.count,
    }
}

/// Shows notices through `inner` in one language.
struct Localized<'a, N: Notifier> {
    inner: &'a N,
    lang: Lang,
}

impl<N: Notifier> Notifier for Localized<'_, N> {
    fn show(&self, notice: &Notice, sound: bool) -> Result<(), NotifyError> {
        self.inner.show(&localize_notice(notice, self.lang), sound)
    }
}

pub async fn notify_with(notifier: &impl Notifier, account_id: &str, new: &[NewMail]) -> bool {
    if new.is_empty() {
        return false;
    }
    let Ok(store) = open_store().await else {
        return false;
    };
    let Ok(prefs) = store.load_preferences(OWNER).await else {
        return false;
    };
    let notifier = Localized {
        inner: notifier,
        lang: prefs.lang,
    };
    notify::deliver(
        &notifier,
        prefs.notify_mode,
        prefs.notify_sound,
        account_id,
        new,
        &viewing(),
    )
}

/// Ids of every cached message of an account, taken before a sync.
pub async fn snapshot(store: &impl MailStore, account_id: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    if let Ok(folders) = store.list_folders(OWNER, account_id).await {
        for f in folders {
            if let Ok(rows) = store.list_messages(OWNER, account_id, &f.id, 500).await {
                ids.extend(rows.into_iter().map(|m| m.id));
            }
        }
    }
    ids
}

/// Messages that appeared since `before` and are still unread, newest first. Mail older than
/// two days is skipped, so a folder synced for the first time does not announce its history.
pub async fn new_since(
    store: &(impl MailStore + FolderRoleStore),
    account_id: &str,
    before: &BTreeSet<String>,
    now: i64,
) -> Vec<NewMail> {
    let Ok(folders) = store.list_folders(OWNER, account_id).await else {
        return Vec::new();
    };
    let roles = store
        .list_folder_roles(OWNER, account_id)
        .await
        .unwrap_or_default();
    let mut out: Vec<(i64, NewMail)> = Vec::new();
    for f in folders {
        let role = roles
            .iter()
            .find(|r| r.folder_id == f.id)
            .and_then(|r| r.role);
        let key = role.map_or_else(|| f.id.clone(), |r: FolderRole| r.key().to_string());
        let Ok(rows) = store.list_messages(OWNER, account_id, &f.id, 500).await else {
            continue;
        };
        for m in rows {
            if before.contains(&m.id) || m.seen || m.internal_date < now - 2 * 86_400 {
                continue;
            }
            out.push((
                m.internal_date,
                NewMail {
                    role,
                    folder_key: key.clone(),
                    from_display: m.from_display,
                    from_address: m.from_address,
                    subject: m.subject,
                },
            ));
        }
    }
    out.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
    out.into_iter().map(|(_, m)| m).collect()
}

/// After a sync of one account: announce what is new.
pub async fn after_sync(
    store: &(impl MailStore + FolderRoleStore),
    account_id: &str,
    before: &BTreeSet<String>,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let new = new_since(store, account_id, before, now).await;
    notify_new_mail(account_id, &new).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailbox_core::folders::{FolderRoleRecord, RoleSource};
    use mailbox_core::store::{AccountRecord, FolderRecord, MessageRecord};
    use mailbox_store::SqliteStore;

    const NOW: i64 = 1_794_000_000;

    fn notice(title: &str, body: &str) -> Notice {
        Notice {
            title: title.into(),
            body: body.into(),
            count: 1,
        }
    }

    #[test]
    fn notices_keep_vietnamese_and_translate_only_the_fixed_phrases_in_english() {
        let one = notice("MailBox", "Có 1 thư mới");
        assert_eq!(localize_notice(&one, Lang::Vi), one);
        assert_eq!(localize_notice(&one, Lang::En).body, "1 new message");
        let many = notice("MailBox", "Có 5 thư mới");
        assert_eq!(localize_notice(&many, Lang::En).body, "5 new messages");
        let detail = notice("Có 3 thư mới", "Mới nhất từ Linh: Báo giá");
        let en = localize_notice(&detail, Lang::En);
        assert_eq!(en.title, "3 new messages");
        assert_eq!(en.body, "Latest from Linh: Báo giá");
        // A sender or subject that looks like a phrase is shown as it is.
        let single = notice("Có 9 thư mới", "Có 1 thư mới");
        assert_eq!(localize_notice(&single, Lang::En).title, "9 new messages");
        let plain = notice("Linh", "Báo giá tháng 11");
        assert_eq!(localize_notice(&plain, Lang::En), plain);
    }

    fn msg(id: &str, folder: &str, uid: u32, date: i64, seen: bool) -> MessageRecord {
        MessageRecord {
            id: id.into(),
            account_id: "acc".into(),
            folder_id: folder.into(),
            uid_validity: 1,
            imap_uid: uid,
            message_id_header: None,
            subject: format!("subject {id}"),
            from_display: "Linh".into(),
            from_address: "linh@example.com".into(),
            preview: String::new(),
            internal_date: date,
            size_bytes: 1,
            seen,
            flagged: false,
        }
    }

    async fn store() -> SqliteStore {
        let s = SqliteStore::in_memory().await.unwrap();
        s.upsert_account(&AccountRecord {
            id: "acc".into(),
            owner_id: OWNER.into(),
            display_name: "x".into(),
            email_address: "a@example.com".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 465,
            auth_type: "password".into(),
            credential_ref: "imap.acc".into(),
            status: "active".into(),
        })
        .await
        .unwrap();
        for (id, name, role) in [
            ("f-inbox", "INBOX", FolderRole::Inbox),
            ("f-sent", "Sent", FolderRole::Sent),
        ] {
            s.upsert_folder(
                OWNER,
                &FolderRecord {
                    id: id.into(),
                    account_id: "acc".into(),
                    remote_name: name.into(),
                    delimiter: None,
                    special_use: None,
                    uid_validity: Some(1),
                    uid_next: Some(1),
                },
            )
            .await
            .unwrap();
            s.save_folder_role(
                OWNER,
                &FolderRoleRecord {
                    folder_id: id.into(),
                    account_id: "acc".into(),
                    role: Some(role),
                    source: RoleSource::Name,
                },
            )
            .await
            .unwrap();
        }
        s
    }

    #[tokio::test]
    async fn only_unread_recent_new_mail_is_reported_newest_first_with_its_role() {
        let s = store().await;
        s.upsert_messages(OWNER, &[msg("old", "f-inbox", 1, NOW - 100, false)])
            .await
            .unwrap();
        let before = snapshot(&s, "acc").await;
        assert_eq!(before.len(), 1);
        s.upsert_messages(
            OWNER,
            &[
                msg("a", "f-inbox", 2, NOW - 50, false),
                msg("b", "f-inbox", 3, NOW - 10, false),
                msg("read", "f-inbox", 4, NOW - 5, true),
                msg("ancient", "f-inbox", 5, NOW - 10 * 86_400, false),
                msg("mine", "f-sent", 1, NOW - 1, false),
            ],
        )
        .await
        .unwrap();
        let new = new_since(&s, "acc", &before, NOW).await;
        let subjects: Vec<_> = new.iter().map(|m| m.subject.as_str()).collect();
        assert_eq!(subjects, ["subject mine", "subject b", "subject a"]);
        assert_eq!(new[0].role, Some(FolderRole::Sent));
        assert_eq!(new[1].folder_key, "inbox");
        // The policy then drops the Sent copy and batches the rest into one notice.
        let notice = notify::plan(
            mailbox_core::prefs::NotifyMode::CountOnly,
            false,
            "acc",
            &new,
            &Viewing::default(),
        )
        .unwrap();
        assert_eq!(notice.count, 2);
    }

    #[tokio::test]
    async fn nothing_new_gives_an_empty_list() {
        let s = store().await;
        s.upsert_messages(OWNER, &[msg("a", "f-inbox", 1, NOW, false)])
            .await
            .unwrap();
        let before = snapshot(&s, "acc").await;
        assert!(new_since(&s, "acc", &before, NOW).await.is_empty());
    }
}
