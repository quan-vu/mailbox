# Track D — live sync worker and status UI (MB-140, MB-141)

Branch `claude/mb-140-live-sync-38f9f8`, cut from `main` at `d13ce9d`. Desktop Real mode only;
Demo mode and the Web build are unchanged (Web compiles the shared types and status components,
but no worker exists there).

## What was built

| Area | Files | What it does |
|---|---|---|
| State machine (pure) | `crates/mailbox-core/src/live.rs` | `LinkState = Connecting / Online(Idle / Polling) / Syncing / Offline{retry_at} / NeedsAttention(AuthFailed / Certificate / Storage)`, `AccountLive` (state, last sync, failed folders, change generation), `classify` (error → retry vs wait-for-user), `Backoff` (exponential, equal jitter in `[d/2, d]`, base 2 s, cap 5 min). Constants: IDLE re-issue 25 min, poll 2 min, other folders 10 min. |
| IDLE | `crates/mailbox-imap/src/idle.rs` | `Session::supports_idle`, `Session::idle(max, interrupt)`: waits for `EXISTS` / `EXPUNGE` / `FETCH` / `VANISHED`, `max`, or an interrupt; every exit sends `DONE` and reads the tagged completion before returning. Cancel-safe wait (`fill_buf`). `BYE` / EOF → `Unreachable` (a state). NO/BAD to `IDLE` → `Refused` (worker polls instead). |
| Worker + connection manager | `crates/mailbox-imap/src/live.rs` | `worker(store, owner, account, connector, config)` → `(LiveHandle, future)`. One long-lived session per account. On connect: replay pending ops once, sync all folders with flags. Then IDLE on INBOX (pre-IDLE `EXAMINE` compares UIDVALIDITY/UIDNEXT with the store to close the race) or poll; a change runs `sync_folder` + `pull_flags` for INBOX; other folders on the rotation (IDLE wakes for it). `LiveHandle::session()` lends the connection to other use cases: a waiting borrower makes the worker end IDLE (`DONE`) and the fair mutex hands it over; `Borrowed::discard()` drops a broken connection and makes the worker reconnect. Transient failure → `Offline` + backoff; auth / certificate / storage → `NeedsAttention`, no retry until `RetryNow` / `SyncNow`. |
| UI registry | `apps/mailbox-ui/src/adapters/worker.rs` | Starts one worker per saved account on the Desktop tokio runtime (`ensure_running`), stops removed ones, `stop_all` on leaving Real mode, `retry_now`, `sync_all`, `snapshot`, `changed`. The connector reads the password from the OS keychain at every attempt (missing entry → re-auth). |
| Routed through the manager | `adapters/actions.rs` (`flush_locked` → `flush_on`), `adapters/body.rs` (`fetch_raw` → `fetch_on`) | Queued flag/move/delete replay and body fetch borrow the live session; with a worker that is offline, ops stay queued and the worker replays them on reconnect. Without a worker the old per-call connection path is kept (public functions unchanged). |
| Status UI | `state/live.rs`, `components/live.rs`, `assets/status.css`, edits in `components/shell.rs`, `screens/settings.rs`, `app.rs` | Top bar: `topbar-real-status` gets `data-live` (worst account state) and a `topbar-live-status` chip. Sidebar: per account `account-live-<id>` line with state, "Đồng bộ lần cuối HH:MM UTC", partial folder failures, `account-retry-<id>` ("Thử lại ngay"), and for auth failure `account-reauth-<id>` ("Nhập lại mật khẩu", opens the connect form; nothing prefilled). Settings → Đồng bộ in Real mode lists real per-account rows (`settings-live-status`). Demo mode renders none of it. |
| Sync does not fight the user | `state/live.rs::pump`, `state/folders.rs::refresh` (made `pub(crate)`), `state/mod.rs::sync_now` | A worker change bumps `generation`; the pump reloads the list through `folders::refresh`, which keeps the open message selected and touches no focus. The refresh button asks workers for a full sync instead of opening its own connection. Pending flag changes keep their local value (`pull_flags` conflict rule, tested). |

