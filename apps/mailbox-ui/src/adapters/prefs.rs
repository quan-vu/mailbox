//! Settings and templates on Desktop (MB-210): read and written through `PreferencesStore` in the
//! local SQLite database. Nothing secret passes through here.
use super::account::{OWNER, open_store};
use mailbox_core::MailboxError;
use mailbox_core::prefs::{Lang, Preferences, PreferencesStore, Template};

pub async fn load() -> Result<(Preferences, Vec<Template>), MailboxError> {
    let store = open_store().await?;
    Ok((
        store.load_preferences(OWNER).await?,
        store.list_templates(OWNER).await?,
    ))
}

/// The stored interface language (Vietnamese when nothing is stored).
pub async fn load_language() -> Result<Lang, MailboxError> {
    language_of(&open_store().await?).await
}

pub async fn save_language(lang: Lang) -> Result<(), MailboxError> {
    set_language(&open_store().await?, lang).await
}

async fn language_of<S: PreferencesStore>(store: &S) -> Result<Lang, MailboxError> {
    Ok(store.load_preferences(OWNER).await?.lang)
}

/// Change only the language of the stored record; every other setting stays as stored.
async fn set_language<S: PreferencesStore>(store: &S, lang: Lang) -> Result<(), MailboxError> {
    let mut prefs = store.load_preferences(OWNER).await?;
    if prefs.lang == lang {
        return Ok(());
    }
    prefs.lang = lang;
    store.save_preferences(OWNER, &prefs).await
}

pub async fn save(prefs: &Preferences) -> Result<(), MailboxError> {
    open_store().await?.save_preferences(OWNER, prefs).await
}

pub async fn save_template(t: &Template) -> Result<Vec<Template>, MailboxError> {
    let store = open_store().await?;
    store.upsert_template(OWNER, t).await?;
    store.list_templates(OWNER).await
}

pub async fn delete_template(id: &str) -> Result<Vec<Template>, MailboxError> {
    let store = open_store().await?;
    store.delete_template(OWNER, id).await?;
    store.list_templates(OWNER).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailbox_core::prefs::ThemeChoice;

    #[tokio::test]
    async fn settings_survive_reopening_the_database() {
        let dir = std::env::temp_dir().join(format!("mb-prefs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.sqlite3");
        let p = Preferences {
            theme: ThemeChoice::Dark,
            ..Default::default()
        };
        {
            let s = mailbox_store::SqliteStore::open(&path).await.unwrap();
            s.save_preferences(OWNER, &p).await.unwrap();
        }
        let s = mailbox_store::SqliteStore::open(&path).await.unwrap();
        assert_eq!(
            s.load_preferences(OWNER).await.unwrap().theme,
            ThemeChoice::Dark
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn language_is_remembered_and_leaves_other_settings_alone() {
        let dir = std::env::temp_dir().join(format!("mb-lang-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.sqlite3");
        {
            let s = mailbox_store::SqliteStore::open(&path).await.unwrap();
            assert_eq!(language_of(&s).await.unwrap(), Lang::Vi);
            let p = Preferences {
                theme: ThemeChoice::Dark,
                ..Default::default()
            };
            s.save_preferences(OWNER, &p).await.unwrap();
            set_language(&s, Lang::En).await.unwrap();
        }
        let s = mailbox_store::SqliteStore::open(&path).await.unwrap();
        assert_eq!(language_of(&s).await.unwrap(), Lang::En);
        assert_eq!(
            s.load_preferences(OWNER).await.unwrap().theme,
            ThemeChoice::Dark
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
