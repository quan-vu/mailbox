# Track E evidence: paging, local search, conversations (MB-200, MB-201)

Branch `claude/mb-200-search-threads-3174b0`, cut from `main` at `d13ce9d`. Date: 2026-11-15.
Scope: Desktop, Real mode. Demo mode keeps its in-memory list, grouping and search unchanged.

Status proposed for TRACKING (not edited here): implemented and tested at core, store, IMAP,
adapter and SSR level, and against the real test provider. **Not DONE** by the project's
definition until the installed Desktop app has been clicked through (see "Not verified").

## What was built

| Task | Where | Behaviour |
|---|---|---|
| MB-201 headers | `crates/mailbox-imap/src/{lib,parse}.rs`, `crates/mailbox-core/src/sync.rs`, migration `0005` (`message_headers`) | The envelope FETCH asks additionally for `IN-REPLY-TO REFERENCES TO CC` (still `BODY.PEEK`). Ids are parsed and bounded (100 refs, 998 bytes each). `sync_folder` stores them with every new envelope through `MailStore::store_headers` (new, defaulted method; both SQL stores implement it). Rows cached before `0005` are backfilled lazily after each sync (`history::backfill_headers`, 300 per folder per sync, `UID FETCH <uid-set>`); UIDs gone from the server are marked so they are not asked again. |
| MB-201 threader | `crates/mailbox-core/src/threading.rs`, `message_threads` table | Per account. Union of `Message-ID`, `In-Reply-To`, `References` (JWZ-style parent links with cycle protection, used to pick a stable root). Same id in two folders (inbox copy + Sent copy) is one message. Subject fallback only for messages with **no ids at all**: same normalized subject (prefixes `Re/Fwd/AW/SV/TR/Trả lời/Chuyển tiếp`, list tags, diacritics removed), within 3 days, with a shared participant other than the account's own address. Thread id `<account>:<fnv64(root id)>`, unchanged when replies or the missing root arrive. Recomputed after a sync when anything changed; only changed rows are written. |
| MB-200 paging | `crates/mailbox-core/src/search.rs` (`PageStore`), `crates/mailbox-store/src/search.rs` | Keyset paging `ORDER BY internal_date DESC, id` with `(date, id)` cursors, for a folder, a role across accounts (unified inbox = role `inbox`) or an account; filter chips (unread, today, account, attachment) are in the `WHERE`. New arrivals never shift older pages (tested). |
| MB-200 load older | `crates/mailbox-core/src/history.rs`, `apps/mailbox-ui/src/adapters/history.rs`, `components/search.rs` | "Tải thư cũ hơn" at the end of a real list: first lowers the view's keyset floor in the local cache (100 per step; each folder then shows every message above the floor), then, when the cache is exhausted, fetches an older UID window per folder from the server (`UID FETCH lo:hi`, 100 UIDs, widening up to 4 rounds over deleted gaps, at most 800 per call), refuses on a UIDVALIDITY change, never moves the sync cursor, then pages the cache again. When every folder is covered down to UID 1 the footer says "Đã hiển thị đến thư cũ nhất của thư mục." Offline: says so and keeps showing the cache. |
| MB-200 search | `search.rs` (core + store), migration `0005` (`message_search`), FTS5 created at open | Folded text (NFD, marks removed, `đ`→`d`, lower case) of subject, sender name/address, To/Cc and downloaded body text (32k chars). SQLite: FTS5 external-content index (bundled SQLite has FTS5; created at open so a library without it still migrates), each word a quoted prefix phrase; fallback `LIKE ... ESCAPE '\'` behind the same trait (`SqliteStore::without_fts` tests it). PostgreSQL: `ILIKE ... ESCAPE '\'`. Scope: folder view / account / all accounts. Query capped at 200 chars, 8 words, 64 chars per word; every value bound as a parameter; syntax characters split words. Index refreshed lazily (new rows, rewritten headers, downloaded/dropped bodies), up to 2,000 rows in front of a search and 5,000 after a sync. |
| UI | `state/search.rs`, `components/search.rs`, `assets/search.css`, `screens/{inbox,thread}.rs` | Real mode: the existing search box (Ctrl/Cmd+K) drives the store search with a 250 ms debounce; a late answer to an older query is dropped. Search bar with scope switch (Thư mục này / Tài khoản này / Tất cả tài khoản), result count ("N kết quả · hiển thị 200 mới nhất" beyond the limit), clear control, explicit empty and failure states. Matches highlighted on folded text, rendered as text inside `mark` (never markup; tested with `<script>` in a subject). The list shows one row per conversation: the newest message of the conversation that is in the current folder view, with the conversation's size across folders (a Sent reply counts and does not appear as an inbox row). The thread screen loads the stored conversation across folders, oldest first, one entry per `Message-ID`, each with the real body pane. |
| Replies | `adapters/send.rs` (`save_in`) | A reply or forward now continues the original's stored `References` chain through the existing `state::compose::threading` function (was a one-element chain). |