## Commands and results (real output)

TDD order: `live.rs` (core) tests were written together with the code, not before (said
plainly). `tests/idle.rs` was written first and failed to compile — RED:

```
   8 error[E0599]: no method named `idle` found for struct `Session<S>` in the current scope
   1 error[E0432]: unresolved import `mailbox_imap::idle`
   2 error[E0599]: no method named `supports_idle` found for struct `Session<S>` in the current scope
```

then GREEN after `idle.rs`: `cargo test -p mailbox-imap --test idle` → `6 passed`. The worker
tests (`crates/mailbox-store/tests/live_worker.rs`) were written before the worker was finished;
first run 1 passed / 7 failed (paused-clock + sqlx problem, see below), then 5/8, then 8/8; the
rotation test was added after a review fix. Five consecutive runs of `live_worker`: 8/8 each.
The UI SSR tests were written after the components.

### Final gate run (2026-11-15, after the last code change)

```
=== cargo fmt --all -- --check                                                            EXIT 0
=== cargo clippy --workspace --all-targets -- -D warnings                                 EXIT 0
=== cargo clippy -p mailbox-ui --no-default-features --features desktop -- -D warnings    EXIT 0
=== cargo clippy -p mailbox-ui --no-default-features --features web --target wasm32-unknown-unknown -- -D warnings   EXIT 0
=== cargo test --workspace                                                                EXIT 0
    sum over all test binaries: 304 passed, 0 failed, 18 ignored (opt-in real-provider tests)
```

The first clippy run failed on `clippy::never_loop` in `wait_after`; fixed before this run.

### Tests added

- `mailbox-core` `live::tests` (4): backoff bounds/cap/no-zero/bad jitter, fault classes, state keys and retry offer.
- `mailbox-imap/tests/idle.rs` (6, paused tokio clock): change ends IDLE with DONE (RECENT alone is not a change); EXPUNGE and FETCH FLAGS + events in the DONE completion; re-issue at exactly 25 min twice, DONE before the next command; interrupt; BYE and EOF → `Unreachable`; server without IDLE detected, refused IDLE → `Refused` without DONE.
- `mailbox-imap` `idle::tests` (1): untagged classification.
- `mailbox-store/tests/live_worker.rs` (9, real SQLite + real session code + scripted stateful server): new mail picked up via IDLE without refresh, one LOGIN for everything; IDLE re-issued on the configured timer; no-IDLE server polled on the interval; drop → reconnect with doubling delays (100/200 ms gaps at jitter 0) and the queued flag replayed exactly once; a move cut off mid-command is not sent again after reconnect (one `UID MOVE`); auth failure → `NeedsAttention`, one connect in 1 s (a 100 ms-base retry loop would make ~10), `RetryNow` reconnects; UIDVALIDITY change mid-session rebuilds INBOX (old epoch rows gone); borrower gets the connection right after `DONE`, and a pending local star survives a server flag push while another message's server change arrives; other folders synced on the rotation while idling.
- `mailbox-ui` `state::live::tests` (1) and `ui_tests` (5, appended): every status in top bar + account row + texts + retry/reauth controls; "online" never shown before a sync, partial failure shown on that account only; reauth control and empty password field; settings sync panel real rows; Demo mode shows no live states and keeps its demo copy.

### Opt-in real-provider test

`crates/mailbox-store/tests/real_live.rs` (`#[ignore]`): worker online → self-send one marked
message → wait until the worker stored it with no manual sync → delete exactly that message over
the borrowed connection (SELECT, UIDVALIDITY check, `UID STORE +FLAGS \Deleted`, `UID EXPUNGE uid`).

Run once (2026-11-15) with `MAILBOX_SMTP_ENV_FILE=…/.env.smtp MAILBOX_TEST_IMAP_HOST=mail2.provider.example MAILBOX_SMTP_SEND_TO=self cargo test -p mailbox-store --test real_live -- --ignored --nocapture`:

