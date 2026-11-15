//! Keyset paging, local search, stored headers and thread ids (MB-200, MB-201) for both dialects.
//!
//! SQL is assembled only from fixed fragments in this file; every value (ids, dates, search
//! words) is a bound parameter. Search words are folded in Rust (`mailbox_core::search::fold`) on
//! both sides. SQLite uses an FTS5 index when the library has it, otherwise `LIKE`; PostgreSQL
//! uses `ILIKE`. Errors leave as static codes; SQL, words and rows never do.
use crate::{PgStore, SqliteStore, storage};
use mailbox_core::MailboxError;
use mailbox_core::search::{
    MAX_INDEXED_BODY_CHARS, MAX_PAGE, Narrowing, PageKey, PageStore, Scope, SearchEngine,
    SearchHit, SearchPage, SearchQuery, SearchStore, fold, fts_match, like_pattern, terms,
};
use mailbox_core::store::MessageRecord;
use mailbox_core::threading::{
    EnvelopeHeaders, HeaderRow, ThreadMessage, ThreadStore, parse_msg_ids,
};
use sqlx::Row;

/// A bound value of a dynamically assembled statement.
enum Param {
    Text(String),
    Int(i64),
}

/// Collects fixed SQL fragments and their parameters, numbering placeholders `$1..$n`.
struct Sql {
    text: String,
    params: Vec<Param>,
}

impl Sql {
    fn new(start: &str) -> Self {
        Self {
            text: start.to_string(),
            params: Vec::new(),
        }
    }
    fn push(&mut self, fragment: &str) -> &mut Self {
        self.text.push_str(fragment);
        self
    }
    /// Append a placeholder bound to `p`.
    fn arg(&mut self, p: Param) -> &mut Self {
        self.params.push(p);
        let n = self.params.len();
        self.text.push_str(&format!("${n}"));
        self
    }
    fn text(&mut self, v: &str) -> &mut Self {
        self.arg(Param::Text(v.to_string()))
    }
    fn int(&mut self, v: i64) -> &mut Self {
        self.arg(Param::Int(v))
    }
}

const COLUMNS: &str = "m.id, m.account_id, m.folder_id, m.uidvalidity, m.imap_uid, m.message_id_header, \
    m.subject, m.from_display, m.from_address, m.preview, m.internal_date, m.size_bytes, m.is_seen, \
    m.is_flagged";

const FROM: &str =
    " FROM messages m JOIN accounts a ON a.id = m.account_id JOIN folders f ON f.id = m.folder_id";

/// The unfolded text the `meta` column is folded from (same expression in both dialects).
const META_SRC: &str = "(m.subject || ' ' || m.from_display || ' ' || m.from_address || ' ' || \
    COALESCE(h.to_line, '') || ' ' || COALESCE(h.cc_line, ''))";

fn scope_sql(sql: &mut Sql, scope: &Scope) {
    match scope {
        Scope::Folder {
            account_id,
            folder_id,
        } => {
            sql.push(" AND m.account_id = ").text(account_id);
            sql.push(" AND m.folder_id = ").text(folder_id);
        }
        Scope::Role { role, account_id } => {
            sql.push(" AND f.special_use = ").text(role);
            if let Some(account_id) = account_id {
                sql.push(" AND m.account_id = ").text(account_id);
            }
        }
        Scope::Account(account_id) => {
            sql.push(" AND m.account_id = ").text(account_id);
        }
        Scope::All => {}
    }
}

fn narrowing_sql(sql: &mut Sql, n: &Narrowing) {
    if n.unread_only {
        sql.push(" AND m.is_seen = 0");
    }
    if n.attachments_only {
        sql.push(
            " AND EXISTS (SELECT 1 FROM attachments t WHERE t.message_id = m.id AND t.is_inline = 0)",
        );
    }
    if let Some(since) = n.since {
        sql.push(" AND m.internal_date >= ").int(since);
    }
    if let Some(account_id) = &n.account_id {
        sql.push(" AND m.account_id = ").text(account_id);
    }
}

/// Lower-case addresses in a free-form address line (`Name <a@b>, c@d`).
fn addresses(line: &str) -> Vec<String> {
    line.split([',', ';', ' ', '<', '>', '"', '\t'])
        .filter(|t| t.contains('@'))
        .map(|t| {
            t.trim_matches(|c: char| c == '(' || c == ')' || c == '\'')
                .to_lowercase()
        })
        .filter(|t| !t.is_empty())
        .collect()
}

