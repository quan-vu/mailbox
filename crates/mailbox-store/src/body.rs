//! SQL implementation of [`mailbox_core::body::BodyStore`] (MB-130) for both dialects.
//! Same rules as the rest of the crate: one query set, owner-scoped, static error codes only.
use crate::{PgStore, SqliteStore, now, storage};
use mailbox_core::MailboxError;
use mailbox_core::body::{AttachmentMeta, BodyRecord, BodyStore, MessageLocator};
use sqlx::Row;

macro_rules! impl_body_store {
    ($store:ty) => {
        impl $store {
            /// A message owned by someone else is indistinguishable from a missing one.
            async fn require_message(&self, owner_id: &str, message_id: &str) -> Result<(), MailboxError> {
                let found = sqlx::query(
                    "SELECT 1 FROM messages m JOIN accounts a ON a.id = m.account_id \
                     WHERE m.id = $1 AND a.owner_id = $2",
                )
                .bind(message_id)
                .bind(owner_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(storage("body_scope"))?;
                found.map(|_| ()).ok_or(MailboxError::NotFound)
            }
        }

        impl BodyStore for $store {
            async fn locate_message(&self, owner_id: &str, message_id: &str) -> Result<MessageLocator, MailboxError> {
                let row = sqlx::query(
                    "SELECT m.account_id, f.remote_name, m.uidvalidity, m.imap_uid, m.size_bytes \
                     FROM messages m JOIN folders f ON f.id = m.folder_id \
                     JOIN accounts a ON a.id = m.account_id \
                     WHERE m.id = $1 AND a.owner_id = $2",
                )
                .bind(message_id)
                .bind(owner_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(storage("locate"))?
                .ok_or(MailboxError::NotFound)?;
                let uid_validity: i64 = row.try_get("uidvalidity").map_err(storage("decode"))?;
                let imap_uid: i64 = row.try_get("imap_uid").map_err(storage("decode"))?;
                let size: i64 = row.try_get("size_bytes").map_err(storage("decode"))?;
                Ok(MessageLocator {
                    account_id: row.try_get("account_id").map_err(storage("decode"))?,
                    folder_remote_name: row.try_get("remote_name").map_err(storage("decode"))?,
                    uid_validity: uid_validity as u64,
                    imap_uid: u32::try_from(imap_uid)
                        .map_err(|_| MailboxError::InconsistentData("uid out of range"))?,
                    size_bytes: size as u64,
                })
            }

            async fn save_body(&self, owner_id: &str, b: &BodyRecord) -> Result<(), MailboxError> {
                self.require_message(owner_id, &b.message_id).await?;
                // Body and attachment list change together or not at all.
                let mut tx = self.pool.begin().await.map_err(storage("tx_begin"))?;
                sqlx::query(
                    "INSERT INTO message_bodies (message_id, body_text, body_html, body_html_remote, \
                     remote_blocked, is_truncated, fetched_at) VALUES ($1,$2,$3,$4,$5,$6,$7) \
                     ON CONFLICT (message_id) DO UPDATE SET body_text = excluded.body_text, \
                     body_html = excluded.body_html, body_html_remote = excluded.body_html_remote, \
                     remote_blocked = excluded.remote_blocked, is_truncated = excluded.is_truncated, \
                     fetched_at = excluded.fetched_at",
                )
                .bind(&b.message_id)
                .bind(&b.text)
                .bind(&b.html)
                .bind(&b.html_remote)
                .bind(i64::from(b.remote_blocked))
                .bind(i64::from(b.truncated))
                .bind(now())
                .execute(&mut *tx)
                .await
                .map_err(storage("body_write"))?;
                sqlx::query("DELETE FROM attachments WHERE message_id = $1")
                    .bind(&b.message_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(storage("attachment_write"))?;
                for a in &b.attachments {
                    sqlx::query(
                        "INSERT INTO attachments (id, message_id, mime_part, display_name, media_type, \
                         size_bytes, content_id, is_inline) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
                    )
                    .bind(format!("{}#att{}", b.message_id, a.part))
                    .bind(&b.message_id)
                    .bind(a.part.to_string())
                    .bind(&a.filename)
                    .bind(&a.media_type)
                    .bind(a.size_bytes as i64)
                    .bind(&a.content_id)
                    .bind(i64::from(a.inline))
                    .execute(&mut *tx)
                    .await
                    .map_err(storage("attachment_write"))?;
                }
                tx.commit().await.map_err(storage("tx_commit"))
            }

            async fn load_body(&self, owner_id: &str, message_id: &str) -> Result<Option<BodyRecord>, MailboxError> {
                self.require_message(owner_id, message_id).await?;
                let row = sqlx::query(
                    "SELECT body_text, body_html, body_html_remote, remote_blocked, is_truncated \
                     FROM message_bodies WHERE message_id = $1",
                )
                .bind(message_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(storage("body_read"))?;
                let Some(row) = row else { return Ok(None) };
                let remote_blocked: i64 = row.try_get("remote_blocked").map_err(storage("decode"))?;
                let truncated: i64 = row.try_get("is_truncated").map_err(storage("decode"))?;
                let rows = sqlx::query(
                    "SELECT mime_part, display_name, media_type, size_bytes, content_id, is_inline \
                     FROM attachments WHERE message_id = $1",
                )
                .bind(message_id)
                .fetch_all(&self.pool)
                .await
                .map_err(storage("attachment_read"))?;
                let mut attachments = rows
                    .iter()
                    .map(|r| {
                        let part: String = r.try_get("mime_part").map_err(storage("decode"))?;
                        let name: Option<String> = r.try_get("display_name").map_err(storage("decode"))?;
                        let media: Option<String> = r.try_get("media_type").map_err(storage("decode"))?;
                        let size: Option<i64> = r.try_get("size_bytes").map_err(storage("decode"))?;
                        let inline: i64 = r.try_get("is_inline").map_err(storage("decode"))?;
                        Ok(AttachmentMeta {
                            part: part
                                .parse()
                                .map_err(|_| MailboxError::InconsistentData("attachment part"))?,
                            filename: name.unwrap_or_default(),
                            media_type: media.unwrap_or_default(),
                            size_bytes: size.unwrap_or(0).max(0) as u64,
                            content_id: r.try_get("content_id").map_err(storage("decode"))?,
                            inline: inline != 0,
                        })
                    })
                    .collect::<Result<Vec<_>, MailboxError>>()?;
                attachments.sort_by_key(|a| a.part);
                Ok(Some(BodyRecord {
                    message_id: message_id.to_string(),
                    text: row.try_get("body_text").map_err(storage("decode"))?,
                    html: row.try_get("body_html").map_err(storage("decode"))?,
                    html_remote: row.try_get("body_html_remote").map_err(storage("decode"))?,
                    remote_blocked: remote_blocked != 0,
                    truncated: truncated != 0,
                    attachments,
                }))
            }

            async fn delete_body(&self, owner_id: &str, message_id: &str) -> Result<(), MailboxError> {
                self.require_message(owner_id, message_id).await?;
                let mut tx = self.pool.begin().await.map_err(storage("tx_begin"))?;
                for sql in [
                    "DELETE FROM attachments WHERE message_id = $1",
                    "DELETE FROM message_bodies WHERE message_id = $1",
                ] {
                    sqlx::query(sql)
                        .bind(message_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(storage("body_delete"))?;
                }
                tx.commit().await.map_err(storage("tx_commit"))
            }
        }
    };
}

impl_body_store!(SqliteStore);
impl_body_store!(PgStore);
