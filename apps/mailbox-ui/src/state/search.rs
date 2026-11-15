//! Real-mode message list: conversations, local search and "load older" (MB-200, MB-201).
//!
//! Demo mode keeps its in-memory list and search exactly as before. In Real mode:
//! - the list shows one row per conversation of the current folder view: the newest message of
//!   the conversation that is in this view, with the conversation's size across folders;
//! - a non-empty query runs a local search in the store (debounced) with a scope switch, and the
//!   filter chips are part of that query;
//! - "load older" at the end of the list extends the cache view, then fetches from the server.
// Real mode has no data source on the Web build yet, so parts of this are native-only.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]
use super::AppState;
use super::folders::{RealState, RealView};
use super::mail::{Filter, Folder, MailState, ThreadRow};
use crate::i18n::{bi, tr};
use dioxus::prelude::*;
use mailbox_contracts::MessageSummary;
use mailbox_core::search::{fold, terms};
use std::collections::HashMap;

/// Where a search looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchScope {
    /// The folder view shown (unified inbox, a role, or one folder).
    #[default]
    View,
    /// Every folder of the selected account.
    Account,
    /// Every folder of every account.
    All,
}

impl SearchScope {
    pub const ALL: [SearchScope; 3] = [SearchScope::View, SearchScope::Account, SearchScope::All];

    pub fn key(self) -> &'static str {
        match self {
            SearchScope::View => "view",
            SearchScope::Account => "account",
            SearchScope::All => "all",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SearchScope::View => tr("Thư mục này"),
            SearchScope::Account => tr("Tài khoản này"),
            SearchScope::All => tr("Tất cả tài khoản"),
        }
    }
}

/// Outcome of the last search, tagged with the request it answers.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub request: String,
    pub messages: Vec<MessageSummary>,
    pub total: u64,
    pub failed: bool,
}

/// State of the "load older" control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Older {
    #[default]
    Idle,
    Loading,
    /// The view is cached down to the first message of every folder.
    AtStart,
    /// The server could not be reached; cached mail is shown.
    Offline,
}

#[derive(Clone, Copy)]
pub struct SearchState {
    pub scope: Signal<SearchScope>,
    pub result: Signal<Option<SearchResult>>,
    pub busy: Signal<bool>,
    generation: Signal<u64>,
    pub older: Signal<Older>,
}

impl SearchState {
    pub fn init() -> Self {
        Self {
            scope: Signal::new(SearchScope::default()),
            result: Signal::new(None),
            busy: Signal::new(false),
            generation: Signal::new(0),
            older: Signal::new(Older::default()),
        }
    }

    /// The state provided by `App`, if any (component tests may render without it).
    pub fn get() -> Option<Self> {
        try_consume_context::<SearchState>()
    }
}

/// A real-mode message belongs in the list for this filter and folder view.
pub fn in_view(m: &MessageSummary, f: &Filter, view: &RealView) -> bool {
    let place = if f.folder == Folder::Starred {
        m.starred && m.folder_id != "trash" && m.folder_id != "junk"
    } else {
        view.matches(&m.folder_id)
    };
    place
        && f.account.as_deref().is_none_or(|a| m.account_id == a)
        && (!f.unread_only || m.unread)
        && (!f.attachments_only || !m.attachments.is_empty())
        && (!f.today_only || m.received_at.contains(':'))
}

