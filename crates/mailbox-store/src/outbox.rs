//! SQL implementation of [`mailbox_core::outbox::DraftStore`] (MB-133, MB-151) for both dialects.
//!
//! Rows hold what the user wrote plus the outbox state; never credentials. Lists (recipients,
//! references, attachment references) are stored as JSON text so both dialects share one query
//! set. State changes are a single conditional UPDATE, so they are atomic compare-and-set.
use crate::{PgStore, SqliteStore, now, storage};
use mailbox_core::MailboxError;
use mailbox_core::outbox::{
    AttachmentRef, DraftRecord, DraftStore, ForwardedPart, OutboxState, ScheduleStore,
    SentCopyState,
};
use serde_json::{Value, json};
use sqlx::Row;

const COLUMNS: &str = "id, owner_id, account_id, version, message_id_header, from_identity, \
    recipient_json, subject, body_text, body_html, in_reply_to, references_json, attachments_json, \
    outbox_state, fail_reason, retryable, sent_copy, updated_at, scheduled_at";

const BAD_ROW: MailboxError = MailboxError::InconsistentData("draft row");

fn strings(value: &Value) -> Result<Vec<String>, MailboxError> {
    value
        .as_array()
        .ok_or(BAD_ROW)?
        .iter()
        .map(|v| v.as_str().map(str::to_string).ok_or(BAD_ROW))
        .collect()
}

fn parse(text: &str) -> Result<Value, MailboxError> {
    serde_json::from_str(text).map_err(|_| BAD_ROW)
}

fn attachments_json(list: &[AttachmentRef]) -> String {
    Value::Array(
        list.iter()
            .map(|a| {
                let mut v = json!({"path": a.path, "name": a.name, "size": a.size_bytes});
                if let Some(f) = &a.forwarded {
                    v["fwd_msg"] = json!(f.message_id);
                    v["fwd_part"] = json!(f.part);
                }
                v
            })
            .collect(),
    )
    .to_string()
}

fn attachments(text: &str) -> Result<Vec<AttachmentRef>, MailboxError> {
    parse(text)?
        .as_array()
        .ok_or(BAD_ROW)?
        .iter()
        .map(|v| {
            Ok(AttachmentRef {
                path: v["path"].as_str().ok_or(BAD_ROW)?.to_string(),
                name: v["name"].as_str().ok_or(BAD_ROW)?.to_string(),
                size_bytes: v["size"].as_u64().ok_or(BAD_ROW)?,
                forwarded: match (v.get("fwd_msg"), v.get("fwd_part")) {
                    (None, None) => None,
                    (Some(m), Some(p)) => Some(ForwardedPart {
                        message_id: m.as_str().ok_or(BAD_ROW)?.to_string(),
                        part: u32::try_from(p.as_u64().ok_or(BAD_ROW)?).map_err(|_| BAD_ROW)?,
                    }),
                    _ => return Err(BAD_ROW),
                },
            })
        })
        .collect()
}

/// (state key, failure reason, retryable flag) as stored.
fn state_columns(state: OutboxState) -> (&'static str, Option<&'static str>, i64) {
    match state {
        OutboxState::Failed { reason, retryable } => {
            (state.key(), Some(reason.key()), i64::from(retryable))
        }
        other => (other.key(), None, 0),
    }
}

