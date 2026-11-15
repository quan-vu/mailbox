# ADR 0006 — Local search index and conversation threading

- Status: Proposed (2026-11-15, MB-200 / MB-201, track E)
- Builds on: ADR 0002 (provider is the source of truth), ADR 0005 (one repository over two SQL dialects)

## Context

ADR 0005 says dialect-specific features such as full-text search get their own ADR. MB-200 needs
search over cached envelopes and body text with Vietnamese matching with and without diacritics,
and MB-201 needs conversations computed from `Message-ID` / `In-Reply-To` / `References`.

## Decision

- **Folding in Rust, not in SQL.** `mailbox_core::search::fold` (Unicode NFD, combining marks
  dropped, `đ` → `d`, lower case) is applied when the index is written and to every query, so
  both dialects compare the same folded text and no custom SQL function is needed.
- **Portable index table.** Migration `0005` adds `message_search(message_id, account_id, meta,
  body, body_at)` with `ON DELETE CASCADE`. It is refreshed lazily: messages without a row,
  headers rewritten (row dropped), bodies downloaded or dropped (`body_at` differs).
- **SQLite:** when the bundled library has FTS5 (the `libsqlite3-sys` bundled build does), the
  store creates an external-content FTS5 table over `message_search` with triggers at open time
  (not in the migration, so a library without FTS5 still migrates) and searches with `MATCH`.
  Every query word is a quoted prefix phrase, so FTS syntax in the input has no meaning.
  Without FTS5 the same trait searches with `LIKE ... ESCAPE '\'`.
- **PostgreSQL:** `ILIKE ... ESCAPE '\'` over the same folded columns for now. A `pg_trgm` or
  `tsvector` index is a later decision once Web has real data (Web is demo-only in Desktop 1.0).
- **Query input is data.** Statements are assembled from fixed fragments only; every value is a
  bound parameter; queries are capped (200 chars, 8 words, 64 chars per word). Search words are
  never logged or persisted.
- **Paging is keyset**, `ORDER BY internal_date DESC, id` with `(date, id)` cursors, stable when
  new mail arrives. A `messages(internal_date DESC, id)` index serves cross-folder lists.
- **Threading** is a pure function in `mailbox_core::threading`, per account: union of ids along
  `References` + `In-Reply-To` (JWZ-style links with cycle protection only for choosing a root).
  A subject fallback applies only to messages with no ids at all, same normalized subject,
  within 3 days and with a shared participant other than the account's own address. The thread
  id is `<account>:<fnv64 of the root id>`; it does not change while replies arrive. Results are
  stored in `message_threads`; header rows in `message_headers` (`MailStore::store_headers`, a
  defaulted method so existing implementations keep compiling).

## Consequences

- Search finds only cached mail (envelopes of synced windows, bodies the user opened). Search on
  the server (`UID SEARCH`) is not part of this decision.
- The index duplicates folded text of subjects, senders, recipients and opened bodies in the same
  local database file (same protection as the cache itself; no encryption at rest yet, see
  PRIVACY.md).
- PostgreSQL search scans; acceptable while Web holds no real mail.
