//! SQL implementations of [`mailbox_core::store::MailStore`]: SQLite for Desktop, PostgreSQL for Web.
//!
//! One query set serves both dialects (portable column types, `$n` placeholders). Each dialect has
//! its own migration directory. Errors are mapped to short static codes: SQL text, parameters and
//! row contents never leave this crate, so they cannot end up in logs.
mod actions;
mod body;
mod outbox;
mod prefs;
mod search;

use mailbox_core::store::{AccountRecord, FolderRecord, MailStore, MessageRecord};
use mailbox_core::{ImapCursor, MailboxError};
use sqlx::Row;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn storage(code: &'static str) -> impl Fn(sqlx::Error) -> MailboxError {
    move |_| MailboxError::Storage(code)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn port(value: i64) -> Result<u16, MailboxError> {
    u16::try_from(value).map_err(|_| MailboxError::InconsistentData("port out of range"))
}

/// Desktop store. The database file holds metadata only; credentials stay in the OS keychain.
#[derive(Debug, Clone)]
pub struct SqliteStore {
    pool: SqlitePool,
    /// The SQLite library has FTS5 and the search index uses it (MB-200).
    fts: bool,
}

/// The database holds headers, cached bodies, drafts and the search index: owner-only access
/// (0600) for the file and for SQLite's side files, whatever the process umask (MB-304). A new
/// file is created with that mode before SQLite opens it; SQLite gives its journal / WAL files
/// the mode of the main file. Existing files from older versions are tightened.
#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> Result<(), MailboxError> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(MailboxError::Storage("open")),
    }
    let mut targets = vec![path.to_path_buf()];
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        targets.push(name.into());
    }
    for p in targets {
        if p.exists() {
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| MailboxError::Storage("open"))?;
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) -> Result<(), MailboxError> {
    // Windows: the per-user %APPDATA% ACL applies; no POSIX mode to set.
    Ok(())
}

impl SqliteStore {
    /// Open (creating if needed) a database file and apply migrations.
    pub async fn open(path: &Path) -> Result<Self, MailboxError> {
        restrict_to_owner(path)?;
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .map_err(storage("open"))?;
        Self::migrate(pool).await
    }

    /// Private in-memory database, for tests.
    pub async fn in_memory() -> Result<Self, MailboxError> {
        let options = SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(true);
        // A single connection: every new in-memory connection would be a separate empty database.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(storage("open"))?;
        Self::migrate(pool).await
    }

    async fn migrate(pool: SqlitePool) -> Result<Self, MailboxError> {
        sqlx::migrate!("../../migrations/sqlite")
            .run(&pool)
            .await
            .map_err(|_| MailboxError::Storage("migrate"))?;
        let fts = Self::prepare_fts(&pool).await;
        Ok(Self { pool, fts })
    }
}

/// Web store. Callers must pass the authenticated owner id to every method.
#[derive(Debug, Clone)]
pub struct PgStore {
    pool: PgPool,
}

impl PgStore {
    /// Connect and apply migrations. The URL may contain a password: it is never logged or echoed.
    pub async fn connect(database_url: &str) -> Result<Self, MailboxError> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .map_err(storage("connect"))?;
        sqlx::migrate!("../../migrations/postgres")
            .run(&pool)
            .await
            .map_err(|_| MailboxError::Storage("migrate"))?;
        Ok(Self { pool })
    }
}

