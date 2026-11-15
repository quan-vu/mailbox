//! MB-304: the local database holds headers, bodies, drafts and the search index, so the file
//! must be readable by its owner only, whatever the process umask is.
#![cfg(unix)]
use mailbox_store::SqliteStore;
use std::os::unix::fs::PermissionsExt;

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("mailbox-mode-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn mode(p: &std::path::Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[tokio::test]
async fn a_new_database_file_is_owner_only() {
    let d = dir("new");
    let path = d.join("mailbox.sqlite3");
    let _store = SqliteStore::open(&path).await.unwrap();
    assert_eq!(mode(&path), 0o600);
    let _ = std::fs::remove_dir_all(&d);
}

#[tokio::test]
async fn an_existing_world_readable_database_is_tightened_on_open() {
    let d = dir("old");
    let path = d.join("mailbox.sqlite3");
    drop(SqliteStore::open(&path).await.unwrap());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let journal = d.join("mailbox.sqlite3-journal");
    std::fs::write(&journal, b"").unwrap();
    std::fs::set_permissions(&journal, std::fs::Permissions::from_mode(0o644)).unwrap();
    let _store = SqliteStore::open(&path).await.unwrap();
    assert_eq!(mode(&path), 0o600);
    if journal.exists() {
        assert_eq!(mode(&journal), 0o600);
    }
    let _ = std::fs::remove_dir_all(&d);
}