/// One row per conversation among `messages` (newest first) that pass `keep`. The row's head is
/// the newest kept message; its count is the conversation size across folders when known.
pub fn group_rows(
    messages: &[MessageSummary],
    keep: impl Fn(&MessageSummary) -> bool,
    thread_of: impl Fn(&str) -> Option<String>,
    size_of: impl Fn(&str) -> Option<usize>,
) -> Vec<ThreadRow> {
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut rows: Vec<ThreadRow> = Vec::new();
    // Members of each row seen in this view (the count never shows fewer than that).
    let mut members: Vec<usize> = Vec::new();
    for m in messages.iter().filter(|m| keep(m)) {
        let key = thread_of(&m.id).unwrap_or_else(|| m.id.clone());
        match index.get(&key) {
            Some(&i) => {
                members[i] += 1;
                let row = &mut rows[i];
                row.count = row.count.max(members[i]);
                row.unread |= m.unread;
                row.starred |= m.starred;
                row.has_attachments |= !m.attachments.is_empty();
            }
            None => {
                index.insert(key.clone(), rows.len());
                members.push(1);
                rows.push(ThreadRow {
                    head: m.clone(),
                    count: size_of(&key).unwrap_or(1).max(1),
                    unread: m.unread,
                    starred: m.starred,
                    has_attachments: !m.attachments.is_empty(),
                });
            }
        }
    }
    rows
}

/// Thread id of a message (Desktop keeps them in the history adapter).
pub fn thread_of(message_id: &str) -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::adapters::history::thread_of(message_id)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = message_id;
        None
    }
}

fn size_of(thread_id: &str) -> Option<usize> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::adapters::history::size_of(thread_id)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = thread_id;
        None
    }
}

fn current_view() -> RealView {
    RealState::get()
        .map(|r| r.view.read().clone())
        .unwrap_or_default()
}

/// A stable text for "this search request", so a late answer to an old query is ignored.
pub(crate) fn request_key(f: &Filter, scope: SearchScope, view: &RealView) -> String {
    format!(
        "{}\u{1}{}\u{1}{:?}\u{1}{:?}\u{1}{}{}{}\u{1}{:?}",
        terms(&f.query).join(" "),
        scope.key(),
        view,
        f.account,
        f.unread_only,
        f.attachments_only,
        f.today_only,
        f.folder
    )
}

/// Rows of the message list. Demo mode: unchanged in-memory behaviour.
pub fn list_rows(app: AppState, f: &Filter, mail: &MailState) -> Vec<ThreadRow> {
    if !app.is_real() {
        return super::folders::rows_in_view(app, f.folder, mail.visible(f));
    }
    let view = current_view();
    if searching(f) {
        let Some(state) = SearchState::get() else {
            return Vec::new();
        };
        let scope = *state.scope.read();
        let key = request_key(f, scope, &view);
        return match &*state.result.read() {
            Some(r) if r.request == key => search_rows(&r.messages),
            _ => Vec::new(),
        };
    }
    group_rows(
        mail.messages(),
        |m| in_view(m, f, &view),
        thread_of,
        size_of,
    )
}

/// A search for the current filter is still running (or waiting out the debounce): the list
/// must not claim "no results" yet.
pub fn search_pending(f: &Filter) -> bool {
    let Some(state) = SearchState::get() else {
        return false;
    };
    let key = request_key(f, *state.scope.read(), &current_view());
    *state.busy.read()
        || state
            .result
            .read()
            .as_ref()
            .is_none_or(|r| r.request != key)
}

/// Search hits as rows: one per message, newest first.
pub fn search_rows(hits: &[MessageSummary]) -> Vec<ThreadRow> {
    hits.iter()
        .map(|m| ThreadRow {
            head: m.clone(),
            count: thread_of(&m.id).and_then(|t| size_of(&t)).unwrap_or(1),
            unread: m.unread,
            starred: m.starred,
            has_attachments: !m.attachments.is_empty(),
        })
        .collect()
}

/// A real search is shown instead of the folder list.
pub fn searching(f: &Filter) -> bool {
    !terms(&f.query).is_empty()
}

