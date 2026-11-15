//! Paging and local search of the envelope cache (MB-200).
//!
//! Search input is data: the store binds it as parameters and, for SQLite FTS5, quotes every
//! term as a phrase so query syntax characters lose their meaning. Text is folded the same way
//! on indexing and on query (lower case, diacritics removed, `đ` → `d`), so Vietnamese matches
//! with and without accents. Search terms are never logged.
use crate::MailboxError;
use crate::store::MessageRecord;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Longest query accepted, in characters; the rest is ignored.
pub const MAX_QUERY_CHARS: usize = 200;
/// At most this many words of a query are used.
pub const MAX_TERMS: usize = 8;
/// Longest single word used, in characters.
pub const MAX_TERM_CHARS: usize = 64;
/// Largest page of results the store returns.
pub const MAX_PAGE: u32 = 500;
/// Folded body text kept in the search index per message, in characters.
pub const MAX_INDEXED_BODY_CHARS: usize = 32_000;

/// Lower case, without diacritics, whitespace collapsed. Used on both index and query side.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.nfd() {
        if is_combining_mark(c) {
            continue;
        }
        let c = match c {
            'đ' | 'Đ' => 'd',
            'ð' | 'Ð' => 'd',
            'ø' | 'Ø' => 'o',
            'ł' | 'Ł' => 'l',
            'ß' => 's',
            _ => c,
        };
        if c.is_whitespace() || c.is_control() {
            space = !out.is_empty();
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        out.extend(c.to_lowercase());
    }
    out
}

/// Folded search words of a user query, bounded in number and length. Characters that are
/// neither letters nor digits split words, so query syntax can never reach the database as such.
pub fn terms(query: &str) -> Vec<String> {
    let capped: String = query.chars().take(MAX_QUERY_CHARS).collect();
    let folded = fold(&capped);
    let mut out: Vec<String> = Vec::new();
    for word in folded
        .split(|c: char| !(c.is_alphanumeric() || c == '@' || c == '.' || c == '_' || c == '-'))
    {
        let word: String = word
            .trim_matches(|c: char| !c.is_alphanumeric())
            .chars()
            .take(MAX_TERM_CHARS)
            .collect();
        if !word.is_empty() && !out.contains(&word) {
            out.push(word);
        }
        if out.len() == MAX_TERMS {
            break;
        }
    }
    out
}

/// One FTS5 phrase per term, quoted (inner quotes doubled) and prefix-matched, AND-combined.
/// Only ever built from [`terms`] output; bound as a parameter, never spliced into SQL.
pub fn fts_match(terms: &[String]) -> String {
    terms
        .iter()
        .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// `LIKE` pattern for a term with `%`, `_` and the escape character itself escaped (`ESCAPE '\'`).
pub fn like_pattern(term: &str) -> String {
    let mut p = String::with_capacity(term.len() + 2);
    p.push('%');
    for c in term.chars() {
        if matches!(c, '%' | '_' | '\\') {
            p.push('\\');
        }
        p.push(c);
    }
    p.push('%');
    p
}

/// Which messages a list or search covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// One folder of one account.
    Folder {
        account_id: String,
        folder_id: String,
    },
    /// Every folder with this role (`inbox`, `sent`, ...), of one account or of all.
    Role {
        role: String,
        account_id: Option<String>,
    },
    /// Every folder of one account.
    Account(String),
    /// Everything cached for the owner.
    All,
}

/// Keyset position in a list ordered newest first (`internal_date DESC, id ASC`). Stable when new
/// mail arrives, unlike an offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageKey {
    pub internal_date: i64,
    pub id: String,
}

