//! Folder roles (MB-122) and multi-folder sync (MB-143).
//!
//! A role says what a server folder is used for (inbox, sent, drafts, trash, archive, junk). It is
//! detected from the server's SPECIAL-USE attributes first, then from well-known folder names, and
//! is stored as data per folder so the user can override it. One account has at most one folder
//! per role.
use crate::MailboxError;
use crate::actions::{FlagPull, FlagSource, PendingOpStore, pull_flags};
use crate::store::{FolderRecord, MailStore};
use crate::sync::{RemoteMailbox, SyncReport, folder_id, sync_folder};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FolderRole {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Archive,
    Junk,
}

impl FolderRole {
    pub const ALL: [FolderRole; 6] = [
        FolderRole::Inbox,
        FolderRole::Sent,
        FolderRole::Drafts,
        FolderRole::Trash,
        FolderRole::Archive,
        FolderRole::Junk,
    ];

    pub fn key(self) -> &'static str {
        match self {
            FolderRole::Inbox => "inbox",
            FolderRole::Sent => "sent",
            FolderRole::Drafts => "drafts",
            FolderRole::Trash => "trash",
            FolderRole::Archive => "archive",
            FolderRole::Junk => "junk",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.key() == key)
    }
}

/// Where a folder's role came from. A `User` choice is never replaced by detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleSource {
    Attribute,
    Name,
    User,
}

impl RoleSource {
    pub fn key(self) -> &'static str {
        match self {
            RoleSource::Attribute => "attribute",
            RoleSource::Name => "name",
            RoleSource::User => "user",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        [RoleSource::Attribute, RoleSource::Name, RoleSource::User]
            .into_iter()
            .find(|s| s.key() == key)
    }
}

/// One folder as the server lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFolder {
    /// Name exactly as the server sent it; used in commands.
    pub name: String,
    /// Human-readable name (decoded), used for name matching and display.
    pub display_name: String,
    pub delimiter: Option<String>,
    /// LIST attributes such as `\Sent` or `\HasNoChildren`.
    pub attributes: Vec<String>,
    pub selectable: bool,
}

/// RFC 6154 SPECIAL-USE attributes plus the older XLIST spellings. `\All` is not a role: it is a
/// view of every message, not a place to move mail to.
pub fn role_from_attributes(attributes: &[String]) -> Option<FolderRole> {
    const MAP: [(&str, FolderRole); 7] = [
        ("\\Inbox", FolderRole::Inbox),
        ("\\Sent", FolderRole::Sent),
        ("\\Drafts", FolderRole::Drafts),
        ("\\Trash", FolderRole::Trash),
        ("\\Archive", FolderRole::Archive),
        ("\\Junk", FolderRole::Junk),
        ("\\Spam", FolderRole::Junk),
    ];
    MAP.iter()
        .find(|(flag, _)| attributes.iter().any(|a| a.eq_ignore_ascii_case(flag)))
        .map(|(_, role)| *role)
}

fn role_of_leaf(leaf: &str) -> Option<FolderRole> {
    const NAMES: [(FolderRole, &[&str]); 5] = [
        (
            FolderRole::Sent,
            &[
                "sent",
                "sent items",
                "sent mail",
                "sent messages",
                "sent-mail",
                "đã gửi",
                "thư đã gửi",
                "mục đã gửi",
                "da gui",
                "thu da gui",
            ],
        ),
        (
            FolderRole::Drafts,
            &[
                "drafts",
                "draft",
                "bản nháp",
                "thư nháp",
                "nháp",
                "ban nhap",
                "thu nhap",
            ],
        ),
        (
            FolderRole::Trash,
            &[
                "trash",
                "deleted items",
                "deleted messages",
                "deleted",
                "bin",
                "thùng rác",
                "thư đã xóa",
                "thư đã xoá",
                "đã xóa",
                "mục đã xóa",
                "thung rac",
            ],
        ),
        (
            FolderRole::Archive,
            &["archive", "archives", "lưu trữ", "thư lưu trữ", "luu tru"],
        ),
        (
            FolderRole::Junk,
            &[
                "junk",
                "spam",
                "junk e-mail",
                "junk email",
                "junk mail",
                "bulk mail",
                "thư rác",
                "thu rac",
            ],
        ),
    ];
    NAMES
        .iter()
        .find(|(_, names)| names.contains(&leaf))
        .map(|(role, _)| *role)
}

