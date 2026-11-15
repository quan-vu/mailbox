//! Pure, renderer-independent mailbox view state for the demo UI.
//! All mutations are local to the session; nothing is sent to a mail server.
use crate::i18n::tr;
use mailbox_contracts::{AccountSummary, MessageSummary};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Folder {
    #[default]
    Inbox,
    Starred,
    Sent,
    Drafts,
    Scheduled,
    Archive,
    Trash,
}

impl Folder {
    pub const ALL: [Folder; 7] = [
        Folder::Inbox,
        Folder::Starred,
        Folder::Sent,
        Folder::Drafts,
        Folder::Scheduled,
        Folder::Archive,
        Folder::Trash,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Folder::Inbox => "inbox",
            Folder::Starred => "starred",
            Folder::Sent => "sent",
            Folder::Drafts => "drafts",
            Folder::Scheduled => "scheduled",
            Folder::Archive => "archive",
            Folder::Trash => "trash",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Folder::Inbox => tr("Hộp thư hợp nhất"),
            Folder::Starred => tr("Thư gắn sao"),
            Folder::Sent => tr("Đã gửi"),
            Folder::Drafts => tr("Bản nháp"),
            Folder::Scheduled => tr("Đã lên lịch"),
            Folder::Archive => tr("Lưu trữ"),
            Folder::Trash => tr("Thùng rác"),
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Folder::Inbox => "inbox",
            Folder::Starred => "star",
            Folder::Sent => "send",
            Folder::Drafts => "file",
            Folder::Scheduled => "clock",
            Folder::Archive => "archive",
            Folder::Trash => "trash",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Filter {
    pub folder: Folder,
    /// `None` = all accounts (unified view).
    pub account: Option<String>,
    pub unread_only: bool,
    pub attachments_only: bool,
    pub today_only: bool,
    pub query: String,
}

impl Filter {
    pub fn has_narrowing(&self) -> bool {
        self.unread_only
            || self.attachments_only
            || self.today_only
            || !self.query.trim().is_empty()
    }

    pub fn clear_narrowing(&mut self) {
        self.unread_only = false;
        self.attachments_only = false;
        self.today_only = false;
        self.query.clear();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Inbox,
    Archive,
    Trash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoAction {
    pub thread: String,
    pub previous: Placement,
    pub label: &'static str,
}

/// One row in the message list: a conversation represented by its first message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadRow {
    pub head: MessageSummary,
    pub count: usize,
    pub unread: bool,
    pub starred: bool,
    pub has_attachments: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MailState {
    pub accounts: Vec<AccountSummary>,
    messages: Vec<MessageSummary>,
    archived: BTreeSet<String>,
    trashed: BTreeSet<String>,
    pub last_undo: Option<UndoAction>,
}

/// A demo "today" marker: fixture times shown as HH:MM are today, labels like "Hôm qua" are not.
fn is_today(m: &MessageSummary) -> bool {
    m.received_at.contains(':')
}

impl MailState {
    pub fn new(accounts: Vec<AccountSummary>, messages: Vec<MessageSummary>) -> Self {
        Self {
            accounts,
            messages,
            archived: BTreeSet::new(),
            trashed: BTreeSet::new(),
            last_undo: None,
        }
    }

    pub fn account(&self, id: &str) -> Option<&AccountSummary> {
        self.accounts.iter().find(|a| a.id == id)
    }

    /// Every loaded message, in snapshot order (Real mode: newest first).
    pub fn messages(&self) -> &[MessageSummary] {
        &self.messages
    }

    pub fn message(&self, id: &str) -> Option<&MessageSummary> {
        self.messages.iter().find(|m| m.id == id)
    }

    fn placement(&self, thread: &str) -> Placement {
        if self.trashed.contains(thread) {
            Placement::Trash
        } else if self.archived.contains(thread) {
            Placement::Archive
        } else {
            Placement::Inbox
        }
    }

    /// All messages of a conversation, in chronological fixture order.
    pub fn thread(&self, thread: &str) -> Vec<MessageSummary> {
        self.messages
            .iter()
            .filter(|m| m.thread_key() == thread)
            .cloned()
            .collect()
    }

    fn rows(&self) -> Vec<ThreadRow> {
        let mut seen: Vec<String> = Vec::new();
        let mut rows = Vec::new();
        for m in &self.messages {
            let key = m.thread_key().to_string();
            if seen.contains(&key) {
                continue;
            }
            seen.push(key.clone());
            let all = self.thread(&key);
            rows.push(ThreadRow {
                head: m.clone(),
                count: all.len(),
                unread: all.iter().any(|x| x.unread),
                starred: all.iter().any(|x| x.starred),
                has_attachments: all.iter().any(|x| !x.attachments.is_empty()),
            });
        }
        rows
    }

    /// Visible conversation rows for a filter. Unified view keeps account provenance on every row.
    pub fn visible(&self, f: &Filter) -> Vec<ThreadRow> {
        let query = f.query.trim().to_lowercase();
        self.rows()
            .into_iter()
            .filter(|r| {
                let key = r.head.thread_key();
                let placement = self.placement(key);
                let in_folder = match f.folder {
                    Folder::Inbox => placement == Placement::Inbox,
                    Folder::Starred => r.starred && placement != Placement::Trash,
                    Folder::Archive => placement == Placement::Archive,
                    Folder::Trash => placement == Placement::Trash,
                    // The demo has no sent/draft/scheduled data: these show an empty state.
                    Folder::Sent | Folder::Drafts | Folder::Scheduled => false,
                };
                in_folder
                    && f.account.as_deref().is_none_or(|a| r.head.account_id == a)
                    && (!f.unread_only || r.unread)
                    && (!f.attachments_only || r.has_attachments)
                    && (!f.today_only || is_today(&r.head))
                    && (query.is_empty() || {
                        let hay = format!(
                            "{} {} {} {}",
                            r.head.subject, r.head.from_name, r.head.from_address, r.head.preview
                        )
                        .to_lowercase();
                        hay.contains(&query)
                    })
            })
            .collect()
    }

    /// Unread conversations in the inbox, optionally for a single account.
    pub fn unread_count(&self, account: Option<&str>) -> usize {
        self.rows()
            .iter()
            .filter(|r| self.placement(r.head.thread_key()) == Placement::Inbox)
            .filter(|r| r.unread && account.is_none_or(|a| r.head.account_id == a))
            .count()
    }

    pub fn toggle_star(&mut self, message_id: &str) {
        if let Some(m) = self.messages.iter_mut().find(|m| m.id == message_id) {
            m.starred = !m.starred;
        }
    }

    pub fn set_thread_unread(&mut self, thread: &str, unread: bool) {
        for m in self
            .messages
            .iter_mut()
            .filter(|m| m.thread_key() == thread)
        {
            m.unread = unread;
        }
    }

    /// Opening a conversation marks it read (local demo state only).
    pub fn mark_read(&mut self, thread: &str) {
        self.set_thread_unread(thread, false);
    }

    fn move_to(&mut self, thread: &str, target: Placement, label: &'static str) {
        let previous = self.placement(thread);
        self.archived.remove(thread);
        self.trashed.remove(thread);
        match target {
            Placement::Archive => {
                self.archived.insert(thread.to_string());
            }
            Placement::Trash => {
                self.trashed.insert(thread.to_string());
            }
            Placement::Inbox => {}
        }
        self.last_undo = Some(UndoAction {
            thread: thread.to_string(),
            previous,
            label,
        });
    }

    pub fn archive(&mut self, thread: &str) {
        self.move_to(
            thread,
            Placement::Archive,
            tr("Đã lưu trữ hội thoại (demo)"),
        );
    }

    pub fn trash(&mut self, thread: &str) {
        self.move_to(
            thread,
            Placement::Trash,
            tr("Đã chuyển vào Thùng rác (demo)"),
        );
    }

    pub fn undo(&mut self) -> bool {
        let Some(action) = self.last_undo.take() else {
            return false;
        };
        self.archived.remove(&action.thread);
        self.trashed.remove(&action.thread);
        match action.previous {
            Placement::Archive => {
                self.archived.insert(action.thread);
            }
            Placement::Trash => {
                self.trashed.insert(action.thread);
            }
            Placement::Inbox => {}
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailbox_contracts::fixtures;

    fn state() -> MailState {
        let d = fixtures::bootstrap();
        MailState::new(d.accounts, d.messages)
    }

    fn ids(rows: &[ThreadRow]) -> Vec<String> {
        rows.iter().map(|r| r.head.id.clone()).collect()
    }

    #[test]
    fn unified_inbox_groups_conversations_into_one_row() {
        let s = state();
        let rows = s.visible(&Filter::default());
        assert_eq!(rows.len(), 9, "13 messages, thread t01 has 5");
        let head = &rows[0];
        assert_eq!(head.head.id, "m01");
        assert_eq!(head.count, 5);
        assert!(head.has_attachments);
    }

    #[test]
    fn account_filter_keeps_only_that_account() {
        let s = state();
        let f = Filter {
            account: Some("support".into()),
            ..Filter::default()
        };
        assert_eq!(ids(&s.visible(&f)), vec!["m07", "m09"]);
    }

    #[test]
    fn unread_attachment_and_today_filters_combine() {
        let s = state();
        let f = Filter {
            unread_only: true,
            attachments_only: true,
            ..Filter::default()
        };
        assert_eq!(ids(&s.visible(&f)), vec!["m01", "m03", "m04"]);
        let f = Filter {
            today_only: true,
            ..Filter::default()
        };
        assert!(
            s.visible(&f)
                .iter()
                .all(|r| r.head.received_at.contains(':'))
        );
    }

    #[test]
    fn search_matches_subject_sender_and_is_case_insensitive() {
        let s = state();
        let f = Filter {
            query: "HÓA ĐƠN".into(),
            ..Filter::default()
        };
        assert_eq!(ids(&s.visible(&f)), vec!["m04"]);
        let f = Filter {
            query: "greentech".into(),
            ..Filter::default()
        };
        assert_eq!(ids(&s.visible(&f)), vec!["m07"]);
        let f = Filter {
            query: "không tồn tại".into(),
            ..Filter::default()
        };
        assert!(s.visible(&f).is_empty());
    }

    #[test]
    fn archive_moves_thread_and_undo_restores_it() {
        let mut s = state();
        s.archive("t01");
        assert!(!ids(&s.visible(&Filter::default())).contains(&"m01".to_string()));
        let archive = Filter {
            folder: Folder::Archive,
            ..Filter::default()
        };
        assert_eq!(ids(&s.visible(&archive)), vec!["m01"]);
        assert!(s.undo());
        assert!(ids(&s.visible(&Filter::default())).contains(&"m01".to_string()));
        assert!(!s.undo(), "undo is single-step");
    }

    #[test]
    fn trash_hides_from_starred_view() {
        let mut s = state();
        let starred = Filter {
            folder: Folder::Starred,
            ..Filter::default()
        };
        assert_eq!(ids(&s.visible(&starred)), vec!["m01", "m07"]);
        s.trash("m07");
        assert_eq!(ids(&s.visible(&starred)), vec!["m01"]);
        let trash = Filter {
            folder: Folder::Trash,
            ..Filter::default()
        };
        assert_eq!(ids(&s.visible(&trash)), vec!["m07"]);
    }

    #[test]
    fn star_and_read_state_update_counts() {
        let mut s = state();
        assert_eq!(s.unread_count(None), 5);
        assert_eq!(s.unread_count(Some("work")), 4);
        s.mark_read("t01");
        assert_eq!(s.unread_count(Some("work")), 3);
        s.set_thread_unread("m06", true);
        assert_eq!(s.unread_count(Some("personal")), 2);
        s.toggle_star("m02");
        assert!(s.message("m02").unwrap().starred);
    }

    #[test]
    fn sent_drafts_scheduled_are_empty_in_demo() {
        let s = state();
        for folder in [Folder::Sent, Folder::Drafts, Folder::Scheduled] {
            assert!(
                s.visible(&Filter {
                    folder,
                    ..Filter::default()
                })
                .is_empty()
            );
        }
    }
}
