//! SQL implementations of the folder-role and pending-operation ports (MB-122, MB-132, MB-144).
//! Same rules as the rest of the crate: one query set for both dialects, every call scoped by
//! owner, errors reduced to short static codes.
use crate::{PgStore, SqliteStore, now, storage};
use mailbox_core::MailboxError;
use mailbox_core::actions::{MessageFlag, NewOp, OpKind, OpState, PendingOp, PendingOpStore};
use mailbox_core::folders::{FolderRole, FolderRoleRecord, FolderRoleStore, RoleSource};
use mailbox_core::store::MessageRecord;
use sqlx::Row;

const MESSAGE_COLUMNS: &str = "m.id, m.account_id, m.folder_id, m.uidvalidity, m.imap_uid, \
     m.message_id_header, m.subject, m.from_display, m.from_address, m.preview, m.internal_date, \
     m.size_bytes, m.is_seen, m.is_flagged";

fn kind_columns(kind: &OpKind) -> (&'static str, Option<i64>, Option<&str>) {
    match kind {
        OpKind::SetFlag {
            flag: MessageFlag::Seen,
            on,
        } => ("seen", Some(i64::from(*on)), None),
        OpKind::SetFlag {
            flag: MessageFlag::Flagged,
            on,
        } => ("flagged", Some(i64::from(*on)), None),
        OpKind::Move { dest_folder_id } => ("move", None, Some(dest_folder_id.as_str())),
        OpKind::Expunge => ("expunge", None, None),
    }
}

fn kind_from(
    kind: &str,
    flag_value: Option<i64>,
    dest: Option<String>,
) -> Result<OpKind, MailboxError> {
    let bad = MailboxError::InconsistentData("pending operation");
    let on = flag_value.map(|v| v != 0);
    Ok(match kind {
        "seen" => OpKind::SetFlag {
            flag: MessageFlag::Seen,
            on: on.ok_or(bad)?,
        },
        "flagged" => OpKind::SetFlag {
            flag: MessageFlag::Flagged,
            on: on.ok_or(bad)?,
        },
        "move" => OpKind::Move {
            dest_folder_id: dest.ok_or(bad)?,
        },
        "expunge" => OpKind::Expunge,
        _ => return Err(bad),
    })
}

fn state_columns(state: &OpState) -> (&'static str, Option<&str>) {
    match state {
        OpState::Queued => ("queued", None),
        OpState::Failed(code) => ("failed", Some(code.as_str())),
    }
}

macro_rules! message_from_row {
    ($r:expr) => {{
        let r = $r;
        let uid_validity: i64 = r.try_get("uidvalidity").map_err(storage("decode"))?;
        let imap_uid: i64 = r.try_get("imap_uid").map_err(storage("decode"))?;
        let size: i64 = r.try_get("size_bytes").map_err(storage("decode"))?;
        let seen: i64 = r.try_get("is_seen").map_err(storage("decode"))?;
        let flagged: i64 = r.try_get("is_flagged").map_err(storage("decode"))?;
        MessageRecord {
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
        }
    }};
}