macro_rules! impl_draft_store {
    ($store:ty, $row:ty) => {
        impl $store {
            fn draft_from_row(r: &$row) -> Result<DraftRecord, MailboxError> {
                let d = storage("decode");
                let recipients = parse(&r.try_get::<String, _>("recipient_json").map_err(&d)?)?;
                let version: i64 = r.try_get("version").map_err(&d)?;
                let retryable: i64 = r.try_get("retryable").map_err(&d)?;
                let state: String = r.try_get("outbox_state").map_err(&d)?;
                let reason: Option<String> = r.try_get("fail_reason").map_err(&d)?;
                let sent_copy: String = r.try_get("sent_copy").map_err(&d)?;
                Ok(DraftRecord {
                    id: r.try_get("id").map_err(&d)?,
                    owner_id: r.try_get("owner_id").map_err(&d)?,
                    account_id: r.try_get("account_id").map_err(&d)?,
                    version: u32::try_from(version).map_err(|_| BAD_ROW)?,
                    message_id: r.try_get("message_id_header").map_err(&d)?,
                    from_identity: r.try_get("from_identity").map_err(&d)?,
                    to: strings(&recipients["to"])?,
                    cc: strings(&recipients["cc"])?,
                    bcc: strings(&recipients["bcc"])?,
                    subject: r.try_get("subject").map_err(&d)?,
                    body_text: r.try_get("body_text").map_err(&d)?,
                    body_html: r.try_get("body_html").map_err(&d)?,
                    in_reply_to: r.try_get("in_reply_to").map_err(&d)?,
                    references: strings(&parse(
                        &r.try_get::<String, _>("references_json").map_err(&d)?,
                    )?)?,
                    attachments: attachments(
                        &r.try_get::<String, _>("attachments_json").map_err(&d)?,
                    )?,
                    // Unknown text in a state column is an error, never read as "draft".
                    state: OutboxState::from_parts(&state, reason.as_deref(), retryable != 0)
                        .ok_or(BAD_ROW)?,
                    sent_copy: SentCopyState::from_key(&sent_copy).ok_or(BAD_ROW)?,
                    updated_at: r.try_get("updated_at").map_err(&d)?,
                    scheduled_at: r.try_get("scheduled_at").map_err(&d)?,
                })
            }

            /// (version, state key) of a draft the owner can see.
            async fn draft_head(
                &self,
                owner_id: &str,
                draft_id: &str,
            ) -> Result<Option<(i64, String)>, MailboxError> {
                let row = sqlx::query(
                    "SELECT version, outbox_state FROM drafts WHERE id = $1 AND owner_id = $2",
                )
                .bind(draft_id)
                .bind(owner_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(storage("draft_read"))?;
                row.map(|r| {
                    Ok((
                        r.try_get("version").map_err(storage("decode"))?,
                        r.try_get("outbox_state").map_err(storage("decode"))?,
                    ))
                })
                .transpose()
            }
        }

        impl DraftStore for $store {
            async fn save_draft(&self, d: &DraftRecord) -> Result<u32, MailboxError> {
                self.require_account(&d.owner_id, &d.account_id).await?;
                let recipients = json!({"to": d.to, "cc": d.cc, "bcc": d.bcc}).to_string();
                let references = json!(d.references).to_string();
                let files = attachments_json(&d.attachments);
                let Some((version, state)) = self.draft_head(&d.owner_id, &d.id).await? else {
                    if d.message_id.is_empty() {
                        return Err(MailboxError::InconsistentData("draft message id"));
                    }
                    // An id that already belongs to another owner is reported as not found.
                    let result = sqlx::query(
                        "INSERT INTO drafts (id, owner_id, account_id, version, message_id_header, \
                         from_identity, recipient_json, subject, body_text, body_html, in_reply_to, \
                         references_json, attachments_json, outbox_state, retryable, sent_copy, updated_at) \
                         VALUES ($1,$2,$3,1,$4,$5,$6,$7,$8,$9,$10,$11,$12,'draft',0,'none',$13) \
                         ON CONFLICT (id) DO NOTHING",
                    )
                    .bind(&d.id)
                    .bind(&d.owner_id)
                    .bind(&d.account_id)
                    .bind(&d.message_id)
                    .bind(&d.from_identity)
                    .bind(&recipients)
                    .bind(&d.subject)
                    .bind(&d.body_text)
                    .bind(&d.body_html)
                    .bind(&d.in_reply_to)
                    .bind(&references)
                    .bind(&files)
                    .bind(now())
                    .execute(&self.pool)
                    .await
                    .map_err(storage("draft_write"))?;
                    return if result.rows_affected() == 1 {
                        Ok(1)
                    } else {
                        Err(MailboxError::NotFound)
                    };
                };
                if state != "draft" {
                    return Err(MailboxError::InconsistentData("draft not editable"));
                }
                if version != i64::from(d.version) {
                    return Err(MailboxError::InconsistentData("draft version"));
                }
                // The guards are repeated in SQL so a concurrent writer cannot slip in between.
                let result = sqlx::query(
                    "UPDATE drafts SET account_id = $3, version = version + 1, from_identity = $4, \
                     recipient_json = $5, subject = $6, body_text = $7, body_html = $8, in_reply_to = $9, \
                     references_json = $10, attachments_json = $11, updated_at = $12 \
                     WHERE id = $1 AND owner_id = $2 AND version = $13 AND outbox_state = 'draft'",
                )
                .bind(&d.id)
                .bind(&d.owner_id)
                .bind(&d.account_id)
                .bind(&d.from_identity)
                .bind(&recipients)
                .bind(&d.subject)
                .bind(&d.body_text)
                .bind(&d.body_html)
                .bind(&d.in_reply_to)
                .bind(&references)
                .bind(&files)
                .bind(now())
                .bind(version)
                .execute(&self.pool)
                .await
                .map_err(storage("draft_write"))?;
                if result.rows_affected() == 1 {
                    Ok(d.version + 1)
                } else {
                    Err(MailboxError::InconsistentData("draft version"))
                }
            }

            async fn get_draft(&self, owner_id: &str, draft_id: &str) -> Result<DraftRecord, MailboxError> {
                let row = sqlx::query(&format!(
                    "SELECT {COLUMNS} FROM drafts WHERE id = $1 AND owner_id = $2"
                ))
                .bind(draft_id)
                .bind(owner_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(storage("draft_read"))?;
                Self::draft_from_row(&row.ok_or(MailboxError::NotFound)?)
            }

            async fn list_drafts(&self, owner_id: &str) -> Result<Vec<DraftRecord>, MailboxError> {
                let rows = sqlx::query(&format!(
                    "SELECT {COLUMNS} FROM drafts WHERE owner_id = $1 ORDER BY updated_at DESC, id"
                ))
                .bind(owner_id)
                .fetch_all(&self.pool)
                .await
                .map_err(storage("draft_read"))?;
                rows.iter().map(Self::draft_from_row).collect()
            }

            async fn delete_draft(&self, owner_id: &str, draft_id: &str) -> Result<(), MailboxError> {
                let result = sqlx::query(
                    "DELETE FROM drafts WHERE id = $1 AND owner_id = $2 \
                     AND outbox_state NOT IN ('queued', 'sending')",
                )
                .bind(draft_id)
                .bind(owner_id)
                .execute(&self.pool)
                .await
                .map_err(storage("draft_delete"))?;
                if result.rows_affected() == 1 {
                    return Ok(());
                }
                match self.draft_head(owner_id, draft_id).await? {
                    Some(_) => Err(MailboxError::InconsistentData("draft in flight")),
                    None => Err(MailboxError::NotFound),
                }
            }

            async fn set_state(
                &self,
                owner_id: &str,
                draft_id: &str,
                expected: OutboxState,
                next: OutboxState,
            ) -> Result<(), MailboxError> {
                let (from_key, from_reason, from_retry) = state_columns(expected);
                let (key, reason, retry) = state_columns(next);
                // Reaching `sent` opens the Sent-folder reconciliation.
                let sent_copy = (next == OutboxState::Sent).then_some("pending");
                let result = sqlx::query(
                    "UPDATE drafts SET outbox_state = $3, fail_reason = $4, retryable = $5, \
                     sent_copy = COALESCE($6, sent_copy), updated_at = $7, \
                     scheduled_at = CASE WHEN $3 = 'draft' THEN NULL ELSE scheduled_at END \
                     WHERE id = $1 AND owner_id = $2 AND outbox_state = $8 \
                     AND COALESCE(fail_reason, '') = $9 AND retryable = $10",
                )
                .bind(draft_id)
                .bind(owner_id)
                .bind(key)
                .bind(reason)
                .bind(retry)
                .bind(sent_copy)
                .bind(now())
                .bind(from_key)
                .bind(from_reason.unwrap_or(""))
                .bind(from_retry)
                .execute(&self.pool)
                .await
                .map_err(storage("draft_state"))?;
                if result.rows_affected() == 1 {
                    return Ok(());
                }
                match self.draft_head(owner_id, draft_id).await? {
                    Some(_) => Err(MailboxError::InconsistentData("outbox state changed")),
                    None => Err(MailboxError::NotFound),
                }
            }

            async fn set_sent_copy(
                &self,
                owner_id: &str,
                draft_id: &str,
                sent_copy: SentCopyState,
            ) -> Result<(), MailboxError> {
                let result =
                    sqlx::query("UPDATE drafts SET sent_copy = $3 WHERE id = $1 AND owner_id = $2")
                        .bind(draft_id)
                        .bind(owner_id)
                        .bind(sent_copy.key())
                        .execute(&self.pool)
                        .await
                        .map_err(storage("draft_state"))?;
                if result.rows_affected() == 1 {
                    Ok(())
                } else {
                    Err(MailboxError::NotFound)
                }
            }
        }
    };
}

macro_rules! impl_schedule_store {
    ($store:ty) => {
        impl ScheduleStore for $store {
            async fn schedule_draft(
                &self,
                owner_id: &str,
                draft_id: &str,
                at: i64,
            ) -> Result<(), MailboxError> {
                let result = sqlx::query(
                    "UPDATE drafts SET outbox_state = 'scheduled', scheduled_at = $3, updated_at = $4 \
                     WHERE id = $1 AND owner_id = $2 AND outbox_state IN ('draft', 'scheduled')",
                )
                .bind(draft_id)
                .bind(owner_id)
                .bind(at)
                .bind(now())
                .execute(&self.pool)
                .await
                .map_err(storage("draft_schedule"))?;
                if result.rows_affected() == 1 {
                    return Ok(());
                }
                match self.draft_head(owner_id, draft_id).await? {
                    Some(_) => Err(MailboxError::InconsistentData("outbox state changed")),
                    None => Err(MailboxError::NotFound),
                }
            }

            async fn unschedule_draft(
                &self,
                owner_id: &str,
                draft_id: &str,
            ) -> Result<(), MailboxError> {
                let result = sqlx::query(
                    "UPDATE drafts SET outbox_state = 'draft', scheduled_at = NULL, updated_at = $3 \
                     WHERE id = $1 AND owner_id = $2 AND outbox_state = 'scheduled'",
                )
                .bind(draft_id)
                .bind(owner_id)
                .bind(now())
                .execute(&self.pool)
                .await
                .map_err(storage("draft_schedule"))?;
                if result.rows_affected() == 1 {
                    return Ok(());
                }
                match self.draft_head(owner_id, draft_id).await? {
                    Some(_) => Err(MailboxError::InconsistentData("outbox state changed")),
                    None => Err(MailboxError::NotFound),
                }
            }
        }
    };
}

impl_draft_store!(SqliteStore, sqlx::sqlite::SqliteRow);
impl_schedule_store!(SqliteStore);
impl_schedule_store!(PgStore);
impl_draft_store!(PgStore, sqlx::postgres::PgRow);