/// Messages a conversation action applies to: the conversation's messages in the same folder as
/// `message_id` (a reply in Sent is not archived along with the inbox copy). Falls back to the
/// message alone, also for a search hit outside the loaded list.
pub fn same_folder_conversation(app: AppState, message_id: &str) -> Vec<(String, String)> {
    let mail = app.mail.peek();
    let hit = SearchState::get().and_then(|s| {
        s.result
            .peek()
            .as_ref()
            .and_then(|r| r.messages.iter().find(|m| m.id == message_id).cloned())
    });
    let Some(origin) = mail.message(message_id).cloned().or(hit) else {
        return Vec::new();
    };
    let Some(thread) = thread_of(message_id) else {
        return vec![(origin.id, origin.folder_id)];
    };
    let mut out: Vec<(String, String)> = mail
        .messages()
        .iter()
        .filter(|m| m.folder_id == origin.folder_id && m.account_id == origin.account_id)
        .filter(|m| thread_of(&m.id).as_deref() == Some(thread.as_str()))
        .map(|m| (m.id.clone(), m.folder_id.clone()))
        .collect();
    if !out.iter().any(|(id, _)| id == message_id) {
        out.push((origin.id, origin.folder_id));
    }
    out
}

/// Split `text` into (segment, is_match) pieces for the query's words, matching folded text so
/// "hoa don" highlights "Hóa đơn". Output is plain text for the renderer, never markup.
pub fn highlight(text: &str, query: &str) -> Vec<(String, bool)> {
    let words = terms(query);
    if words.is_empty() || text.is_empty() {
        return vec![(text.to_string(), false)];
    }
    // Fold char by char, remembering which original char each folded byte came from.
    let chars: Vec<char> = text.chars().collect();
    let mut folded = String::new();
    let mut origin: Vec<usize> = Vec::new();
    for (i, c) in chars.iter().enumerate() {
        let f = fold(&c.to_string());
        let f = if f.is_empty() && c.is_whitespace() {
            " ".to_string()
        } else {
            f
        };
        for _ in 0..f.len() {
            origin.push(i);
        }
        folded.push_str(&f);
    }
    let mut marked = vec![false; chars.len()];
    for w in &words {
        let mut from = 0;
        while let Some(pos) = folded[from..].find(w.as_str()) {
            let start = from + pos;
            let end = start + w.len();
            for flag in marked
                .iter_mut()
                .take(origin[end - 1] + 1)
                .skip(origin[start])
            {
                *flag = true;
            }
            from = end;
        }
    }
    let mut out: Vec<(String, bool)> = Vec::new();
    for (c, m) in chars.into_iter().zip(marked) {
        match out.last_mut() {
            Some((s, last)) if *last == m => s.push(c),
            _ => out.push((c.to_string(), m)),
        }
    }
    out
}

/// Store view for the current real folder view and account filter.
#[cfg(not(target_arch = "wasm32"))]
fn view_spec(f: &Filter, view: &RealView) -> crate::adapters::history::ViewSpec {
    use crate::adapters::history::ViewSpec;
    match view {
        RealView::Inbox => ViewSpec::Role {
            role: "inbox".into(),
            account: f.account.clone(),
        },
        RealView::Role(role) => ViewSpec::Role {
            role: role.clone(),
            account: f.account.clone(),
        },
        RealView::Folder(id) => ViewSpec::Folder(id.clone()),
    }
}

/// The account "this account" means: the account filter, else the folder's account.
pub fn scope_account(app: AppState, f: &Filter, view: &RealView) -> Option<String> {
    if let Some(a) = &f.account {
        return Some(a.clone());
    }
    if let RealView::Folder(id) = view {
        return RealState::get().and_then(|r| {
            r.folders
                .read()
                .iter()
                .find(|x| &x.folder_id == id)
                .map(|x| x.account_id.clone())
        });
    }
    let mail = app.mail.read();
    (mail.accounts.len() == 1).then(|| mail.accounts[0].id.clone())
}

