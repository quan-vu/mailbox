//! Behavioural contract of `PreferencesStore` (MB-210). SQLite always; PostgreSQL when
//! `MAILBOX_TEST_PG_URL` points at a throwaway database (see `contract.rs`).
use mailbox_core::MailboxError;
use mailbox_core::prefs::{
    Density, MAX_TEMPLATES, NotifyMode, Preferences, PreferencesStore, Signature, Template,
    ThemeChoice,
};
use mailbox_store::{PgStore, SqliteStore};

fn tpl(id: &str, name: &str) -> Template {
    Template {
        id: id.into(),
        name: name.into(),
        subject: "Cảm ơn {{first_name}}".into(),
        body: "Chào {{name}},\n{{email}}".into(),
    }
}

async fn contract(store: &impl PreferencesStore, o: &str) {
    let other = format!("{o}-other");
    // Nothing saved: defaults, and remote images stay blocked.
    let d = store.load_preferences(o).await.unwrap();
    assert_eq!(d, Preferences::default());
    assert!(!d.remote_images);

    let mut p = Preferences {
        theme: ThemeChoice::Dark,
        density: Density::Compact,
        notify_mode: NotifyMode::SenderAndSubject,
        default_identity: Some("acc-a".into()),
        remote_images: true, // must not survive
        ..Default::default()
    };
    p.signatures.insert(
        "acc-a".into(),
        Signature {
            text: "Linh\nCông ty".into(),
            use_for_replies: false,
        },
    );
    store.save_preferences(o, &p).await.unwrap();
    let back = store.load_preferences(o).await.unwrap();
    assert_eq!(back.theme, ThemeChoice::Dark);
    assert_eq!(back.density, Density::Compact);
    assert_eq!(back.notify_mode, NotifyMode::SenderAndSubject);
    assert_eq!(back.signatures["acc-a"].text, "Linh\nCông ty");
    assert!(!back.signatures["acc-a"].use_for_replies);
    assert!(!back.remote_images);

    // Update replaces; another owner is untouched.
    store
        .save_preferences(o, &Preferences::default())
        .await
        .unwrap();
    assert_eq!(
        store.load_preferences(o).await.unwrap().theme,
        ThemeChoice::Light
    );
    assert_eq!(
        store.load_preferences(&other).await.unwrap(),
        Preferences::default()
    );

    store.save_preferences(o, &p).await.unwrap();
    store.reset_preferences(o).await.unwrap();
    assert_eq!(
        store.load_preferences(o).await.unwrap(),
        Preferences::default()
    );

    // Templates: CRUD, order by name, owner scope.
    store.upsert_template(o, &tpl("b", "Beta")).await.unwrap();
    store.upsert_template(o, &tpl("a", "Alpha")).await.unwrap();
    store
        .upsert_template(&other, &tpl("a", "Other owner"))
        .await
        .unwrap();
    let names: Vec<_> = store
        .list_templates(o)
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, ["Alpha", "Beta"]);
    store
        .upsert_template(
            o,
            &Template {
                name: "Alpha 2".into(),
                ..tpl("a", "")
            },
        )
        .await
        .unwrap();
    assert_eq!(store.list_templates(o).await.unwrap()[0].name, "Alpha 2");
    assert_eq!(
        store.list_templates(&other).await.unwrap()[0].name,
        "Other owner"
    );
    // Reset leaves templates alone.
    store.reset_preferences(o).await.unwrap();
    assert_eq!(store.list_templates(o).await.unwrap().len(), 2);
    // Invalid template refused.
    assert_eq!(
        store.upsert_template(o, &tpl("c", " ")).await,
        Err(MailboxError::InconsistentData("template"))
    );
    assert!(!store.delete_template(&other, "b").await.unwrap());
    assert!(store.delete_template(o, "b").await.unwrap());
    assert!(!store.delete_template(o, "b").await.unwrap());
    assert_eq!(store.list_templates(o).await.unwrap().len(), 1);

    // Count limit.
    for i in 0..MAX_TEMPLATES {
        let r = store
            .upsert_template(o, &tpl(&format!("n{i}"), &format!("N{i}")))
            .await;
        if i + 1 < MAX_TEMPLATES {
            r.unwrap();
        } else {
            assert_eq!(r, Err(MailboxError::Storage("template_limit")));
        }
    }
    // Editing an existing one still works at the limit.
    store
        .upsert_template(o, &tpl("a", "Alpha 3"))
        .await
        .unwrap();
    for t in store.list_templates(o).await.unwrap() {
        store.delete_template(o, &t.id).await.unwrap();
    }
    store.reset_preferences(&other).await.unwrap();
    store.delete_template(&other, "a").await.unwrap();
}

#[tokio::test]
async fn sqlite_meets_the_preferences_contract() {
    let store = SqliteStore::in_memory().await.unwrap();
    contract(&store, "local").await;
}

#[test]
fn settings_schema_has_no_secret_columns() {
    for file in [
        "../../migrations/sqlite/0006_settings.sql",
        "../../migrations/postgres/0006_settings.sql",
    ] {
        let ddl: String = std::fs::read_to_string(file)
            .unwrap()
            .to_lowercase()
            .lines()
            .filter(|l| !l.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in ["password", "secret", "token", "blob", "bytea"] {
            assert!(!ddl.contains(forbidden), "{file} has `{forbidden}`");
        }
    }
}

#[tokio::test]
#[ignore = "needs MAILBOX_TEST_PG_URL"]
async fn postgres_meets_the_preferences_contract() {
    let url =
        std::env::var("MAILBOX_TEST_PG_URL").expect("set MAILBOX_TEST_PG_URL to run this test");
    let store = PgStore::connect(&url).await.unwrap();
    let run = format!(
        "p{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    contract(&store, &run).await;
}