Choice for actions on a conversation row: archive, delete and read/unread apply to the
conversation's messages **in the same folder as the opened message** (`same_folder_conversation`).
A copy of the conversation in another folder (for example the user's reply in Sent) is not moved.
Star applies to the one message, as before.

New `data-testid`s: `search-bar`, `search-scope-view`, `search-scope-account`, `search-scope-all`,
`search-count` (with `data-state` `busy|done|many|failed`), `search-clear`, `search-result-<id>`,
`list-footer` (with `data-older` `idle|loading|at-start|offline`), `list-load-older`,
`list-history-end`, `thread-span`. New attribute `data-search` on `.message-list`. Existing ids
unchanged; `message-row-<id>` keeps `data-thread-count`.

Server search (`UID SEARCH` with CHARSET UTF-8): **not built** (left out rather than shipped half
safe). Row in `LIMITATIONS_1.0.md`.

## Test first (RED, then GREEN)

Core threader, folding and IMAP parsing were written test-and-code together and run green on the
first run (34 core unit tests). The store layer was test-first:

```
cargo test -p mailbox-store --test search_contract      # before src/search.rs existed
  error[E0277]: the trait bound `SqliteStore: PageStore` is not satisfied      (x3)
  error[E0277]: the trait bound `SqliteStore: SearchStore` is not satisfied    (x3)
  error[E0277]: the trait bound `SqliteStore: ThreadStore` is not satisfied    (x3)
  (same for PgStore), error[E0599]: no method named `without_fts` / `search_engine`
  error: could not compile `mailbox-store` (test "search_contract") due to 21 previous errors
```

Then GREEN after the implementation (one wrong expectation in the test itself was corrected:
the unified page order of equal-date pairs).

## Commands and results (this branch)

All with `export PATH="$HOME/.cargo/bin:$PATH"`.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo test --workspace` | 316 passed, 0 failed, 20 ignored (opt-in network / PostgreSQL / timing). `main` before this branch: 278 / 17 |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo clippy -p mailbox-ui --no-default-features --features desktop -- -D warnings` | pass |
| `cargo clippy -p mailbox-ui --no-default-features --features web --target wasm32-unknown-unknown -- -D warnings` | pass |
| `cargo test -p mailbox-core --lib` | 34 passed (threader 12: chains, missing parent + stable id, forks, duplicate ids, same-subject strangers, id-less window/participants, cycles, input order, 1,000-message thread, accounts; folding 3 incl. NFD input and injection-shaped queries) |
| `cargo test -p mailbox-imap --test history` | 1 passed (UID range / UID set, reply headers + RFC 2047 To, out-of-range UID dropped, `BODY.PEEK` only, no STORE) |
| `cargo test -p mailbox-store --test search_contract` | 2 passed, 1 ignored (SQLite FTS5 and SQLite LIKE fallback; PostgreSQL opt-in): paging across inserts, unified order, owner isolation, `since`, lowest UID, unread narrowing, search by subject/sender/recipient/body with and without diacritics, scope, today/account narrowing, totals beyond the limit, 12 injection inputs, `%` not a wildcard, re-index on header rewrite, headers, backfill list, cross-owner refusal, thread ids, sizes (Sent + inbox copy = 1), conversation order, cascade on account removal |
| `cargo test -p mailbox-store --test history` | 5 passed (load older over a UID gap until UID 1, bounded, cursor untouched, read-only calls; UIDVALIDITY change refused; lazy backfill bounded per call, no round trip when nothing is missing; gone UIDs marked; sync stores reply headers) |
| `cargo test -p mailbox-ui` | 98 passed: grouping (conversation = one row headed by its newest in-view message, size across folders, Sent reply not an inbox row), narrowing, folded highlights, syntax-only query is no search, cache paging of "load older" (depth 100 → 200 → 250 → exhausted), reply `References` chain, and SSR: conversation row, search bar/scope/count/clear/escaped highlights, empty result, pending search, Demo search unchanged |

### Real provider (opt-in, test mailbox, self-send only)

```
MAILBOX_SMTP_ENV_FILE=<repo>/.env.smtp \
MAILBOX_TEST_IMAP_HOST=mail2.provider.example MAILBOX_SMTP_SEND_TO=self \
  cargo test -p mailbox-store --test real_threads -- --ignored --nocapture

sent 2 marked messages (the second answers the first) to the test mailbox itself
both arrived after 2 sync(s); load older stored 0 (start reached: true); backfilled 0
conversation of 2 confirmed from Message-ID / In-Reply-To / References
search found the conversation (Fts5), with and without diacritics
cleanup: [Expunged, Expunged]
test real_reply_forms_one_conversation_that_search_finds ... ok
```

Only the two messages the test created were moved and expunged (each mutation checks the marker).
The test mailbox is small, so "load older" found nothing older on the real server: the
`UID FETCH lo:hi` path ran against the scripted server and the fake only.

### Timing with 50,000 cached envelopes (`#[ignore]`)

`cargo test -p mailbox-store --test scale -- --ignored --nocapture` (synthetic data: 2 accounts x
25,000 envelopes, 10-message conversations, 5,000 bodies, file-backed SQLite).

Run with the SQLite C library optimized as in shipped builds
(`--config 'profile.dev.package.libsqlite3-sys.opt-level=3'`, no file change; Rust code in debug).
The machine was shared with two other build sessions: load average 48 before / 41 after, on 10
cores, so these are pessimistic.

| Step | Time |
|---|---|
| Unified inbox, first page (100 of 40,000 inbox rows, two accounts) | 42.6 ms |
| 50 further keyset pages | 4.9 s total, worst page 208 ms (about 99 ms on average) |
| One folder, unread only, first page | 2.9 ms |
| FTS5 search, 7 queries (incl. `HÓA ĐƠN` = `hoa don`, address, body words, no match, folder scope) | 3.0 – 231 ms (5,002 / 7,144 matches counted and the newest 200 returned) |
| `LIKE` fallback search, 3 queries | 111 – 570 ms |
| Index check with nothing to do (runs before each search) | 108 ms |
| First indexing of 50,000 messages | 28.5 s (done after syncs in batches of 5,000, outside the IMAP gate) |
| Threading 2 x 25,000 messages, first time | 11.0 s |
| Re-threading 25,000 with no change | 1.6 s (only after a sync that changed something) |
| Seeding 50,000 envelopes + headers + 5,000 bodies | 44.0 s (test set-up, not a user path) |

Asserted bounds (pass): first and deepest page < 250 ms, FTS < 500 ms, LIKE < 2 s, idle index
check < 500 ms. An earlier run with a debug-built SQLite C library under the same load was 5 to
20 times slower; it showed that the keyset predicate `(date < d OR (date = d AND id > x))` made
deep pages scan from the top, which was changed to `date <= d AND (date < d OR id > x)`, and
that the stale-row check scanned every row, which was replaced by three narrow queries.

## Not verified

- The installed Desktop app was not clicked through: search typing, scope switch, load older,
  conversation rows and the thread screen are covered by SSR and adapter tests, not by a click.
  No `dx serve`, Playwright or bundle was run (owner rule for this track).
- PostgreSQL: migration `0005` and the search contract were not run (no PostgreSQL, Docker not
  started).
- `UID FETCH lo:hi` on the real server with a non-trivial gap (test mailbox too small).
- Windows / Linux.

## Risks and follow-ups

- Thread ids and sizes live in a small in-process map refreshed with every snapshot; per-folder
  "load older" depth is in memory and resets to 100 at restart.
- Re-threading reads all envelopes of an account; it runs only after a sync that changed
  something (1.6 s for 25,000 messages under load, see timing). An incremental threader is the
  next step if large mailboxes sync often.
- The first indexing of a large existing cache takes tens of seconds in batches after syncs;
  until it finishes, search covers the part already indexed (plus up to 2,000 rows indexed in
  front of each search).
- `RemoteEnvelope` gained a `headers` field: any new struct literal in another branch needs
  `..Default::default()` or the field (two test literals here were updated).
- `MailStore::store_headers` has a default no-op: a third store implementation would silently
  keep no headers (both existing stores override it and are tested).
- The "has attachment" chip in Real mode only knows messages whose body was downloaded.
- Search highlights subject and sender in the result row; no body snippet is shown.
