# Track B evidence: folder roles, multi-folder sync, flags, archive / trash / move

Tasks: MB-122, MB-143, MB-132, MB-144. Branch `claude/mb-132-flags-folders-72c2b8`, based on `f375122` (v0.2.0).
Date: 2026-11-15. Scope: Desktop, Real mode. Demo mode keeps its local-only behaviour and "(demo)" labels.

Status proposed for TRACKING (not edited here): implemented and tested at core, store, IMAP and SSR level, and against the real test provider. **Not DONE** by the project's definition until the installed Desktop app has been clicked through (see "Not verified").

## What was built

| Task | Where | Behaviour |
|---|---|---|
| MB-122 | `crates/mailbox-core/src/folders.rs`, `crates/mailbox-imap/src/folders.rs`, migration `0004` (`folder_roles`) | Role per folder: inbox, sent, drafts, trash, archive, junk. Source order: SPECIAL-USE / XLIST attributes (`\Sent`, `\Drafts`, `\Trash`, `\Archive`, `\Junk`, `\Spam`, `\Inbox`), then well-known English and Vietnamese names at top level or directly under `INBOX` / `[Provider]` with `/` or `.`, else none. One folder per role per account. Stored as data with its source (`attribute`, `name`, `user`); a `user` row is never replaced by detection (`set_user_role`). `\All` is not a role. |
| MB-143 | `sync_all_folders` in core `folders.rs`; `adapters/account.rs` | Every selectable folder is synced (newest 100 envelopes first, then incremental), each with its own UIDVALIDITY cursor. Per-folder report; one failing folder does not stop the others. Cap of 64 folders per account, the rest is counted as skipped. Folders deleted on the server are removed locally; a listing without INBOX is rejected and changes nothing. Sidebar in Real mode lists the real folders with unread counts; the unified inbox stays the default. |
| MB-132 | `crates/mailbox-core/src/actions.rs`, `crates/mailbox-imap/src/mutate.rs`, `crates/mailbox-store/src/actions.rs`, migration `0004` (`pending_ops`) | `MutableSession`: `SELECT`, `UID STORE +FLAGS/-FLAGS (\Seen \Flagged)`. Change is applied locally, queued, replayed on the next connection. UIDVALIDITY is compared before every mutation. `NO`/`BAD`: local value restored, operation kept as `failed` (visible on the row). Connection lost: operation stays queued. Flags are pulled on every sync with `UID FETCH first:last (UID FLAGS)` over the cached UID range; messages gone from the server are dropped from the cache. A flag with a queued local change is not overwritten; after confirm or reject the server wins. |
| MB-144 | same files | `UID MOVE` when the server has MOVE; otherwise `UID COPY` + `UID STORE +FLAGS.SILENT (\Deleted)` + `UID EXPUNGE <uid>` (UIDPLUS). Without UIDPLUS the original stays flagged and this is reported; a plain `EXPUNGE` is never sent. Delete = move to trash. Permanent delete only for mail in the trash folder, behind the existing confirm dialog. Undo: cancels a move that has not been sent, else moves the message back (new UID from `COPYUID`, else destination re-synced and matched by `Message-ID`). No archive folder: the UI says so, nothing is created. |
| UI | `adapters/actions.rs`, `state/folders.rs`, `components/folders.rs`, `assets/folders.css` | Reader toolbar, row star, read/unread, archive, delete and a new "Chuyển đến…" menu call the real actions in Real mode. Rows show pending / failed state. |

New `data-testid`s: `nav-folder-junk`, `nav-real-folder-<slug>`, `row-op-pending-<id>`, `row-op-failed-<id>`, `row-op-dismiss-<id>`, `reader-move`. New row attribute `data-op` (`none`/`pending`/`failed`). Existing ids unchanged.

Decision taken: CONDSTORE `CHANGEDSINCE` is not used. With a 100-message window the full flag scan is one cheap command, and it also finds messages deleted elsewhere, which `CHANGEDSINCE` cannot do without QRESYNC.

## Test first (RED, then GREEN)

RED runs before the implementation existed:

```
cargo test -p mailbox-core --test folder_roles
  error[E0432]: unresolved import `mailbox_core::folders`
cargo test -p mailbox-store --test actions --test actions_contract
  error[E0432]: unresolved import `mailbox_core::actions` (x2), `mailbox_core::folders` (x2)
cargo test -p mailbox-imap --test mutate
  error[E0432]: unresolved import `mailbox_imap::mutate`
cargo test -p mailbox-ui      (SSR tests added before the components)
  test result: FAILED. 44 passed; 4 failed
  failed: real_mode_sidebar_lists_real_folders_with_unread_counts, real_mode_list_follows_the_selected_real_folder,
          real_mode_rows_show_pending_and_failed_states_and_a_move_menu, demo_mode_has_no_real_folder_controls
```

`crates/mailbox-store/tests/replay_dialogue.rs` was written after the code it exercises (it combines parts that already had RED/GREEN runs).

## Gates (run 2026-11-15 on macOS, rustc from `rust-toolchain.toml`)

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo test --workspace` | 153 passed, 0 failed, 13 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo clippy -p mailbox-ui --no-default-features --features desktop -- -D warnings` | clean (also with `--all-targets`) |
| `cargo check -p mailbox-ui --no-default-features --features web --target wasm32-unknown-unknown` | clean |
| `cargo clippy -p mailbox-ui --no-default-features --features web --target wasm32-unknown-unknown -- -D warnings` | clean |

New test suites inside the 153:

| Suite | Tests | Covers |
|---|---|---|
| `mailbox-core/tests/folder_roles.rs` | 6 | attributes, English/Vietnamese names, prefixes and delimiters, user folders, one folder per role |
| `mailbox-imap/tests/mutate.rs` | 12 | every command path: MOVE present/absent, UIDPLUS present/absent, COPYUID tagged/untagged/missing, NO and BAD, dropped connection, read-only SELECT, command injection in folder names, flag scan, folder listing |
| `mailbox-store/tests/actions.rs` | 19 | use cases against SQLite and a fake server: roles, override, pruning, per-folder sync report, optimistic flag, rollback, queued after lost connection, UIDVALIDITY change, server-wins rule, move with/without COPYUID, no UIDPLUS, interrupted move not repeated, no archive folder, permanent delete only from trash, cancel |
| `mailbox-store/tests/actions_contract.rs` | 2 (+1 ignored: PostgreSQL) | store contract for roles and pending operations, owner isolation, cascades, portable migration |
| `mailbox-store/tests/replay_dialogue.rs` | 5 | real session code + SQLite against a scripted server that keeps state between connections |
| `mailbox-ui` | 12 new (48 total) | adapter view/summary (4), real-folder view logic (4), SSR: real sidebar, folder routing, pending/failed rows and move menu, demo unchanged (4) |

## Real provider (opt-in, owner test mailbox on hosted business mail / Dovecot)

```
MAILBOX_SMTP_ENV_FILE=<repo>/.env.smtp MAILBOX_SMTP_SEND_TO=self MAILBOX_TEST_IMAP_HOST=mail2.provider.example \
  cargo test -p mailbox-store --test real_actions -- --ignored --nocapture

sent 1 marked message to the test mailbox itself
folders synced: 5 (failed 0, skipped 0); roles: inbox, sent, drafts, trash, junk
marked message arrived after 2 sync(s)
it arrived in the folder with role Some("inbox")
server capabilities after login: MOVE=true UIDPLUS=true
server confirms \Seen and \Flagged on the marked message
moved to trash; new UID reported by server: true; original left flagged deleted: false
cleanup: Expunged
test result: ok. 1 passed; 0 failed; finished in 24.48s
```

The test only touches the message it sent (checked by its unique subject before every mutation), deletes it with `UID EXPUNGE` of that one UID, and prints no address or mail content. Finding worth keeping: this provider announces MOVE, UIDPLUS and SPECIAL-USE only **after** login, so capabilities must be read again after authentication (`MutableSession::upgrade` does).

## Not verified

- The Desktop app itself: no `dx serve`, no bundle, no click-through in this track. The flow star / read / archive / delete / move / undo in the running app is untested by hand.
- Playwright E2E was not run. Demo handlers in `screens/inbox.rs`, `screens/thread.rs` and the toast undo now go through `state/folders.rs`; the demo branch does what the old inline code did, but only unit/SSR tests confirm it.
- PostgreSQL: `postgres_meets_the_actions_contract` was not run (Docker Desktop did not start on this machine). Migration `postgres/0004_actions.sql` is unexecuted.
- Servers without MOVE or without UIDPLUS, read-only folders, and servers that send no COPYUID: scripted tests only.
- Undo after a move without COPYUID (re-sync and match by `Message-ID`): core path tested, adapter path not.
- More than one real account; folders with non-ASCII raw (non modified-UTF-7) names.