macro_rules! impl_action_store {
    ($store:ty) => {
        impl $store {
            /// A folder of an account the owner does not own behaves like a missing one.
            async fn require_folder(
                &self,
                owner_id: &str,
                account_id: &str,
                folder_id: &str,
            ) -> Result<(), MailboxError> {
                let found = sqlx::query(
                    "SELECT 1 FROM folders f JOIN accounts a ON a.id = f.account_id \
                     WHERE f.id = $1 AND f.account_id = $2 AND a.owner_id = $3",
                )
                .bind(folder_id)
                .bind(account_id)
                .bind(owner_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(storage("folder_lookup"))?;
                found.map(|_| ()).ok_or(MailboxError::NotFound)
            }
        }

        impl FolderRoleStore for $store {
            async fn save_folder_role(
                &self,
                owner_id: &str,
                record: &FolderRoleRecord,
            ) -> Result<(), MailboxError> {
                self.require_folder(owner_id, &record.account_id, &record.folder_id)
                    .await?;
                sqlx::query(
                    "INSERT INTO folder_roles (folder_id, account_id, role, source) \
                     VALUES ($1,$2,$3,$4) \
                     ON CONFLICT (folder_id) DO UPDATE SET role = excluded.role, \
                     source = excluded.source",
                )
                .bind(&record.folder_id)
                .bind(&record.account_id)
                .bind(record.role.map(FolderRole::key))
                .bind(record.source.key())
                .execute(&self.pool)
                .await
                .map_err(storage("role_write"))?;
                Ok(())
            }

            async fn list_folder_roles(
                &self,
                owner_id: &str,
                account_id: &str,
            ) -> Result<Vec<FolderRoleRecord>, MailboxError> {
                self.require_account(owner_id, account_id).await?;
                let rows = sqlx::query(
                    "SELECT folder_id, account_id, role, source FROM folder_roles \
                     WHERE account_id = $1 ORDER BY folder_id",
                )
                .bind(account_id)
                .fetch_all(&self.pool)
                .await
                .map_err(storage("role_read"))?;
                rows.iter()
                    .map(|r| {
                        let role: Option<String> = r.try_get("role").map_err(storage("decode"))?;
                        let source: String = r.try_get("source").map_err(storage("decode"))?;
                        Ok(FolderRoleRecord {
                            folder_id: r.try_get("folder_id").map_err(storage("decode"))?,
                            account_id: r.try_get("account_id").map_err(storage("decode"))?,
                            role: match role {
                                None => None,
                                Some(key) => Some(
                                    FolderRole::from_key(&key)
                                        .ok_or(MailboxError::InconsistentData("folder role"))?,
                                ),
                            },
                            source: RoleSource::from_key(&source)
                                .ok_or(MailboxError::InconsistentData("folder role source"))?,
                        })
                    })
                    .collect()
            }

            async fn remove_folder(
                &self,
                owner_id: &str,
                account_id: &str,
                folder_id: &str,
            ) -> Result<(), MailboxError> {
                self.require_folder(owner_id, account_id, folder_id).await?;
                sqlx::query("DELETE FROM folders WHERE id = $1 AND account_id = $2")
                    .bind(folder_id)
                    .bind(account_id)
                    .execute(&self.pool)
                    .await
                    .map_err(storage("folder_delete"))?;
                Ok(())
            }
        }

        impl PendingOpStore for $store {
            async fn get_message(
                &self,
                owner_id: &str,
                message_id: &str,
            ) -> Result<MessageRecord, MailboxError> {
                let row = sqlx::query(&format!(
                    "SELECT {MESSAGE_COLUMNS} FROM messages m JOIN accounts a ON a.id = m.account_id \
                     WHERE m.id = $1 AND a.owner_id = $2"
                ))
                .bind(message_id)
                .bind(owner_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(storage("message_read"))?
                .ok_or(MailboxError::NotFound)?;
                Ok(message_from_row!(&row))
            }

            async fn set_local_flags(
                &self,
                owner_id: &str,
                message_id: &str,
                seen: Option<bool>,
                flagged: Option<bool>,
            ) -> Result<(), MailboxError> {
                self.get_message(owner_id, message_id).await?;
                for (column, value) in [("is_seen", seen), ("is_flagged", flagged)] {
                    let Some(value) = value else { continue };
                    // `column` is one of two literals above, never caller input.
                    sqlx::query(&format!("UPDATE messages SET {column} = $1 WHERE id = $2"))
                        .bind(i64::from(value))
                        .bind(message_id)
                        .execute(&self.pool)
                        .await
                        .map_err(storage("flag_write"))?;
                }
                Ok(())
            }

            async fn remove_message(
                &self,
                owner_id: &str,
                message_id: &str,
            ) -> Result<(), MailboxError> {
                self.get_message(owner_id, message_id).await?;
                sqlx::query("DELETE FROM messages WHERE id = $1")
                    .bind(message_id)
                    .execute(&self.pool)
                    .await
                    .map_err(storage("message_delete"))?;
                Ok(())
            }

            async fn folder_messages(
                &self,
                owner_id: &str,
                account_id: &str,
                folder_id: &str,
            ) -> Result<Vec<MessageRecord>, MailboxError> {
                self.require_account(owner_id, account_id).await?;
                let rows = sqlx::query(&format!(
                    "SELECT {MESSAGE_COLUMNS} FROM messages m \
                     WHERE m.account_id = $1 AND m.folder_id = $2 ORDER BY m.imap_uid"
                ))
                .bind(account_id)
                .bind(folder_id)
                .fetch_all(&self.pool)
                .await
                .map_err(storage("message_read"))?;
                rows.iter().map(|r| Ok(message_from_row!(r))).collect()
            }

            async fn enqueue_op(
                &self,
                owner_id: &str,
                op: &NewOp,
            ) -> Result<PendingOp, MailboxError> {
                // The message must be the owner's and must be where the operation says it is.
                let m = self.get_message(owner_id, &op.message_id).await?;
                if m.account_id != op.account_id || m.folder_id != op.folder_id {
                    return Err(MailboxError::NotFound);
                }
                let (kind, flag_value, dest) = kind_columns(&op.kind);
                let mut tx = self.pool.begin().await.map_err(storage("tx_begin"))?;
                let seq: i64 = sqlx::query("SELECT COALESCE(MAX(seq), 0) + 1 AS next FROM pending_ops")
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(storage("op_seq"))?
                    .try_get("next")
                    .map_err(storage("decode"))?;
                // Sequence numbers can repeat once the queue has been emptied; ids must not.
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                let id = format!("op-{seq}-{nanos:x}");
                sqlx::query(
                    "INSERT INTO pending_ops (id, seq, account_id, message_id, folder_id, uidvalidity, \
                     imap_uid, kind, flag_value, previous_value, dest_folder_id, state, created_at) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'queued',$12)",
                )
                .bind(&id)
                .bind(seq)
                .bind(&op.account_id)
                .bind(&op.message_id)
                .bind(&op.folder_id)
                .bind(op.uid_validity as i64)
                .bind(i64::from(op.imap_uid))
                .bind(kind)
                .bind(flag_value)
                .bind(op.previous.map(i64::from))
                .bind(dest)
                .bind(now())
                .execute(&mut *tx)
                .await
                .map_err(storage("op_write"))?;
                tx.commit().await.map_err(storage("tx_commit"))?;
                Ok(PendingOp {
                    id,
                    seq: seq as u64,
                    account_id: op.account_id.clone(),
                    message_id: op.message_id.clone(),
                    folder_id: op.folder_id.clone(),
                    uid_validity: op.uid_validity,
                    imap_uid: op.imap_uid,
                    kind: op.kind.clone(),
                    previous: op.previous,
                    state: OpState::Queued,
                })
            }

            async fn list_ops(
                &self,
                owner_id: &str,
                account_id: &str,
            ) -> Result<Vec<PendingOp>, MailboxError> {
                self.require_account(owner_id, account_id).await?;
                let rows = sqlx::query(
                    "SELECT id, seq, account_id, message_id, folder_id, uidvalidity, imap_uid, kind, \
                     flag_value, previous_value, dest_folder_id, state, error_code \
                     FROM pending_ops WHERE account_id = $1 ORDER BY seq",
                )
                .bind(account_id)
                .fetch_all(&self.pool)
                .await
                .map_err(storage("op_read"))?;
                rows.iter()
                    .map(|r| {
                        let seq: i64 = r.try_get("seq").map_err(storage("decode"))?;
                        let uid_validity: i64 = r.try_get("uidvalidity").map_err(storage("decode"))?;
                        let imap_uid: i64 = r.try_get("imap_uid").map_err(storage("decode"))?;
                        let kind: String = r.try_get("kind").map_err(storage("decode"))?;
                        let flag_value: Option<i64> =
                            r.try_get("flag_value").map_err(storage("decode"))?;
                        let previous: Option<i64> =
                            r.try_get("previous_value").map_err(storage("decode"))?;
                        let dest: Option<String> =
                            r.try_get("dest_folder_id").map_err(storage("decode"))?;
                        let state: String = r.try_get("state").map_err(storage("decode"))?;
                        let error: Option<String> =
                            r.try_get("error_code").map_err(storage("decode"))?;
                        Ok(PendingOp {
                            id: r.try_get("id").map_err(storage("decode"))?,
                            seq: seq as u64,
                            account_id: r.try_get("account_id").map_err(storage("decode"))?,
                            message_id: r.try_get("message_id").map_err(storage("decode"))?,
                            folder_id: r.try_get("folder_id").map_err(storage("decode"))?,
                            uid_validity: uid_validity as u64,
                            imap_uid: u32::try_from(imap_uid)
                                .map_err(|_| MailboxError::InconsistentData("uid out of range"))?,
                            kind: kind_from(&kind, flag_value, dest)?,
                            previous: previous.map(|v| v != 0),
                            state: if state == "failed" {
                                OpState::Failed(error.unwrap_or_default())
                            } else {
                                OpState::Queued
                            },
                        })
                    })
                    .collect()
            }

            async fn update_op(&self, owner_id: &str, op: &PendingOp) -> Result<(), MailboxError> {
                self.require_account(owner_id, &op.account_id).await?;
                let (kind, flag_value, dest) = kind_columns(&op.kind);
                let (state, error) = state_columns(&op.state);
                let result = sqlx::query(
                    "UPDATE pending_ops SET kind = $1, flag_value = $2, dest_folder_id = $3, \
                     state = $4, error_code = $5 WHERE id = $6 AND account_id = $7",
                )
                .bind(kind)
                .bind(flag_value)
                .bind(dest)
                .bind(state)
                .bind(error)
                .bind(&op.id)
                .bind(&op.account_id)
                .execute(&self.pool)
                .await
                .map_err(storage("op_write"))?;
                if result.rows_affected() == 0 {
                    return Err(MailboxError::NotFound);
                }
                Ok(())
            }

            async fn remove_op(&self, owner_id: &str, op_id: &str) -> Result<bool, MailboxError> {
                let result = sqlx::query(
                    "DELETE FROM pending_ops WHERE id = $1 AND account_id IN \
                     (SELECT id FROM accounts WHERE owner_id = $2)",
                )
                .bind(op_id)
                .bind(owner_id)
                .execute(&self.pool)
                .await
                .map_err(storage("op_delete"))?;
                Ok(result.rows_affected() > 0)
            }
        }
    };
}

impl_action_store!(SqliteStore);
impl_action_store!(PgStore);