/// Start (debounced) the search for the current filter; called from an effect on every change.
pub fn drive_search(app: AppState) {
    let Some(state) = SearchState::get() else {
        return;
    };
    let f = app.filter.read().clone();
    let view = current_view();
    let scope = *state.scope.read();
    // Re-run after any reload of the mail cache (an action or a sync may change results).
    let _ = app.mail.read().accounts.len();
    let (mut generation, mut result, mut busy) = (state.generation, state.result, state.busy);
    let next = *generation.peek() + 1;
    generation.set(next);
    if !app.is_real() || !searching(&f) {
        busy.set(false);
        if result.peek().is_some() {
            result.set(None);
        }
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use crate::adapters::history::{SearchIn, search};
        use mailbox_core::search::Narrowing;
        let request = request_key(&f, scope, &view);
        let target = match scope {
            SearchScope::View if f.folder == Folder::Starred => SearchIn::Everywhere,
            SearchScope::View => SearchIn::View(view_spec(&f, &view)),
            SearchScope::Account => match scope_account(app, &f, &view) {
                Some(a) => SearchIn::Account(a),
                None => SearchIn::Everywhere,
            },
            SearchScope::All => SearchIn::Everywhere,
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let narrowing = Narrowing {
            unread_only: f.unread_only,
            attachments_only: f.attachments_only,
            // Times are shown in UTC, so "today" is the UTC day.
            since: f.today_only.then_some(now - now.rem_euclid(86_400)),
            account_id: f.account.clone(),
        };
        let text = f.query.clone();
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            if *generation.peek() != next {
                return;
            }
            busy.set(true);
            let found = search(text, target, narrowing).await;
            if *generation.peek() != next {
                return;
            }
            busy.set(false);
            result.set(Some(match found {
                Ok(found) => SearchResult {
                    request,
                    messages: found.messages,
                    total: found.total,
                    failed: false,
                },
                Err(_) => SearchResult {
                    request,
                    messages: Vec::new(),
                    total: 0,
                    failed: true,
                },
            }));
        });
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (scope, view);
}

/// Clear the query and any result.
pub fn clear(app: AppState) {
    let mut filter = app.filter;
    filter.write().query.clear();
    if let Some(state) = SearchState::get() {
        let mut result = state.result;
        result.set(None);
    }
}