## Risks and known gaps

1. Each action opens its own IMAP connection (about 10 s TLS handshake on this provider). The row shows "Đang gửi lên máy chủ" meanwhile. A kept-alive session belongs to MB-140.
2. Without MOVE: if `UID COPY` succeeds and the following `\Deleted` store is refused, the copy stays in the destination and the move is reported as refused.
3. Without UIDPLUS the original stays on the server flagged `\Deleted`; envelope sync does not filter `\Deleted`, so a folder rebuild shows it again.
4. `UID STORE` on a UID that no longer exists answers OK; the row disappears at the next flag scan, not immediately.
5. A failed operation stays on the row until dismissed or until the user acts on that message again. It is never retried automatically.
6. Undo information lives in memory for the session; after a restart a move can only be undone by moving the message back by hand.
7. User override of a folder's role exists in core and store (`set_user_role`); there is no settings UI for it yet.
8. Per folder the list shows the newest 100 cached messages; the cache itself grows with incremental sync.
9. The empty-state text of an empty real folder still talks about adding an account.
10. Migration number is `0004` while `0002`/`0003` belong to other tracks. A database that ran this branch alone will apply the lower numbers later; check `sqlx` accepts that order on the merged build (a fresh or v1 database is not affected).
11. `\All` / `\Flagged` / `\Important` provider folders are listed as ordinary user folders.

## Security review

- TLS: unchanged transport (`mailbox-net`), no new connection path, no bypass.
- Credentials: read from the OS keychain only when a flush or sync connects; never logged or stored elsewhere.
- No server text, address, subject or SQL leaves the IMAP or store crates in errors: failures are short static codes (`refused`, `folder_missing`, `read_only`).
- Folder names are validated and quoted before use; CR/LF injection is covered by a test.
- UIDs are only used in a folder selected in the same session after its UIDVALIDITY matched the operation's.
- Destructive scope: permanent delete acts on one UID, only in the trash folder, only after the confirm dialog.

## Shared files touched

| File | Change |
|---|---|
| `crates/mailbox-core/src/lib.rs` | two `pub mod` lines (`actions`, `folders`) |
| `crates/mailbox-imap/src/lib.rs` | `mod folders; pub mod mutate;` |
| `crates/mailbox-store/src/lib.rs` | `mod actions;` |
| `crates/mailbox-store/Cargo.toml` | dev-dependencies: `mailbox-smtp`, tokio `time` (real-provider test) |
| `apps/mailbox-ui/Cargo.toml` | native dependency tokio `sync` (the action gate) |
| `Cargo.lock` | follows the two lines above |
| `apps/mailbox-ui/src/adapters/mod.rs` | `pub mod actions;` |
| `apps/mailbox-ui/src/adapters/account.rs` | `sync_account` replays queued operations then syncs all folders; `snapshot_hiding` (all folders, role key or folder id in `folder_id`); `SyncSummary.failed_folders`; `open_store`, `failure`, `WINDOW` now `pub(super)`. `snapshot`, `save_and_sync`, `resync_all`, `load_real` keep their signatures |
| `apps/mailbox-ui/src/state/mod.rs` | `pub mod folders;`, `ConfirmAction::DeleteForever` and its arm in `run_confirm` |
| `apps/mailbox-ui/src/app.rs` | provides `RealState`, loads `folders.css` |
| `apps/mailbox-ui/src/components/mod.rs` | `pub mod folders;` |
| `apps/mailbox-ui/src/components/common.rs` | toast undo calls `state::folders::undo` |
| `apps/mailbox-ui/src/components/shell.rs` | real-mode sidebar (`RealFolderNav`), inbox-only account counts in Real mode, real-mode status text (the old line mentioned MB-130: likely one-line conflict with Track A) |
| `apps/mailbox-ui/src/screens/inbox.rs` | rows filtered by real folder; star, read, archive, delete handlers call `state::folders`; `RowOpBadge`, `MoveMenu`, `data-op` |
| `apps/mailbox-ui/src/screens/thread.rs` | archive, delete, star handlers call `state::folders` |
| `apps/mailbox-ui/src/main.rs` | four SSR tests and two helpers appended to `ui_tests` |
