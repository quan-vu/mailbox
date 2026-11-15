//! Policy for desktop notifications about new mail (MB-211). Pure: what to say, and when to stay
//! silent. Showing the notice is the job of a [`Notifier`] adapter.
//!
//! * Default is the count only ("Có N thư mới"). Sender and subject appear only when the user
//!   opted in. The message body never appears.
//! * One notice per batch, however many messages arrived.
//! * No notice for Sent, Drafts, Trash or Junk, nor for mail in the folder the user is looking at
//!   in a focused window, nor in Demo mode.
use crate::folders::FolderRole;
use crate::prefs::NotifyMode;

/// One new message, with only what a notice may use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMail {
    /// Role of its folder, if it has one.
    pub role: Option<FolderRole>,
    /// Key of its folder as the UI names it (role key such as `inbox`, or the folder id).
    pub folder_key: String,
    pub from_display: String,
    pub from_address: String,
    pub subject: String,
}

/// What the user is looking at right now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Viewing {
    pub window_focused: bool,
    /// Folder shown in the mail list, if the list is on screen.
    pub folder_key: Option<String>,
    /// Account whose mail is listed; `None` = all accounts (unified view).
    pub account_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    pub count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyError {
    /// The system refused or could not show it (permission, unsigned app, no service).
    Unavailable,
}

pub trait Notifier {
    fn show(&self, notice: &Notice, sound: bool) -> Result<(), NotifyError>;
}

fn silent_role(role: Option<FolderRole>) -> bool {
    matches!(
        role,
        Some(FolderRole::Sent | FolderRole::Drafts | FolderRole::Trash | FolderRole::Junk)
    )
}