impl PageKey {
    pub fn of(m: &MessageRecord) -> Self {
        Self {
            internal_date: m.internal_date,
            id: m.id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Narrowing {
    pub unread_only: bool,
    /// Messages whose downloaded body has a non-inline attachment.
    pub attachments_only: bool,
    /// Only messages received at or after this Unix time (e.g. start of today).
    pub since: Option<i64>,
    /// Only this account.
    pub account_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub text: String,
    pub scope: Scope,
    pub narrowing: Narrowing,
    pub limit: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchEngine {
    /// SQLite FTS5 over folded text.
    Fts5,
    /// Substring match over folded text (`LIKE` on SQLite, `ILIKE` on PostgreSQL).
    Like,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub message: MessageRecord,
    pub thread_id: Option<String>,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPage {
    /// Newest first, at most `limit`.
    pub hits: Vec<SearchHit>,
    /// All matches, beyond `limit`.
    pub total: u64,
    pub engine: SearchEngine,
}

/// Keyset paging of the cache. Implemented by `mailbox-store` for SQLite and PostgreSQL.
#[allow(async_fn_in_trait)]
pub trait PageStore {
    /// Up to `limit` messages of `scope` older than `before` (or the newest when `None`),
    /// newest first.
    async fn page(
        &self,
        owner_id: &str,
        scope: &Scope,
        narrowing: &Narrowing,
        before: Option<&PageKey>,
        limit: u32,
    ) -> Result<Vec<MessageRecord>, MailboxError>;
    /// Every message of `scope` at or newer than `floor`, newest first, at most `cap`.
    async fn since(
        &self,
        owner_id: &str,
        scope: &Scope,
        floor: &PageKey,
        cap: u32,
    ) -> Result<Vec<MessageRecord>, MailboxError>;
    /// Lowest cached UID of a folder under one UIDVALIDITY.
    async fn lowest_uid(
        &self,
        owner_id: &str,
        account_id: &str,
        folder_id: &str,
        uid_validity: u64,
    ) -> Result<Option<u32>, MailboxError>;
}

/// Local search over cached envelopes and downloaded body text.
#[allow(async_fn_in_trait)]
pub trait SearchStore {
    /// Bring the folded search index up to date for at most `max` messages (new messages,
    /// changed headers, newly downloaded bodies). Returns how many rows were indexed.
    async fn refresh_search_index(&self, owner_id: &str, max: u32) -> Result<usize, MailboxError>;
    async fn search(&self, owner_id: &str, query: &SearchQuery)
    -> Result<SearchPage, MailboxError>;
    fn search_engine(&self) -> SearchEngine;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vietnamese_folds_with_and_without_diacritics() {
        assert_eq!(fold("Hóa đơn tháng 11"), "hoa don thang 11");
        assert_eq!(fold("HÓA ĐƠN"), fold("hoa don"));
        assert_eq!(fold("Nguyễn Văn Ánh"), "nguyen van anh");
        assert_eq!(fold("Trường Đại học Bách khoa"), "truong dai hoc bach khoa");
        // Decomposed input (NFD, as some clients send it) folds the same.
        assert_eq!(fold("Ho\u{0301}a đơn"), fold("Hóa đơn"));
        assert_eq!(fold("ữ ự ỳ ỷ ỹ ỵ"), "u u y y y y");
        assert_eq!(fold("  a\tb\r\nc  "), "a b c");
        assert_eq!(fold("Straße Øre Łódź"), "strase ore lodz");
    }

    #[test]
    fn query_terms_are_folded_bounded_and_split_on_syntax() {
        assert_eq!(terms("Hóa ĐƠN"), vec!["hoa", "don"]);
        assert_eq!(
            terms("\"x\" OR 1=1; DROP TABLE messages --"),
            vec!["x", "or", "1", "drop", "table", "messages"]
        );
        assert_eq!(terms("a@b.vn"), vec!["a@b.vn"]);
        assert_eq!(
            terms("NEAR(a b) col:val *"),
            vec!["near", "a", "b", "col", "val"]
        );
        assert!(terms("   ").is_empty());
        assert!(terms("%%% ___ ''' ").is_empty());
        let long = "x".repeat(10_000);
        assert_eq!(terms(&long)[0].chars().count(), MAX_TERM_CHARS);
        let many: String = (0..50).map(|i| format!("w{i} ")).collect();
        assert_eq!(terms(&many).len(), MAX_TERMS);
    }

    #[test]
    fn fts_and_like_patterns_neutralise_syntax() {
        assert_eq!(
            fts_match(&["hoa".into(), "don".into()]),
            "\"hoa\"* AND \"don\"*"
        );
        assert_eq!(fts_match(&["a\"b".into()]), "\"a\"\"b\"*");
        assert_eq!(like_pattern("50%_x\\"), "%50\\%\\_x\\\\%");
    }
}