/// "Load older": extend the list from the cache, then from the server; reload the list after.
pub fn load_older(app: AppState) {
    let Some(state) = SearchState::get() else {
        return;
    };
    let mut older = state.older;
    if *older.peek() == Older::Loading {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let f = app.filter.peek().clone();
        let view = RealState::get()
            .map(|r| r.view.peek().clone())
            .unwrap_or_default();
        let spec = view_spec(&f, &view);
        older.set(Older::Loading);
        spawn(async move {
            match crate::adapters::history::load_older(spec).await {
                Ok(done) => {
                    reload_keeping_selection(app).await;
                    older.set(if done.at_start {
                        Older::AtStart
                    } else if done.offline && done.shown == 0 {
                        Older::Offline
                    } else {
                        Older::Idle
                    });
                    if done.fetched > 0 {
                        app.notify(
                            bi(
                                format!("Đã tải thêm {} thư cũ từ máy chủ.", done.fetched),
                                format!(
                                    "Loaded {} more older message{} from the server.",
                                    done.fetched,
                                    crate::i18n::s(done.fetched)
                                ),
                            ),
                            false,
                        );
                    }
                }
                Err(_) => {
                    older.set(Older::Offline);
                    app.notify(tr("Không tải được thư cũ hơn. Thử lại sau."), false);
                }
            }
        });
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (app, &mut older);
}

/// Reload the snapshot without touching the selection or the draft.
#[cfg(not(target_arch = "wasm32"))]
async fn reload_keeping_selection(app: AppState) {
    if let Ok(data) = crate::adapters::account::load_real().await
        && app.is_real()
    {
        let mut mail = app.mail;
        mail.set(MailState::new(data.accounts, data.messages));
    }
}

/// A different folder view starts with the "load older" control ready again.
pub fn reset_older() {
    if let Some(state) = SearchState::get() {
        let mut older = state.older;
        if *older.peek() != Older::Idle {
            older.set(Older::Idle);
        }
    }
}

/// The conversation of a message for the thread screen (Real mode, Desktop), oldest first.
/// `None` while loading or in Demo mode.
pub fn use_conversation(app: AppState, message_id: String) -> Option<Vec<MessageSummary>> {
    let real = app.is_real();
    let res = use_resource(use_reactive!(|(message_id, real)| async move {
        if !real {
            return None;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            crate::adapters::history::conversation_of(&message_id)
                .await
                .ok()
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = message_id;
            None
        }
    }));
    res.read().clone().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailbox_contracts::fixtures;

    fn m(id: &str, folder: &str, unread: bool) -> MessageSummary {
        let mut x = fixtures::bootstrap().messages.remove(0);
        x.id = id.into();
        x.thread_id = String::new();
        x.folder_id = folder.into();
        x.account_id = "acc".into();
        x.unread = unread;
        x.starred = false;
        x.attachments.clear();
        x.received_at = "01/11".into();
        x
    }

    fn threads(id: &str) -> Option<String> {
        match id {
            "r1" | "s1" | "r0" => Some("acc:t1".into()),
            "r2" => Some("acc:t2".into()),
            _ => None,
        }
    }

    #[test]
    fn a_conversation_is_one_row_headed_by_its_newest_message_in_the_view() {
        // Newest first: a Sent reply (s1) is newer than the inbox message r1.
        let list = vec![
            m("s1", "sent", false),
            m("r1", "inbox", false),
            m("r2", "inbox", true),
            m("r0", "inbox", true),
            m("x", "inbox", false),
        ];
        let f = Filter::default();
        let rows = group_rows(
            &list,
            |x| in_view(x, &f, &RealView::Inbox),
            threads,
            |t| (t == "acc:t1").then_some(3),
        );
        let heads: Vec<&str> = rows.iter().map(|r| r.head.id.as_str()).collect();
        assert_eq!(heads, vec!["r1", "r2", "x"]);
        assert_eq!(rows[0].count, 3, "size across folders, Sent reply included");
        assert!(
            rows[0].unread,
            "an unread older message makes the row unread"
        );
        assert_eq!(rows[1].count, 1);
        // The Sent view shows the same conversation headed by the reply.
        let sent = group_rows(
            &list,
            |x| in_view(x, &f, &RealView::Role("sent".into())),
            threads,
            |_| Some(3),
        );
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].head.id, "s1");
    }

    #[test]
    fn unread_and_account_narrowing_apply_per_message() {
        let list = vec![m("r1", "inbox", false), m("r2", "inbox", true)];
        let f = Filter {
            unread_only: true,
            ..Filter::default()
        };
        let rows = group_rows(
            &list,
            |x| in_view(x, &f, &RealView::Inbox),
            threads,
            |_| None,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].head.id, "r2");
        let f = Filter {
            account: Some("other".into()),
            ..Filter::default()
        };
        assert!(
            group_rows(
                &list,
                |x| in_view(x, &f, &RealView::Inbox),
                threads,
                |_| None
            )
            .is_empty()
        );
    }

    #[test]
    fn highlights_match_folded_text_and_stay_plain_text() {
        assert_eq!(
            highlight("Hóa đơn tháng 11", "hoa don"),
            vec![
                ("Hóa".to_string(), true),
                (" ".to_string(), false),
                ("đơn".to_string(), true),
                (" tháng 11".to_string(), false)
            ]
        );
        assert_eq!(
            highlight("<b>x</b> HOA", "hoa"),
            vec![("<b>x</b> ".to_string(), false), ("HOA".to_string(), true)]
        );
        assert_eq!(highlight("abc", ""), vec![("abc".to_string(), false)]);
        assert_eq!(highlight("", "x"), vec![(String::new(), false)]);
    }

    #[test]
    fn a_query_of_only_syntax_is_not_a_search() {
        assert!(!searching(&Filter {
            query: "  %% \"\" ".into(),
            ..Filter::default()
        }));
        assert!(searching(&Filter {
            query: "hóa".into(),
            ..Filter::default()
        }));
    }
}