macro_rules! impl_mail_store {
    ($store:ty) => {
        impl $store {
            /// An account owned by someone else is indistinguishable from a missing one.
            async fn require_account(&self, owner_id: &str, account_id: &str) -> Result<(), MailboxError> {
                let found = sqlx::query("SELECT 1 FROM accounts WHERE id = $1 AND owner_id = $2")
                    .bind(account_id)
                    .bind(owner_id)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(storage("account_lookup"))?;
                found.map(|_| ()).ok_or(MailboxError::NotFound)
            }
        }

        impl MailStore for $store {
            async fn upsert_account(&self, a: &AccountRecord) -> Result<(), MailboxError> {
                let ts = now();
                // The owner filter on UPDATE stops one owner from taking over another owner's id.
                let result = sqlx::query(
                    "INSERT INTO accounts (id, owner_id, display_name, email_address, imap_host, imap_port, \
                     smtp_host, smtp_port, auth_type, credential_ref, status, created_at, updated_at) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$12) \
                     ON CONFLICT (id) DO UPDATE SET display_name = excluded.display_name, \
                     email_address = excluded.email_address, imap_host = excluded.imap_host, \
                     imap_port = excluded.imap_port, smtp_host = excluded.smtp_host, \
                     smtp_port = excluded.smtp_port, auth_type = excluded.auth_type, \
                     credential_ref = excluded.credential_ref, status = excluded.status, \
                     updated_at = excluded.updated_at \
                     WHERE accounts.owner_id = excluded.owner_id",
                )
                .bind(&a.id)
                .bind(&a.owner_id)
                .bind(&a.display_name)
                .bind(&a.email_address)
                .bind(&a.imap_host)
                .bind(i64::from(a.imap_port))
                .bind(&a.smtp_host)
                .bind(i64::from(a.smtp_port))
                .bind(&a.auth_type)
                .bind(&a.credential_ref)
                .bind(&a.status)
                .bind(ts)
                .execute(&self.pool)
                .await
                .map_err(storage("account_write"))?;
                if result.rows_affected() == 0 {
                    return Err(MailboxError::NotFound);
                }
                Ok(())
            }

            async fn list_accounts(&self, owner_id: &str) -> Result<Vec<AccountRecord>, MailboxError> {
                let rows = sqlx::query(
                    "SELECT id, owner_id, display_name, email_address, imap_host, imap_port, smtp_host, \
                     smtp_port, auth_type, credential_ref, status FROM accounts WHERE owner_id = $1 \
                     ORDER BY created_at, id",
                )
                .bind(owner_id)
                .fetch_all(&self.pool)
                .await
                .map_err(storage("account_read"))?;
                rows.iter()
                    .map(|r| {
                        Ok(AccountRecord {
                            id: r.try_get("id").map_err(storage("decode"))?,
                            owner_id: r.try_get("owner_id").map_err(storage("decode"))?,
                            display_name: r.try_get("display_name").map_err(storage("decode"))?,
                            email_address: r.try_get("email_address").map_err(storage("decode"))?,
                            imap_host: r.try_get("imap_host").map_err(storage("decode"))?,
                            imap_port: port(r.try_get("imap_port").map_err(storage("decode"))?)?,
                            smtp_host: r.try_get("smtp_host").map_err(storage("decode"))?,
                            smtp_port: port(r.try_get("smtp_port").map_err(storage("decode"))?)?,
                            auth_type: r.try_get("auth_type").map_err(storage("decode"))?,
                            credential_ref: r.try_get("credential_ref").map_err(storage("decode"))?,
                            status: r.try_get("status").map_err(storage("decode"))?,
                        })
                    })
                    .collect()
            }

            async fn get_account(&self, owner_id: &str, account_id: &str) -> Result<AccountRecord, MailboxError> {
                self.list_accounts(owner_id)
                    .await?
                    .into_iter()
                    .find(|a| a.id == account_id)
                    .ok_or(MailboxError::NotFound)
            }

            async fn delete_account(&self, owner_id: &str, account_id: &str) -> Result<(), MailboxError> {
                let result = sqlx::query("DELETE FROM accounts WHERE id = $1 AND owner_id = $2")
                    .bind(account_id)
                    .bind(owner_id)
                    .execute(&self.pool)
                    .await
                    .map_err(storage("account_delete"))?;
                if result.rows_affected() == 0 {
                    return Err(MailboxError::NotFound);
                }
                Ok(())
            }

            async fn upsert_folder(&self, owner_id: &str, f: &FolderRecord) -> Result<(), MailboxError> {
                self.require_account(owner_id, &f.account_id).await?;
                sqlx::query(
                    "INSERT INTO folders (id, account_id, remote_name, delimiter, special_use, uidvalidity, uidnext) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7) \
                     ON CONFLICT (account_id, remote_name) DO UPDATE SET delimiter = excluded.delimiter, \
                     special_use = excluded.special_use, uidvalidity = excluded.uidvalidity, \
                     uidnext = excluded.uidnext",
                )
                .bind(&f.id)
                .bind(&f.account_id)
                .bind(&f.remote_name)
                .bind(&f.delimiter)
                .bind(&f.special_use)
                .bind(f.uid_validity.map(|v| v as i64))
                .bind(f.uid_next.map(|v| v as i64))
                .execute(&self.pool)
                .await
                .map_err(storage("folder_write"))?;
                Ok(())
            }

            async fn list_folders(&self, owner_id: &str, account_id: &str) -> Result<Vec<FolderRecord>, MailboxError> {
                self.require_account(owner_id, account_id).await?;
                let rows = sqlx::query(
                    "SELECT id, account_id, remote_name, delimiter, special_use, uidvalidity, uidnext \
                     FROM folders WHERE account_id = $1 ORDER BY remote_name",
                )
                .bind(account_id)
                .fetch_all(&self.pool)
                .await
                .map_err(storage("folder_read"))?;
                rows.iter()
                    .map(|r| {
                        let uid_validity: Option<i64> = r.try_get("uidvalidity").map_err(storage("decode"))?;
                        let uid_next: Option<i64> = r.try_get("uidnext").map_err(storage("decode"))?;
                        Ok(FolderRecord {
                            id: r.try_get("id").map_err(storage("decode"))?,
                            account_id: r.try_get("account_id").map_err(storage("decode"))?,
                            remote_name: r.try_get("remote_name").map_err(storage("decode"))?,
                            delimiter: r.try_get("delimiter").map_err(storage("decode"))?,
                            special_use: r.try_get("special_use").map_err(storage("decode"))?,
                            uid_validity: uid_validity.map(|v| v as u64),
                            uid_next: uid_next.map(|v| v as u64),
                        })
                    })
                    .collect()
            }

            async fn upsert_messages(&self, owner_id: &str, messages: &[MessageRecord]) -> Result<(), MailboxError> {
                // All-or-nothing: a crash mid-batch must not leave a cursor ahead of stored rows.
                let mut tx = self.pool.begin().await.map_err(storage("tx_begin"))?;
                for m in messages {
                    let owned = sqlx::query(
                        "SELECT 1 FROM folders f JOIN accounts a ON a.id = f.account_id \
                         WHERE f.id = $1 AND f.account_id = $2 AND a.owner_id = $3",
                    )
                    .bind(&m.folder_id)
                    .bind(&m.account_id)
                    .bind(owner_id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(storage("message_scope"))?;
                    if owned.is_none() {
                        return Err(MailboxError::NotFound);
                    }
                    sqlx::query(
                        "INSERT INTO messages (id, account_id, folder_id, uidvalidity, imap_uid, message_id_header, \
                         subject, from_display, from_address, preview, internal_date, size_bytes, is_seen, is_flagged) \
                         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) \
                         ON CONFLICT (account_id, folder_id, uidvalidity, imap_uid) DO UPDATE SET \
                         message_id_header = excluded.message_id_header, subject = excluded.subject, \
                         from_display = excluded.from_display, from_address = excluded.from_address, \
                         preview = excluded.preview, internal_date = excluded.internal_date, \
                         size_bytes = excluded.size_bytes, is_seen = excluded.is_seen, \
                         is_flagged = excluded.is_flagged",
                    )
                    .bind(&m.id)
                    .bind(&m.account_id)
                    .bind(&m.folder_id)
                    .bind(m.uid_validity as i64)
                    .bind(i64::from(m.imap_uid))
                    .bind(&m.message_id_header)
                    .bind(&m.subject)
                    .bind(&m.from_display)
                    .bind(&m.from_address)
                    .bind(&m.preview)
                    .bind(m.internal_date)
                    .bind(m.size_bytes as i64)
                    .bind(i64::from(m.seen))
                    .bind(i64::from(m.flagged))
                    .execute(&mut *tx)
                    .await
                    .map_err(storage("message_write"))?;
                }
                tx.commit().await.map_err(storage("tx_commit"))
            }

            async fn list_messages(
                &self,
                owner_id: &str,
                account_id: &str,
                folder_id: &str,
                limit: u32,
            ) -> Result<Vec<MessageRecord>, MailboxError> {
                self.require_account(owner_id, account_id).await?;
                let rows = sqlx::query(
                    "SELECT id, account_id, folder_id, uidvalidity, imap_uid, message_id_header, subject, \
                     from_display, from_address, preview, internal_date, size_bytes, is_seen, is_flagged \
                     FROM messages WHERE account_id = $1 AND folder_id = $2 \
                     ORDER BY internal_date DESC, id LIMIT $3",
                )
                .bind(account_id)
                .bind(folder_id)
                .bind(i64::from(limit))
                .fetch_all(&self.pool)
                .await
                .map_err(storage("message_read"))?;
                rows.iter()
                    .map(|r| {
                        let uid_validity: i64 = r.try_get("uidvalidity").map_err(storage("decode"))?;
                        let imap_uid: i64 = r.try_get("imap_uid").map_err(storage("decode"))?;
                        let size: i64 = r.try_get("size_bytes").map_err(storage("decode"))?;
                        let seen: i64 = r.try_get("is_seen").map_err(storage("decode"))?;
                        let flagged: i64 = r.try_get("is_flagged").map_err(storage("decode"))?;
                        Ok(MessageRecord {
                            id: r.try_get("id").map_err(storage("decode"))?,
                            account_id: r.try_get("account_id").map_err(storage("decode"))?,
                            folder_id: r.try_get("folder_id").map_err(storage("decode"))?,
                            uid_validity: uid_validity as u64,
                            imap_uid: u32::try_from(imap_uid)
                                .map_err(|_| MailboxError::InconsistentData("uid out of range"))?,
                            message_id_header: r.try_get("message_id_header").map_err(storage("decode"))?,
                            subject: r.try_get("subject").map_err(storage("decode"))?,
                            from_display: r.try_get("from_display").map_err(storage("decode"))?,
                            from_address: r.try_get("from_address").map_err(storage("decode"))?,
                            preview: r.try_get("preview").map_err(storage("decode"))?,
                            internal_date: r.try_get("internal_date").map_err(storage("decode"))?,
                            size_bytes: size as u64,
                            seen: seen != 0,
                            flagged: flagged != 0,
                        })
                    })
                    .collect()
            }

            async fn reset_folder(&self, owner_id: &str, account_id: &str, folder_id: &str) -> Result<(), MailboxError> {
                self.require_account(owner_id, account_id).await?;
                let mut tx = self.pool.begin().await.map_err(storage("tx_begin"))?;
                sqlx::query("DELETE FROM messages WHERE account_id = $1 AND folder_id = $2")
                    .bind(account_id)
                    .bind(folder_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(storage("folder_reset"))?;
                sqlx::query("DELETE FROM sync_states WHERE account_id = $1 AND folder_id = $2")
                    .bind(account_id)
                    .bind(folder_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(storage("folder_reset"))?;
                tx.commit().await.map_err(storage("tx_commit"))
            }

            async fn store_headers(
                &self,
                owner_id: &str,
                rows: &[mailbox_core::threading::HeaderRow],
            ) -> Result<(), MailboxError> {
                self.put_headers(owner_id, rows).await
            }

            async fn save_cursor(&self, owner_id: &str, c: &ImapCursor) -> Result<(), MailboxError> {
                self.require_account(owner_id, &c.account_id).await?;
                sqlx::query(
                    "INSERT INTO sync_states (account_id, folder_id, uidvalidity, last_uid, state, last_success_at) \
                     VALUES ($1,$2,$3,$4,'synced',$5) \
                     ON CONFLICT (account_id, folder_id) DO UPDATE SET uidvalidity = excluded.uidvalidity, \
                     last_uid = excluded.last_uid, state = excluded.state, \
                     last_success_at = excluded.last_success_at",
                )
                .bind(&c.account_id)
                .bind(&c.folder_id)
                .bind(c.uid_validity as i64)
                .bind(i64::from(c.last_uid))
                .bind(now())
                .execute(&self.pool)
                .await
                .map_err(storage("cursor_write"))?;
                Ok(())
            }

            async fn load_cursor(
                &self,
                owner_id: &str,
                account_id: &str,
                folder_id: &str,
            ) -> Result<Option<ImapCursor>, MailboxError> {
                self.require_account(owner_id, account_id).await?;
                let row = sqlx::query(
                    "SELECT uidvalidity, last_uid FROM sync_states WHERE account_id = $1 AND folder_id = $2",
                )
                .bind(account_id)
                .bind(folder_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(storage("cursor_read"))?;
                let Some(r) = row else { return Ok(None) };
                let uid_validity: Option<i64> = r.try_get("uidvalidity").map_err(storage("decode"))?;
                let last_uid: i64 = r.try_get("last_uid").map_err(storage("decode"))?;
                let Some(uid_validity) = uid_validity else { return Ok(None) };
                Ok(Some(ImapCursor {
                    account_id: account_id.to_string(),
                    folder_id: folder_id.to_string(),
                    uid_validity: uid_validity as u64,
                    last_uid: u32::try_from(last_uid)
                        .map_err(|_| MailboxError::InconsistentData("uid out of range"))?,
                }))
            }
        }
    };
}

impl_mail_store!(SqliteStore);
impl_mail_store!(PgStore);