/// Text for a notice: one line, no control characters, shortened.
pub fn clean(text: &str, max: usize) -> String {
    let one_line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let collapsed = one_line.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        collapsed
    } else {
        let cut: String = collapsed.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

fn sender(m: &NewMail) -> String {
    let name = clean(&m.from_display, 60);
    if name.is_empty() {
        clean(&m.from_address, 60)
    } else {
        name
    }
}

/// The notice for a batch of new mail, or `None` when nothing should be shown.
pub fn plan(
    mode: NotifyMode,
    demo: bool,
    account_id: &str,
    new: &[NewMail],
    viewing: &Viewing,
) -> Option<Notice> {
    if demo || mode == NotifyMode::Off {
        return None;
    }
    let looking_here = |m: &NewMail| {
        viewing.window_focused
            && viewing.folder_key.as_deref() == Some(m.folder_key.as_str())
            && viewing
                .account_id
                .as_deref()
                .is_none_or(|a| a == account_id)
    };
    let shown: Vec<&NewMail> = new
        .iter()
        .filter(|m| !silent_role(m.role) && !looking_here(m))
        .collect();
    let count = shown.len();
    let last = shown.first()?; // callers pass newest first
    let notice = match (mode, count) {
        (NotifyMode::SenderAndSubject, 1) => Notice {
            title: sender(last),
            body: clean(&last.subject, 100),
            count,
        },
        (NotifyMode::SenderAndSubject, n) => Notice {
            title: format!("Có {n} thư mới"),
            body: format!("Mới nhất từ {}: {}", sender(last), clean(&last.subject, 80)),
            count,
        },
        (_, 1) => Notice {
            title: "MailBox".into(),
            body: "Có 1 thư mới".into(),
            count,
        },
        (_, n) => Notice {
            title: "MailBox".into(),
            body: format!("Có {n} thư mới"),
            count,
        },
    };
    Some(notice)
}

/// Plan and show. Returns whether a notice was shown.
pub fn deliver(
    notifier: &impl Notifier,
    mode: NotifyMode,
    sound: bool,
    account_id: &str,
    new: &[NewMail],
    viewing: &Viewing,
) -> bool {
    match plan(mode, false, account_id, new, viewing) {
        Some(notice) => notifier.show(&notice, sound).is_ok(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn mail(role: Option<FolderRole>, key: &str, from: &str, subject: &str) -> NewMail {
        NewMail {
            role,
            folder_key: key.into(),
            from_display: from.into(),
            from_address: "linh@example.com".into(),
            subject: subject.into(),
        }
    }

    fn inbox(from: &str, subject: &str) -> NewMail {
        mail(Some(FolderRole::Inbox), "inbox", from, subject)
    }

    fn away() -> Viewing {
        Viewing::default()
    }

    #[derive(Default)]
    struct Fake {
        shown: RefCell<Vec<(Notice, bool)>>,
        fail: bool,
    }

    impl Notifier for Fake {
        fn show(&self, n: &Notice, sound: bool) -> Result<(), NotifyError> {
            if self.fail {
                return Err(NotifyError::Unavailable);
            }
            self.shown.borrow_mut().push((n.clone(), sound));
            Ok(())
        }
    }

    #[test]
    fn default_mode_shows_the_count_and_nothing_personal() {
        let n = plan(
            NotifyMode::CountOnly,
            false,
            "a",
            &[inbox("Trần Linh", "Hợp đồng bí mật")],
            &away(),
        )
        .unwrap();
        assert_eq!(
            (n.title.as_str(), n.body.as_str()),
            ("MailBox", "Có 1 thư mới")
        );
        let many: Vec<_> = (0..5)
            .map(|i| inbox("Trần Linh", &format!("S{i}")))
            .collect();
        let n = plan(NotifyMode::CountOnly, false, "a", &many, &away()).unwrap();
        assert_eq!(n.body, "Có 5 thư mới");
        for text in [&n.title, &n.body] {
            assert!(!text.contains("Linh") && !text.contains("example.com") && !text.contains('S'));
        }
    }

    #[test]
    fn sender_and_subject_only_on_opt_in_and_never_the_body() {
        let one = plan(
            NotifyMode::SenderAndSubject,
            false,
            "a",
            &[inbox("Trần Linh", "Báo giá")],
            &away(),
        )
        .unwrap();
        assert_eq!(
            (one.title.as_str(), one.body.as_str()),
            ("Trần Linh", "Báo giá")
        );
        let two = plan(
            NotifyMode::SenderAndSubject,
            false,
            "a",
            &[inbox("A", "Một"), inbox("B", "Hai")],
            &away(),
        )
        .unwrap();
        assert_eq!(two.title, "Có 2 thư mới");
        assert!(two.body.contains("A") && two.body.contains("Một"));
        // No field exists for the body; this guards the type from growing one by accident.
        let fields = format!("{:?}", inbox("x", "y"));
        assert!(!fields.contains("body"));
    }

    #[test]
    fn off_and_demo_are_silent() {
        assert!(plan(NotifyMode::Off, false, "a", &[inbox("x", "y")], &away()).is_none());
        assert!(
            plan(
                NotifyMode::SenderAndSubject,
                true,
                "a",
                &[inbox("x", "y")],
                &away()
            )
            .is_none()
        );
    }

    #[test]
    fn sent_drafts_trash_junk_never_notify() {
        for role in [
            FolderRole::Sent,
            FolderRole::Drafts,
            FolderRole::Trash,
            FolderRole::Junk,
        ] {
            let m = mail(Some(role), role.key(), "x", "y");
            assert!(
                plan(NotifyMode::CountOnly, false, "a", &[m], &away()).is_none(),
                "{role:?}"
            );
        }
        // Archive and user folders are not in the silent list.
        assert!(
            plan(
                NotifyMode::CountOnly,
                false,
                "a",
                &[mail(None, "folder-9", "x", "y")],
                &away()
            )
            .is_some()
        );
        // A burst of Sent + Inbox counts only the inbox mail.
        let n = plan(
            NotifyMode::CountOnly,
            false,
            "a",
            &[
                inbox("x", "y"),
                mail(Some(FolderRole::Sent), "sent", "x", "y"),
            ],
            &away(),
        )
        .unwrap();
        assert_eq!(n.count, 1);
    }

    #[test]
    fn mail_in_the_folder_being_looked_at_is_silent_only_when_focused() {
        let looking = Viewing {
            window_focused: true,
            folder_key: Some("inbox".into()),
            account_id: None,
        };
        assert!(
            plan(
                NotifyMode::CountOnly,
                false,
                "a",
                &[inbox("x", "y")],
                &looking
            )
            .is_none()
        );
        let unfocused = Viewing {
            window_focused: false,
            ..looking.clone()
        };
        assert!(
            plan(
                NotifyMode::CountOnly,
                false,
                "a",
                &[inbox("x", "y")],
                &unfocused
            )
            .is_some()
        );
        let other_folder = Viewing {
            folder_key: Some("starred".into()),
            ..looking.clone()
        };
        assert!(
            plan(
                NotifyMode::CountOnly,
                false,
                "a",
                &[inbox("x", "y")],
                &other_folder
            )
            .is_some()
        );
        let other_account = Viewing {
            account_id: Some("b".into()),
            ..looking
        };
        assert!(
            plan(
                NotifyMode::CountOnly,
                false,
                "a",
                &[inbox("x", "y")],
                &other_account
            )
            .is_some()
        );
    }

    #[test]
    fn text_is_one_short_clean_line() {
        let n = plan(
            NotifyMode::SenderAndSubject,
            false,
            "a",
            &[inbox("A\r\nB\u{7}", &format!("dòng\n{}", "x".repeat(300)))],
            &away(),
        )
        .unwrap();
        assert_eq!(n.title, "A B");
        assert!(!n.body.contains('\n') && n.body.chars().count() <= 100);
        assert!(n.body.ends_with('…'));
        let from_address_only = NewMail {
            from_display: "  ".into(),
            ..inbox("", "s")
        };
        let n = plan(
            NotifyMode::SenderAndSubject,
            false,
            "a",
            &[from_address_only],
            &away(),
        )
        .unwrap();
        assert_eq!(n.title, "linh@example.com");
    }

    #[test]
    fn a_burst_gives_exactly_one_notification() {
        let f = Fake::default();
        let burst: Vec<_> = (0..40).map(|i| inbox("x", &format!("{i}"))).collect();
        assert!(deliver(
            &f,
            NotifyMode::CountOnly,
            true,
            "a",
            &burst,
            &away()
        ));
        assert_eq!(f.shown.borrow().len(), 1);
        assert_eq!(f.shown.borrow()[0].0.count, 40);
        assert!(f.shown.borrow()[0].1, "sound flag is passed through");
    }

    #[test]
    fn nothing_new_or_refusal_by_the_system_reports_false() {
        let f = Fake::default();
        assert!(!deliver(
            &f,
            NotifyMode::CountOnly,
            false,
            "a",
            &[],
            &away()
        ));
        assert!(f.shown.borrow().is_empty());
        let refusing = Fake {
            fail: true,
            ..Default::default()
        };
        assert!(!deliver(
            &refusing,
            NotifyMode::CountOnly,
            false,
            "a",
            &[inbox("x", "y")],
            &away()
        ));
    }
}
