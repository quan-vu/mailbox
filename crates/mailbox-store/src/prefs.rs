//! SQL implementation of [`mailbox_core::prefs::PreferencesStore`] for both dialects.
//! Settings are one JSON row (`preferences`, key `settings`); templates have their own table.
use crate::{PgStore, SqliteStore, now, storage};
use mailbox_core::MailboxError;
use mailbox_core::prefs::{
    MAX_TEMPLATES, Preferences, PreferencesStore, Template, decode_preferences, encode_preferences,
};
use sqlx::Row;

const SETTINGS_KEY: &str = "settings";

macro_rules! impl_preferences_store {
    ($store:ty) => {
        impl PreferencesStore for $store {
            async fn load_preferences(&self, owner_id: &str) -> Result<Preferences, MailboxError> {
                let row = sqlx::query(
                    "SELECT setting_value_json FROM preferences WHERE owner_id = $1 AND setting_key = $2",
                )
                .bind(owner_id)
                .bind(SETTINGS_KEY)
                .fetch_optional(&self.pool)
                .await
                .map_err(storage("prefs_read"))?;
                let json: Option<String> = match row {
                    Some(r) => Some(r.try_get("setting_value_json").map_err(storage("decode"))?),
                    None => None,
                };
                decode_preferences(json.as_deref())
            }

            async fn save_preferences(&self, owner_id: &str, prefs: &Preferences) -> Result<(), MailboxError> {
                let json = encode_preferences(prefs)?;
                sqlx::query(
                    "INSERT INTO preferences (owner_id, setting_key, setting_value_json) VALUES ($1,$2,$3) \
                     ON CONFLICT (owner_id, setting_key) DO UPDATE SET setting_value_json = excluded.setting_value_json",
                )
                .bind(owner_id)
                .bind(SETTINGS_KEY)
                .bind(json)
                .execute(&self.pool)
                .await
                .map_err(storage("prefs_write"))?;
                Ok(())
            }

            async fn reset_preferences(&self, owner_id: &str) -> Result<(), MailboxError> {
                sqlx::query("DELETE FROM preferences WHERE owner_id = $1 AND setting_key = $2")
                    .bind(owner_id)
                    .bind(SETTINGS_KEY)
                    .execute(&self.pool)
                    .await
                    .map_err(storage("prefs_write"))?;
                Ok(())
            }

            async fn list_templates(&self, owner_id: &str) -> Result<Vec<Template>, MailboxError> {
                let rows = sqlx::query(
                    "SELECT id, name, subject, body FROM message_templates WHERE owner_id = $1 ORDER BY name, id",
                )
                .bind(owner_id)
                .fetch_all(&self.pool)
                .await
                .map_err(storage("template_read"))?;
                rows.iter()
                    .map(|r| {
                        Ok(Template {
                            id: r.try_get("id").map_err(storage("decode"))?,
                            name: r.try_get("name").map_err(storage("decode"))?,
                            subject: r.try_get("subject").map_err(storage("decode"))?,
                            body: r.try_get("body").map_err(storage("decode"))?,
                        })
                    })
                    .collect()
            }

            async fn upsert_template(&self, owner_id: &str, t: &Template) -> Result<(), MailboxError> {
                if t.problem().is_some() {
                    return Err(MailboxError::InconsistentData("template"));
                }
                let exists = sqlx::query("SELECT 1 FROM message_templates WHERE owner_id = $1 AND id = $2")
                    .bind(owner_id)
                    .bind(&t.id)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(storage("template_read"))?
                    .is_some();
                if !exists {
                    let count: i64 = sqlx::query("SELECT COUNT(*) AS n FROM message_templates WHERE owner_id = $1")
                        .bind(owner_id)
                        .fetch_one(&self.pool)
                        .await
                        .map_err(storage("template_read"))?
                        .try_get("n")
                        .map_err(storage("decode"))?;
                    if count as usize >= MAX_TEMPLATES {
                        return Err(MailboxError::Storage("template_limit"));
                    }
                }
                sqlx::query(
                    "INSERT INTO message_templates (id, owner_id, name, subject, body, updated_at) \
                     VALUES ($1,$2,$3,$4,$5,$6) \
                     ON CONFLICT (owner_id, id) DO UPDATE SET name = excluded.name, \
                     subject = excluded.subject, body = excluded.body, updated_at = excluded.updated_at",
                )
                .bind(&t.id)
                .bind(owner_id)
                .bind(&t.name)
                .bind(&t.subject)
                .bind(&t.body)
                .bind(now())
                .execute(&self.pool)
                .await
                .map_err(storage("template_write"))?;
                Ok(())
            }

            async fn delete_template(&self, owner_id: &str, template_id: &str) -> Result<bool, MailboxError> {
                let result = sqlx::query("DELETE FROM message_templates WHERE owner_id = $1 AND id = $2")
                    .bind(owner_id)
                    .bind(template_id)
                    .execute(&self.pool)
                    .await
                    .map_err(storage("template_write"))?;
                Ok(result.rows_affected() > 0)
            }
        }
    };
}

impl_preferences_store!(SqliteStore);
impl_preferences_store!(PgStore);

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_corrupt_or_newer_record_is_an_error_not_defaults() {
        let store = SqliteStore::in_memory().await.unwrap();
        for raw in [r#"{"version":99}"#, "not json"] {
            sqlx::query(
                "INSERT INTO preferences (owner_id, setting_key, setting_value_json) VALUES ('local','settings',$1) \
                 ON CONFLICT (owner_id, setting_key) DO UPDATE SET setting_value_json = excluded.setting_value_json",
            )
            .bind(raw)
            .execute(&store.pool)
            .await
            .unwrap();
            // Refused, so an old build never overwrites a record it does not understand by accident.
            assert!(store.load_preferences("local").await.is_err(), "{raw}");
        }
    }
}