/// Role by well-known name. Accepts the folder at the top level or directly under `INBOX` or a
/// provider container such as `[Gmail]`; deeper folders are the user's own.
pub fn role_from_name(display_name: &str, delimiter: Option<&str>) -> Option<FolderRole> {
    let name = display_name.trim().to_lowercase();
    if name.is_empty() {
        return None;
    }
    if name == "inbox" {
        return Some(FolderRole::Inbox);
    }
    let mut delimiters: Vec<&str> = delimiter.into_iter().filter(|d| !d.is_empty()).collect();
    for common in ["/", "."] {
        if !delimiters.contains(&common) {
            delimiters.push(common);
        }
    }
    delimiters.into_iter().find_map(|d| {
        let parts: Vec<&str> = name.split(d).collect();
        match parts.as_slice() {
            [leaf] => role_of_leaf(leaf),
            [prefix, leaf] if *prefix == "inbox" || prefix.starts_with('[') => role_of_leaf(leaf),
            _ => None,
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleAssignment {
    pub name: String,
    pub role: Option<FolderRole>,
    pub source: RoleSource,
}

/// Detect roles for a folder listing: attributes first, names for roles still free. The first
/// folder in server order wins a role; the others keep none.
pub fn assign_roles(folders: &[ListedFolder]) -> Vec<RoleAssignment> {
    let mut taken = BTreeSet::new();
    let mut out: Vec<RoleAssignment> = folders
        .iter()
        .map(|f| {
            let role = role_from_attributes(&f.attributes).filter(|r| taken.insert(*r));
            RoleAssignment {
                name: f.name.clone(),
                role,
                source: if role.is_some() {
                    RoleSource::Attribute
                } else {
                    RoleSource::Name
                },
            }
        })
        .collect();
    for (f, a) in folders.iter().zip(out.iter_mut()) {
        if a.role.is_none() {
            a.role = role_from_name(&f.display_name, f.delimiter.as_deref())
                .filter(|r| taken.insert(*r));
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderRoleRecord {
    pub folder_id: String,
    pub account_id: String,
    /// `None` = explicitly no role.
    pub role: Option<FolderRole>,
    pub source: RoleSource,
}

/// Storage for folder roles. Scoped by owner like every store port.
#[allow(async_fn_in_trait)]
pub trait FolderRoleStore {
    async fn save_folder_role(
        &self,
        owner_id: &str,
        record: &FolderRoleRecord,
    ) -> Result<(), MailboxError>;
    async fn list_folder_roles(
        &self,
        owner_id: &str,
        account_id: &str,
    ) -> Result<Vec<FolderRoleRecord>, MailboxError>;
    /// Forget a folder that no longer exists on the server, with its cached messages. Local only.
    async fn remove_folder(
        &self,
        owner_id: &str,
        account_id: &str,
        folder_id: &str,
    ) -> Result<(), MailboxError>;
}

/// Folder listing with attributes, as needed for role detection.
#[allow(async_fn_in_trait)]
pub trait FolderSource {
    async fn list_detailed(&mut self) -> Result<Vec<ListedFolder>, MailboxError>;
}

/// Mirror the server's folders into the store with their roles. `FolderRecord::special_use`
/// carries the effective role key. Folders that disappeared from the server are forgotten.
pub async fn discover_with_roles<St>(
    store: &St,
    remote: &mut impl FolderSource,
    owner_id: &str,
    account_id: &str,
) -> Result<Vec<FolderRecord>, MailboxError>
where
    St: MailStore + FolderRoleStore,
{
    let listed: Vec<ListedFolder> = remote
        .list_detailed()
        .await?
        .into_iter()
        .filter(|f| f.selectable)
        .collect();
    // Every IMAP account has INBOX. A listing without it is a broken answer, and acting on it
    // would wipe the local folder list.
    if !listed.iter().any(|f| f.name.eq_ignore_ascii_case("INBOX")) {
        return Err(MailboxError::InconsistentData("folder list has no INBOX"));
    }
    let existing = store.list_folders(owner_id, account_id).await?;
    let user: BTreeMap<String, Option<FolderRole>> = store
        .list_folder_roles(owner_id, account_id)
        .await?
        .into_iter()
        .filter(|r| r.source == RoleSource::User)
        .map(|r| (r.folder_id, r.role))
        .collect();
    let claimed: BTreeSet<FolderRole> = user.values().flatten().copied().collect();

    let mut out = Vec::new();
    let mut live = BTreeSet::new();
    for (f, detected) in listed.iter().zip(assign_roles(&listed)) {
        let id = folder_id(account_id, &f.name);
        let role = match user.get(&id) {
            Some(chosen) => *chosen,
            None => detected.role.filter(|r| !claimed.contains(r)),
        };
        let previous = existing.iter().find(|e| e.id == id);
        let record = FolderRecord {
            id: id.clone(),
            account_id: account_id.to_string(),
            remote_name: f.name.clone(),
            delimiter: f.delimiter.clone(),
            special_use: role.map(|r| r.key().to_string()),
            // Discovery does not know these; keep what the last sync stored.
            uid_validity: previous.and_then(|p| p.uid_validity),
            uid_next: previous.and_then(|p| p.uid_next),
        };
        store.upsert_folder(owner_id, &record).await?;
        if !user.contains_key(&id) {
            store
                .save_folder_role(
                    owner_id,
                    &FolderRoleRecord {
                        folder_id: id.clone(),
                        account_id: account_id.to_string(),
                        role: detected.role,
                        source: detected.source,
                    },
                )
                .await?;
        }
        live.insert(id);
        out.push(record);
    }
    for gone in existing.iter().filter(|e| !live.contains(&e.id)) {
        store.remove_folder(owner_id, account_id, &gone.id).await?;
    }
    Ok(out)
}

/// Record the user's choice of role for a folder (`None` = no role). Takes the role away from any
/// other folder of the account.
pub async fn set_user_role<St>(
    store: &St,
    owner_id: &str,
    account_id: &str,
    target_folder_id: &str,
    role: Option<FolderRole>,
) -> Result<(), MailboxError>
where
    St: MailStore + FolderRoleStore,
{
    let folders = store.list_folders(owner_id, account_id).await?;
    if !folders.iter().any(|f| f.id == target_folder_id) {
        return Err(MailboxError::NotFound);
    }
    store
        .save_folder_role(
            owner_id,
            &FolderRoleRecord {
                folder_id: target_folder_id.to_string(),
                account_id: account_id.to_string(),
                role,
                source: RoleSource::User,
            },
        )
        .await?;
    let key = role.map(|r| r.key().to_string());
    for mut f in folders {
        let wanted = if f.id == target_folder_id {
            key.clone()
        } else if key.is_some() && f.special_use == key {
            None
        } else {
            continue;
        };
        if f.special_use != wanted {
            f.special_use = wanted;
            store.upsert_folder(owner_id, &f).await?;
        }
    }
    Ok(())
}

/// The effective role of a stored folder.
pub fn role_of(folder: &FolderRecord) -> Option<FolderRole> {
    folder.special_use.as_deref().and_then(FolderRole::from_key)
}

#[derive(Debug, PartialEq, Eq)]
pub struct FolderOutcome {
    pub folder_id: String,
    pub role: Option<FolderRole>,
    pub result: Result<SyncReport, MailboxError>,
    /// Flag reconciliation, when the folder synced.
    pub flags: Option<FlagPull>,
}

#[derive(Debug, PartialEq, Eq, Default)]
pub struct AccountSync {
    /// One entry per folder tried, role folders first.
    pub folders: Vec<FolderOutcome>,
    /// Folders left out because the account has more than the cap.
    pub skipped: usize,
}

impl AccountSync {
    pub fn stored(&self) -> usize {
        self.folders
            .iter()
            .filter_map(|f| f.result.as_ref().ok())
            .map(|r| r.stored)
            .sum()
    }

    pub fn failed(&self) -> usize {
        self.folders.iter().filter(|f| f.result.is_err()).count()
    }
}

/// Sync every selectable folder of an account: the newest `window` envelopes on first contact,
/// then only new mail, each folder with its own UIDVALIDITY cursor, followed by a flag
/// reconciliation. A folder that fails is reported and the others still sync.
pub async fn sync_all_folders<St, R>(
    store: &St,
    remote: &mut R,
    owner_id: &str,
    account_id: &str,
    window: u32,
    max_folders: usize,
) -> Result<AccountSync, MailboxError>
where
    St: MailStore + FolderRoleStore + PendingOpStore,
    R: RemoteMailbox + FolderSource + FlagSource,
{
    let mut folders = discover_with_roles(store, remote, owner_id, account_id).await?;
    folders.sort_by(|a, b| {
        let rank = |f: &FolderRecord| role_of(f).map_or(FolderRole::ALL.len(), |r| r as usize);
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.remote_name.cmp(&b.remote_name))
    });
    let skipped = folders.len().saturating_sub(max_folders);
    folders.truncate(max_folders);

    let mut report = AccountSync {
        folders: Vec::new(),
        skipped,
    };
    let mut offline = false;
    for folder in &folders {
        let mut flags = None;
        let result = if offline {
            Err(MailboxError::TransientConnectionFailure)
        } else {
            match sync_folder(store, remote, owner_id, folder, window).await {
                Ok(sync) => {
                    let pulled = match store.load_cursor(owner_id, account_id, &folder.id).await {
                        Ok(Some(cursor)) => {
                            pull_flags(store, remote, owner_id, folder, cursor.uid_validity).await
                        }
                        Ok(None) => Ok(FlagPull::default()),
                        Err(e) => Err(e),
                    };
                    pulled.map(|p| {
                        flags = Some(p);
                        sync
                    })
                }
                Err(e) => Err(e),
            }
        };
        // Once the connection is gone every further folder would only wait for a timeout.
        offline |= result == Err(MailboxError::TransientConnectionFailure);
        report.folders.push(FolderOutcome {
            folder_id: folder.id.clone(),
            role: role_of(folder),
            result,
            flags,
        });
    }
    Ok(report)
}