```
worker online after 0.5s in mode online-idle (0 folder(s) failed)
sent 1 marked message to the test mailbox itself
worker stored it 1.2s after the send, state online-idle
cleanup: removed with UID EXPUNGE
done; last state online-idle, generation 2
test real_new_mail_arrives_through_the_worker_without_a_manual_sync ... ok
```

The test provider advertises IDLE; the polling path was verified only against the scripted
server. Other existing real-provider tests were not re-run.

## Why the worker tests use the real clock

`#[tokio::test(start_paused = true)]` failed with `Storage("open")`: SQLite work runs on its own
thread, tokio counts that as idle time and auto-advances, so the sqlx pool timeouts fire. The
worker tests therefore run on the real clock with millisecond settings and slack bounds. The
production 25-minute re-issue is asserted exactly on a paused clock at the IMAP layer
(`tests/idle.rs`), where no SQLite is involved.

## Not verified

- No click-through of the Desktop app (no `dx serve` / bundle per the brief). Status rendering
  is SSR-tested; the pump/refresh loop and tokio spawning were compiled (desktop clippy) but not
  run inside the app.
- Sent-copy reconciliation (`adapters/send.rs`) still opens its own IMAP connection: left as a
  seam (`worker::handle(account).session()` would replace `Session::connect` there).
- `resync_all` / `save_and_sync` still use their own connection (first save, and the fallback
  when no worker runs).
- Focus is not touched by refresh by construction (it only sets signals); there is no automated
  focus test.

## Risks

- The worker holds the connection mutex during a full sync (up to 64 folders): a body fetch or
  action started then waits for it instead of opening a second connection.
- Spurious wake: if a borrower arrives while the worker is not idling, the next IDLE may end at
  once (one extra IDLE/DONE pair). Harmless, by design (no lost wake-ups).
- `last_synced` is in memory only; after a restart it is empty until the first sync.
- The settings "auto-sync" toggle and interval dropdown are still demo controls; the worker uses
  the constants.
- Background sync while the app is closed is not available (row added to `LIMITATIONS_1.0.md`).

## Shared files touched

- `crates/mailbox-core/src/lib.rs` — `pub mod live;`.
- `crates/mailbox-imap/src/lib.rs` — `pub mod idle; pub mod live;`.
- `crates/mailbox-imap/src/mutate.rs` — `MutableSession::reader_mut` / `reader` (read path on the same connection).
- `crates/mailbox-imap/Cargo.toml` — tokio `time, sync, macros, rt`; dev `test-util`.
- `crates/mailbox-store/Cargo.toml` — dev tokio `test-util, sync`.
- `apps/mailbox-ui/src/adapters/mod.rs` — `pub mod worker;` (native).
- `apps/mailbox-ui/src/adapters/actions.rs` — `flush_locked` borrows the live session; shared `flush_on`.
- `apps/mailbox-ui/src/adapters/body.rs` — `fetch_raw` borrows the live session; shared `fetch_on`.
- `apps/mailbox-ui/src/state/mod.rs` — `pub mod live;`, start/stop workers, `sync_now` via workers, retry after re-entered password.
- `apps/mailbox-ui/src/state/folders.rs` — `refresh` → `pub(crate)`.
- `apps/mailbox-ui/src/components/mod.rs` — `pub mod live;`.
- `apps/mailbox-ui/src/components/shell.rs` — top bar `data-live` + chip, account live line.
- `apps/mailbox-ui/src/screens/settings.rs` — Sync panel shows real rows in Real mode.
- `apps/mailbox-ui/src/app.rs` — `status.css`, status pump.
- `apps/mailbox-ui/src/main.rs` — `ui_tests` appended (helpers prefixed `live_`).
- `docs/06-delivery/LIMITATIONS_1.0.md` — one row appended.
