//! MB-122: mapping server folders to roles. Pure functions, no I/O.
use mailbox_core::folders::{
    FolderRole, ListedFolder, RoleSource, assign_roles, role_from_attributes, role_from_name,
};

fn listed(name: &str, delimiter: &str, attributes: &[&str]) -> ListedFolder {
    ListedFolder {
        name: name.into(),
        display_name: name.into(),
        delimiter: Some(delimiter.into()),
        attributes: attributes.iter().map(|a| a.to_string()).collect(),
        selectable: true,
    }
}

#[test]
fn special_use_attributes_map_to_roles_case_insensitively() {
    let attr = |a: &str| role_from_attributes(&[a.to_string()]);
    assert_eq!(attr("\\Sent"), Some(FolderRole::Sent));
    assert_eq!(attr("\\drafts"), Some(FolderRole::Drafts));
    assert_eq!(attr("\\TRASH"), Some(FolderRole::Trash));
    assert_eq!(attr("\\Archive"), Some(FolderRole::Archive));
    assert_eq!(attr("\\Junk"), Some(FolderRole::Junk));
    // XLIST-style attributes some servers still return.
    assert_eq!(attr("\\Spam"), Some(FolderRole::Junk));
    assert_eq!(attr("\\Inbox"), Some(FolderRole::Inbox));
    // Not roles: all-mail and plain LIST flags.
    for other in [
        "\\All",
        "\\AllMail",
        "\\HasNoChildren",
        "\\Flagged",
        "\\Important",
    ] {
        assert_eq!(attr(other), None, "{other}");
    }
}

#[test]
fn well_known_names_are_recognised_in_english_and_vietnamese() {
    let cases = [
        ("INBOX", FolderRole::Inbox),
        ("inbox", FolderRole::Inbox),
        ("Sent", FolderRole::Sent),
        ("Sent Items", FolderRole::Sent),
        ("Sent Messages", FolderRole::Sent),
        ("Thư đã gửi", FolderRole::Sent),
        ("Đã gửi", FolderRole::Sent),
        ("Drafts", FolderRole::Drafts),
        ("Bản nháp", FolderRole::Drafts),
        ("Thư nháp", FolderRole::Drafts),
        ("Trash", FolderRole::Trash),
        ("Deleted Items", FolderRole::Trash),
        ("Thùng rác", FolderRole::Trash),
        ("Archive", FolderRole::Archive),
        ("Archives", FolderRole::Archive),
        ("Lưu trữ", FolderRole::Archive),
        ("Junk", FolderRole::Junk),
        ("Spam", FolderRole::Junk),
        ("Junk E-mail", FolderRole::Junk),
        ("Thư rác", FolderRole::Junk),
    ];
    for (name, role) in cases {
        assert_eq!(role_from_name(name, Some("/")), Some(role), "{name}");
    }
}

#[test]
fn names_under_an_inbox_or_provider_prefix_match_with_either_delimiter() {
    assert_eq!(
        role_from_name("INBOX.Sent", Some(".")),
        Some(FolderRole::Sent)
    );
    assert_eq!(
        role_from_name("INBOX/Trash", Some("/")),
        Some(FolderRole::Trash)
    );
    assert_eq!(
        role_from_name("INBOX.Thùng rác", Some(".")),
        Some(FolderRole::Trash)
    );
    assert_eq!(
        role_from_name("[Gmail]/Spam", Some("/")),
        Some(FolderRole::Junk)
    );
    // Delimiter unknown: both common ones are tried.
    assert_eq!(
        role_from_name("INBOX.Drafts", None),
        Some(FolderRole::Drafts)
    );
    assert_eq!(
        role_from_name("INBOX/Archive", None),
        Some(FolderRole::Archive)
    );
}

#[test]
fn user_folders_and_deeper_paths_get_no_role() {
    for name in [
        "Projects",
        "Sent.2024",
        "Work/Clients/Sent",
        "INBOX.Projects.Trash",
        "Sentinel",
        "Not Spam",
        "",
    ] {
        assert_eq!(role_from_name(name, Some(".")), None, "{name}");
        assert_eq!(role_from_name(name, Some("/")), None, "{name}");
    }
}

#[test]
fn attributes_win_over_names_and_each_role_goes_to_one_folder() {
    let folders = vec![
        listed("INBOX", ".", &["\\HasNoChildren"]),
        // Named like trash, but the server marks another folder as the trash.
        listed("INBOX.Trash", ".", &[]),
        listed("INBOX.Bin", ".", &["\\Trash"]),
        listed("INBOX.Sent", ".", &[]),
        listed("INBOX.Sent Items", ".", &[]),
        listed("INBOX.Projects", ".", &[]),
    ];
    let got: Vec<_> = assign_roles(&folders)
        .into_iter()
        .map(|a| (a.name, a.role, a.source))
        .collect();
    assert_eq!(
        got,
        vec![
            (
                "INBOX".to_string(),
                Some(FolderRole::Inbox),
                RoleSource::Name
            ),
            ("INBOX.Trash".to_string(), None, RoleSource::Name),
            (
                "INBOX.Bin".to_string(),
                Some(FolderRole::Trash),
                RoleSource::Attribute
            ),
            (
                "INBOX.Sent".to_string(),
                Some(FolderRole::Sent),
                RoleSource::Name
            ),
            ("INBOX.Sent Items".to_string(), None, RoleSource::Name),
            ("INBOX.Projects".to_string(), None, RoleSource::Name),
        ]
    );
}

#[test]
fn role_keys_round_trip() {
    for role in FolderRole::ALL {
        assert_eq!(FolderRole::from_key(role.key()), Some(role));
    }
    assert_eq!(FolderRole::from_key("all"), None);
    for source in [RoleSource::Attribute, RoleSource::Name, RoleSource::User] {
        assert_eq!(RoleSource::from_key(source.key()), Some(source));
    }
}