/// Folded search text of a message's header fields and body.
fn folded(meta_src: &str, body: Option<&str>) -> (String, String) {
    let body: String = body
        .unwrap_or_default()
        .chars()
        .take(MAX_INDEXED_BODY_CHARS)
        .collect();
    (fold(meta_src), fold(&body))
}

macro_rules! impl_search_store {
    ($store:ty, $row:ty, $like:literal) => {
        impl $store {
            fn decode_message(r: &$row) -> Result<MessageRecord, MailboxError> {
                let uid_validity: i64 = r.try_get("uidvalidity").map_err(storage("decode"))?;
                let imap_uid: i64 = r.try_get("imap_uid").map_err(storage("decode"))?;
                let size: i64 = r.try_get("size_bytes").map_err(storage("decode"))?;
                let seen: i64 = r.try_get("is_seen").map_err(storage("decode"))?;
                let flagged: i64 = r.try_get("is_flagged").map_err(storage("decode"))?;
                Ok(MessageRecord {
                    id: r.try_get("id").map_err(storage("decode"))?,
                    account_id: r.try_get("account_id").map_err(storage("decode"))?,
                    folder_id: r.try_get("folder_id").map_err(storage("decode"))?,
                    uid_validity: uid_validity as u64,
                    imap_uid: u32::try_from(imap_uid)
                        .map_err(|_| MailboxError::InconsistentData("uid out of range"))?,
                    message_id_header: r.try_get("message_id_header").map_err(storage("decode"))?,
                    subject: r.try_get("subject").map_err(storage("decode"))?,
                    from_display: r.try_get("from_display").map_err(storage("decode"))?,
                    from_address: r.try_get("from_address").map_err(storage("decode"))?,
                    preview: r.try_get("preview").map_err(storage("decode"))?,
                    internal_date: r.try_get("internal_date").map_err(storage("decode"))?,
                    size_bytes: size as u64,
                    seen: seen != 0,
                    flagged: flagged != 0,
                })
            }

            async fn fetch_rows(&self, sql: &Sql, code: &'static str) -> Result<Vec<$row>, MailboxError> {
                let mut q = sqlx::query(&sql.text);
                for p in &sql.params {
                    q = match p {
                        Param::Text(v) => q.bind(v.clone()),
                        Param::Int(v) => q.bind(*v),
                    };
                }
                q.fetch_all(&self.pool).await.map_err(storage(code))
            }

            /// `MailStore::store_headers` for this dialect.
            pub(crate) async fn put_headers(&self, owner_id: &str, rows: &[HeaderRow]) -> Result<(), MailboxError> {
                if rows.is_empty() {
                    return Ok(());
                }
                let mut tx = self.pool.begin().await.map_err(storage("tx_begin"))?;
                for row in rows {
                    let owned = sqlx::query(
                        "SELECT 1 FROM messages m JOIN accounts a ON a.id = m.account_id \
                         WHERE m.id = $1 AND m.account_id = $2 AND a.owner_id = $3",
                    )
                    .bind(&row.message_id)
                    .bind(&row.account_id)
                    .bind(owner_id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(storage("header_scope"))?;
                    if owned.is_none() {
                        return Err(MailboxError::NotFound);
                    }
                    sqlx::query(
                        "INSERT INTO message_headers (message_id, account_id, in_reply_to, references_list, \
                         to_line, cc_line) VALUES ($1,$2,$3,$4,$5,$6) \
                         ON CONFLICT (message_id) DO UPDATE SET in_reply_to = excluded.in_reply_to, \
                         references_list = excluded.references_list, to_line = excluded.to_line, \
                         cc_line = excluded.cc_line",
                    )
                    .bind(&row.message_id)
                    .bind(&row.account_id)
                    .bind(&row.headers.in_reply_to)
                    .bind(row.headers.references.join(" "))
                    .bind(&row.headers.to)
                    .bind(&row.headers.cc)
                    .execute(&mut *tx)
                    .await
                    .map_err(storage("header_write"))?;
                    // Recipients are part of the indexed text: index this message again.
                    sqlx::query("DELETE FROM message_search WHERE message_id = $1")
                        .bind(&row.message_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(storage("index_write"))?;
                }
                tx.commit().await.map_err(storage("tx_commit"))
            }

            /// Messages of `scope` in list order, for paging (`before`) or a floor (`floor`).
            fn list_sql(owner_id: &str, scope: &Scope, narrowing: &Narrowing) -> Sql {
                let mut sql = Sql::new(&format!("SELECT {COLUMNS}{FROM} WHERE a.owner_id = "));
                sql.text(owner_id);
                scope_sql(&mut sql, scope);
                narrowing_sql(&mut sql, narrowing);
                sql
            }
        }

        impl PageStore for $store {
            async fn page(
                &self,
                owner_id: &str,
                scope: &Scope,
                narrowing: &Narrowing,
                before: Option<&PageKey>,
                limit: u32,
            ) -> Result<Vec<MessageRecord>, MailboxError> {
                let mut sql = Self::list_sql(owner_id, scope, narrowing);
                if let Some(k) = before {
                    // The leading `<=` lets the date index seek instead of scanning from the top.
                    sql.push(" AND m.internal_date <= ").int(k.internal_date);
                    sql.push(" AND (m.internal_date < ").int(k.internal_date);
                    sql.push(" OR m.id > ").text(&k.id).push(")");
                }
                sql.push(" ORDER BY m.internal_date DESC, m.id LIMIT ")
                    .int(i64::from(limit.clamp(1, MAX_PAGE)));
                self.fetch_rows(&sql, "page_read")
                    .await?
                    .iter()
                    .map(Self::decode_message)
                    .collect()
            }

            async fn since(
                &self,
                owner_id: &str,
                scope: &Scope,
                floor: &PageKey,
                cap: u32,
            ) -> Result<Vec<MessageRecord>, MailboxError> {
                let mut sql = Self::list_sql(owner_id, scope, &Narrowing::default());
                sql.push(" AND m.internal_date >= ").int(floor.internal_date);
                sql.push(" AND (m.internal_date > ").int(floor.internal_date);
                sql.push(" OR m.id <= ").text(&floor.id).push(")");
                sql.push(" ORDER BY m.internal_date DESC, m.id LIMIT ")
                    .int(i64::from(cap.max(1)));
                self.fetch_rows(&sql, "page_read")
                    .await?
                    .iter()
                    .map(Self::decode_message)
                    .collect()
            }

            async fn lowest_uid(
                &self,
                owner_id: &str,
                account_id: &str,
                folder_id: &str,
                uid_validity: u64,
            ) -> Result<Option<u32>, MailboxError> {
                let row = sqlx::query(
                    "SELECT MIN(m.imap_uid) AS low FROM messages m JOIN accounts a ON a.id = m.account_id \
                     WHERE a.owner_id = $1 AND m.account_id = $2 AND m.folder_id = $3 AND m.uidvalidity = $4",
                )
                .bind(owner_id)
                .bind(account_id)
                .bind(folder_id)
                .bind(uid_validity as i64)
                .fetch_one(&self.pool)
                .await
                .map_err(storage("page_read"))?;
                let low: Option<i64> = row.try_get("low").map_err(storage("decode"))?;
                low.map(|v| u32::try_from(v).map_err(|_| MailboxError::InconsistentData("uid out of range")))
                    .transpose()
            }
        }

        impl SearchStore for $store {
            async fn refresh_search_index(&self, owner_id: &str, max: u32) -> Result<usize, MailboxError> {
                // Rows to (re)index, cheapest checks first. IMAP envelopes never change, so a
                // header row being (re)written drops the index row (see `put_headers`) and a
                // newly fetched or dropped body shows as a different `body_at`.
                let candidates = "SELECT m.id FROM messages m JOIN accounts a ON a.id = m.account_id \
                     WHERE a.owner_id = $1 AND NOT EXISTS \
                     (SELECT 1 FROM message_search s WHERE s.message_id = m.id) LIMIT $2";
                let mut ids: Vec<String> = sqlx::query(candidates)
                    .bind(owner_id)
                    .bind(i64::from(max.max(1)))
                    .fetch_all(&self.pool)
                    .await
                    .map_err(storage("index_read"))?
                    .iter()
                    .map(|r| r.try_get("id"))
                    .collect::<Result<_, _>>()
                    .map_err(storage("decode"))?;
                // Bodies downloaded (or replaced) since the row was indexed: scans bodies only.
                let changed = "SELECT b.message_id AS id FROM message_bodies b \
                     JOIN message_search s ON s.message_id = b.message_id \
                     JOIN accounts a ON a.id = s.account_id \
                     WHERE a.owner_id = $1 AND s.body_at <> b.fetched_at LIMIT $2";
                // Bodies dropped from the cache: their text must leave the index too.
                let dropped = "SELECT s.message_id AS id FROM message_search s \
                     JOIN accounts a ON a.id = s.account_id \
                     WHERE a.owner_id = $1 AND s.body_at > 0 AND NOT EXISTS \
                     (SELECT 1 FROM message_bodies b WHERE b.message_id = s.message_id) LIMIT $2";
                for q in [changed, dropped] {
                    let room = i64::from(max.max(1)) - ids.len() as i64;
                    if room <= 0 {
                        break;
                    }
                    let rows = sqlx::query(q)
                        .bind(owner_id)
                        .bind(room)
                        .fetch_all(&self.pool)
                        .await
                        .map_err(storage("index_read"))?;
                    for r in &rows {
                        ids.push(r.try_get("id").map_err(storage("decode"))?);
                    }
                }
                if ids.is_empty() {
                    return Ok(0);
                }
                let row_sql = format!(
                    "SELECT m.account_id, {META_SRC} AS meta_src, b.body_text, \
                     COALESCE(b.fetched_at, 0) AS body_at \
                     FROM messages m LEFT JOIN message_headers h ON h.message_id = m.id \
                     LEFT JOIN message_bodies b ON b.message_id = m.id WHERE m.id = $1"
                );
                let mut tx = self.pool.begin().await.map_err(storage("tx_begin"))?;
                for id in &ids {
                    let Some(r) = sqlx::query(&row_sql)
                        .bind(id)
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(storage("index_read"))?
                    else {
                        continue;
                    };
                    let account_id: String = r.try_get("account_id").map_err(storage("decode"))?;
                    let meta_src: String = r.try_get("meta_src").map_err(storage("decode"))?;
                    let body: Option<String> = r.try_get("body_text").map_err(storage("decode"))?;
                    let body_at: i64 = r.try_get("body_at").map_err(storage("decode"))?;
                    let (meta, body) = folded(&meta_src, body.as_deref());
                    sqlx::query(
                        "INSERT INTO message_search (message_id, account_id, meta, body, body_at) \
                         VALUES ($1,$2,$3,$4,$5) ON CONFLICT (message_id) DO UPDATE SET \
                         meta = excluded.meta, body = excluded.body, body_at = excluded.body_at",
                    )
                    .bind(id)
                    .bind(&account_id)
                    .bind(&meta)
                    .bind(&body)
                    .bind(body_at)
                    .execute(&mut *tx)
                    .await
                    .map_err(storage("index_write"))?;
                }
                tx.commit().await.map_err(storage("tx_commit"))?;
                Ok(ids.len())
            }

            async fn search(&self, owner_id: &str, q: &SearchQuery) -> Result<SearchPage, MailboxError> {
                let engine = self.search_engine();
                let words = terms(&q.text);
                if words.is_empty() {
                    return Ok(SearchPage { hits: Vec::new(), total: 0, engine });
                }
                let mut filter = Sql::new(&format!("{FROM} WHERE a.owner_id = "));
                filter.text(owner_id);
                scope_sql(&mut filter, &q.scope);
                narrowing_sql(&mut filter, &q.narrowing);
                match engine {
                    SearchEngine::Fts5 => {
                        filter.push(
                            " AND m.id IN (SELECT s.message_id FROM message_fts \
                             JOIN message_search s ON s.rowid = message_fts.rowid WHERE message_fts MATCH ",
                        );
                        filter.text(&fts_match(&words)).push(")");
                    }
                    SearchEngine::Like => {
                        for w in &words {
                            let pattern = like_pattern(w);
                            filter.push(" AND EXISTS (SELECT 1 FROM message_search s WHERE s.message_id = m.id AND (s.meta ");
                            filter.push($like).push(" ").text(&pattern).push(" ESCAPE '\\' OR s.body ");
                            filter.push($like).push(" ").text(&pattern).push(" ESCAPE '\\'))");
                        }
                    }
                }
                let mut count = Sql::new("SELECT COUNT(*) AS n");
                count.push(&filter.text);
                count.params = filter.params.iter().map(|p| match p {
                    Param::Text(v) => Param::Text(v.clone()),
                    Param::Int(v) => Param::Int(*v),
                }).collect();
                let total: i64 = self
                    .fetch_rows(&count, "search_read")
                    .await?
                    .first()
                    .map(|r| r.try_get("n"))
                    .transpose()
                    .map_err(storage("decode"))?
                    .unwrap_or(0);

                let mut list = Sql::new(&format!(
                    "SELECT {COLUMNS}, th.thread_id, COALESCE(hh.to_line, '') AS to_line"
                ));
                list.params = filter.params;
                let rest = filter.text.replacen(
                    FROM,
                    &format!("{FROM} LEFT JOIN message_threads th ON th.message_id = m.id \
                              LEFT JOIN message_headers hh ON hh.message_id = m.id"),
                    1,
                );
                list.push(&rest);
                list.push(" ORDER BY m.internal_date DESC, m.id LIMIT ")
                    .int(i64::from(q.limit.clamp(1, MAX_PAGE)));
                let hits = self
                    .fetch_rows(&list, "search_read")
                    .await?
                    .iter()
                    .map(|r| {
                        Ok(SearchHit {
                            message: Self::decode_message(r)?,
                            thread_id: r.try_get("thread_id").map_err(storage("decode"))?,
                            to: r.try_get("to_line").map_err(storage("decode"))?,
                        })
                    })
                    .collect::<Result<Vec<_>, MailboxError>>()?;
                Ok(SearchPage { hits, total: total.max(0) as u64, engine })
            }

            fn search_engine(&self) -> SearchEngine {
                self.engine()
            }
        }

        impl ThreadStore for $store {
            async fn thread_inputs(&self, owner_id: &str, account_id: &str) -> Result<Vec<ThreadMessage>, MailboxError> {
                let rows = sqlx::query(
                    "SELECT m.id, m.message_id_header, m.subject, m.internal_date, m.from_address, \
                     h.in_reply_to, h.references_list, h.to_line, h.cc_line \
                     FROM messages m JOIN accounts a ON a.id = m.account_id \
                     LEFT JOIN message_headers h ON h.message_id = m.id \
                     WHERE m.account_id = $1 AND a.owner_id = $2",
                )
                .bind(account_id)
                .bind(owner_id)
                .fetch_all(&self.pool)
                .await
                .map_err(storage("thread_read"))?;
                rows.iter()
                    .map(|r| {
                        let from: String = r.try_get("from_address").map_err(storage("decode"))?;
                        let refs: Option<String> = r.try_get("references_list").map_err(storage("decode"))?;
                        let to: Option<String> = r.try_get("to_line").map_err(storage("decode"))?;
                        let cc: Option<String> = r.try_get("cc_line").map_err(storage("decode"))?;
                        let mut participants = vec![from.to_lowercase()];
                        participants.extend(addresses(to.as_deref().unwrap_or_default()));
                        participants.extend(addresses(cc.as_deref().unwrap_or_default()));
                        Ok(ThreadMessage {
                            key: r.try_get("id").map_err(storage("decode"))?,
                            message_id: r.try_get("message_id_header").map_err(storage("decode"))?,
                            in_reply_to: r.try_get("in_reply_to").map_err(storage("decode"))?,
                            references: parse_msg_ids(refs.as_deref().unwrap_or_default()),
                            subject: r.try_get("subject").map_err(storage("decode"))?,
                            date: r.try_get("internal_date").map_err(storage("decode"))?,
                            participants,
                        })
                    })
                    .collect()
            }

            async fn set_thread_ids(
                &self,
                owner_id: &str,
                account_id: &str,
                ids: &[(String, String)],
            ) -> Result<usize, MailboxError> {
                self.require_account(owner_id, account_id).await?;
                let current: std::collections::HashMap<String, String> =
                    self.thread_ids(owner_id, account_id).await?.into_iter().collect();
                let changed: Vec<&(String, String)> = ids
                    .iter()
                    .filter(|(k, t)| current.get(k) != Some(t))
                    .collect();
                if changed.is_empty() {
                    return Ok(0);
                }
                let mut tx = self.pool.begin().await.map_err(storage("tx_begin"))?;
                let mut written = 0;
                for (key, thread) in changed {
                    // Only messages of this account; a stale key simply matches nothing.
                    let r = sqlx::query(
                        "INSERT INTO message_threads (message_id, account_id, thread_id) \
                         SELECT m.id, m.account_id, $3 FROM messages m WHERE m.id = $1 AND m.account_id = $2 \
                         ON CONFLICT (message_id) DO UPDATE SET thread_id = excluded.thread_id",
                    )
                    .bind(key)
                    .bind(account_id)
                    .bind(thread)
                    .execute(&mut *tx)
                    .await
                    .map_err(storage("thread_write"))?;
                    written += r.rows_affected() as usize;
                }
                tx.commit().await.map_err(storage("tx_commit"))?;
                Ok(written)
            }

            async fn thread_ids(&self, owner_id: &str, account_id: &str) -> Result<Vec<(String, String)>, MailboxError> {
                let rows = sqlx::query(
                    "SELECT t.message_id, t.thread_id FROM message_threads t \
                     JOIN accounts a ON a.id = t.account_id WHERE t.account_id = $1 AND a.owner_id = $2",
                )
                .bind(account_id)
                .bind(owner_id)
                .fetch_all(&self.pool)
                .await
                .map_err(storage("thread_read"))?;
                rows.iter()
                    .map(|r| {
                        Ok((
                            r.try_get("message_id").map_err(storage("decode"))?,
                            r.try_get("thread_id").map_err(storage("decode"))?,
                        ))
                    })
                    .collect()
            }

            async fn thread_sizes(&self, owner_id: &str, account_id: &str) -> Result<Vec<(String, usize)>, MailboxError> {
                let rows = sqlx::query(
                    "SELECT t.thread_id, COUNT(DISTINCT COALESCE(m.message_id_header, m.id)) AS n \
                     FROM message_threads t JOIN messages m ON m.id = t.message_id \
                     JOIN accounts a ON a.id = t.account_id \
                     WHERE t.account_id = $1 AND a.owner_id = $2 GROUP BY t.thread_id",
                )
                .bind(account_id)
                .bind(owner_id)
                .fetch_all(&self.pool)
                .await
                .map_err(storage("thread_read"))?;
                rows.iter()
                    .map(|r| {
                        let n: i64 = r.try_get("n").map_err(storage("decode"))?;
                        Ok((r.try_get("thread_id").map_err(storage("decode"))?, n.max(0) as usize))
                    })
                    .collect()
            }

            async fn conversation(
                &self,
                owner_id: &str,
                account_id: &str,
                thread_id: &str,
            ) -> Result<Vec<MessageRecord>, MailboxError> {
                let rows = sqlx::query(&format!(
                    "SELECT {COLUMNS} FROM messages m JOIN accounts a ON a.id = m.account_id \
                     JOIN message_threads t ON t.message_id = m.id \
                     WHERE t.account_id = $1 AND t.thread_id = $2 AND a.owner_id = $3 \
                     ORDER BY m.internal_date, m.id LIMIT 2000"
                ))
                .bind(account_id)
                .bind(thread_id)
                .bind(owner_id)
                .fetch_all(&self.pool)
                .await
                .map_err(storage("thread_read"))?;
                rows.iter().map(Self::decode_message).collect()
            }

            async fn headers_of(&self, owner_id: &str, message_key: &str) -> Result<Option<EnvelopeHeaders>, MailboxError> {
                let row = sqlx::query(
                    "SELECT h.in_reply_to, h.references_list, h.to_line, h.cc_line FROM message_headers h \
                     JOIN accounts a ON a.id = h.account_id WHERE h.message_id = $1 AND a.owner_id = $2",
                )
                .bind(message_key)
                .bind(owner_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(storage("header_read"))?;
                let Some(r) = row else { return Ok(None) };
                let refs: String = r.try_get("references_list").map_err(storage("decode"))?;
                Ok(Some(EnvelopeHeaders {
                    in_reply_to: r.try_get("in_reply_to").map_err(storage("decode"))?,
                    references: parse_msg_ids(&refs),
                    to: r.try_get("to_line").map_err(storage("decode"))?,
                    cc: r.try_get("cc_line").map_err(storage("decode"))?,
                }))
            }

            async fn uids_missing_headers(
                &self,
                owner_id: &str,
                account_id: &str,
                folder_id: &str,
                uid_validity: u64,
                limit: u32,
            ) -> Result<Vec<u32>, MailboxError> {
                let rows = sqlx::query(
                    "SELECT m.imap_uid FROM messages m JOIN accounts a ON a.id = m.account_id \
                     LEFT JOIN message_headers h ON h.message_id = m.id \
                     WHERE a.owner_id = $1 AND m.account_id = $2 AND m.folder_id = $3 \
                     AND m.uidvalidity = $4 AND h.message_id IS NULL \
                     ORDER BY m.imap_uid DESC LIMIT $5",
                )
                .bind(owner_id)
                .bind(account_id)
                .bind(folder_id)
                .bind(uid_validity as i64)
                .bind(i64::from(limit.max(1)))
                .fetch_all(&self.pool)
                .await
                .map_err(storage("header_read"))?;
                rows.iter()
                    .map(|r| {
                        let uid: i64 = r.try_get("imap_uid").map_err(storage("decode"))?;
                        u32::try_from(uid).map_err(|_| MailboxError::InconsistentData("uid out of range"))
                    })
                    .collect()
            }
        }
    };
}

impl_search_store!(SqliteStore, sqlx::sqlite::SqliteRow, "LIKE");
impl_search_store!(PgStore, sqlx::postgres::PgRow, "ILIKE");

impl PgStore {
    /// PostgreSQL has no full-text index here yet: substring search with `ILIKE`.
    fn engine(&self) -> SearchEngine {
        SearchEngine::Like
    }
}

impl SqliteStore {
    fn engine(&self) -> SearchEngine {
        if self.fts {
            SearchEngine::Fts5
        } else {
            SearchEngine::Like
        }
    }

    /// The same store searching with `LIKE` instead of FTS5 (tests of the fallback path).
    pub fn without_fts(mut self) -> Self {
        self.fts = false;
        self
    }

    /// Create the FTS5 index over `message_search` and its sync triggers when the SQLite library
    /// has FTS5. Returns whether FTS5 is in use. A library without it keeps the `LIKE` path.
    pub(crate) async fn prepare_fts(pool: &sqlx::SqlitePool) -> bool {
        let existed = sqlx::query(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'message_fts'",
        )
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .is_some();
        let statements = [
            "CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(meta, body, \
             content='message_search', content_rowid='rowid', tokenize='unicode61 remove_diacritics 2')",
            "CREATE TRIGGER IF NOT EXISTS message_search_ai AFTER INSERT ON message_search BEGIN \
             INSERT INTO message_fts(rowid, meta, body) VALUES (new.rowid, new.meta, new.body); END",
            "CREATE TRIGGER IF NOT EXISTS message_search_ad AFTER DELETE ON message_search BEGIN \
             INSERT INTO message_fts(message_fts, rowid, meta, body) VALUES ('delete', old.rowid, old.meta, old.body); END",
            "CREATE TRIGGER IF NOT EXISTS message_search_au AFTER UPDATE ON message_search BEGIN \
             INSERT INTO message_fts(message_fts, rowid, meta, body) VALUES ('delete', old.rowid, old.meta, old.body); \
             INSERT INTO message_fts(rowid, meta, body) VALUES (new.rowid, new.meta, new.body); END",
        ];
        for s in statements {
            if sqlx::query(s).execute(pool).await.is_err() {
                return false;
            }
        }
        // A new index is filled from the rows that already exist.
        existed
            || sqlx::query("INSERT INTO message_fts(message_fts) VALUES ('rebuild')")
                .execute(pool)
                .await
                .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_extracted_from_free_form_lines() {
        assert_eq!(
            addresses("\"Trần, Linh\" <Linh@X.vn>, b@y.org; (c@z.io)"),
            vec!["linh@x.vn", "b@y.org", "c@z.io"]
        );
        assert!(addresses("Undisclosed recipients:;").is_empty());
    }

    #[test]
    fn statements_number_placeholders_and_never_embed_values() {
        let mut sql = Sql::new("SELECT 1 WHERE a = ");
        sql.text("'; DROP TABLE x; --").push(" AND b = ").int(5);
        assert_eq!(sql.text, "SELECT 1 WHERE a = $1 AND b = $2");
        assert_eq!(sql.params.len(), 2);
    }
}
