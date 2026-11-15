# Track G evidence — scheduled send and compose/send gaps (MB-218, MB-151, MB-150, MB-133, MB-140)

Branch `claude/mb-sched-compose-84cd3a`, 2026-11-15, macOS aarch64, rustc 1.97.0. Desktop Real mode;
Web stays demo-only (every new control is either absent or disabled and labelled there).

## What was built (real)

| Item | Where | Behaviour |
|---|---|---|
| MB-218 scheduled send — domain | `crates/mailbox-core/src/outbox.rs` | New state `Scheduled` and events `Schedule`, `Unschedule`, `Due`, `Missed`; `DraftRecord::scheduled_at` (UTC epoch s); `SendIntent::Scheduled` is accepted only from `Scheduled`, and `Send`/`Retry`/`ResendAfterUnknown` cannot bypass a schedule. `check_schedule` (≥ 60 s ahead, ≤ 366 days), `due_action` (wait / send / missed), `take_due` (marks rows more than `MISSED_GRACE_SECS` = 12 h late as `Failed { MissedSchedule, retryable }`, returns due rows and the next time). A scheduled row without a time is marked missed, never sent on a guess. Recovery leaves `Scheduled` alone. New `ScheduleStore` trait (no `DraftStore`/`MailStore` signature changed). |
| MB-218 — storage | `migrations/{sqlite,postgres}/0007_scheduled_send.sql`, `crates/mailbox-store/src/outbox.rs` | `drafts.scheduled_at` (INTEGER / BIGINT) + index. `schedule_draft` / `unschedule_draft` are single conditional UPDATEs (compare-and-set); content saves stay refused outside `draft`; any transition back to `draft` clears the time. |
| MB-218 — UI | `apps/mailbox-ui/src/state/schedule.rs`, `components/schedule.rs`, `adapters/clock.rs`, `assets/schedule.css`, `state/outbox.rs`, `adapters/send.rs` | Composer button "Hẹn giờ gửi" (replaces the disabled MB-218 placeholder) opens a picker: presets "Sau 1 giờ", "8:00 sáng mai", `datetime-local` input, zone line ("Giờ theo máy này (UTC+07:00)" or "…tính theo UTC"), the rule text, errors. The draft is saved first, then scheduled; blockers are the real send blockers. While scheduled the composer is locked and shows the time, the rule, "MailBox không gửi được thư khi ứng dụng đang đóng.", "Hủy hẹn giờ để sửa", "Gửi ngay" (cancel + normal send). Outbox rows show "Đã hẹn giờ", "Gửi lúc …", "Hủy hẹn giờ"; the outbox header states the rule. A loop (`state::schedule::run`, started in `app.rs`, Real mode only, ≤ 15 s tick) sends due rows through the same `submit` path (state written before the network, unknown stays unknown, toast for sent / unknown / failed) and saves the Sent copy. Local offset from `localtime_r` per instant (DST-correct); other platforms fall back to labelled UTC. |
| Missed rule (stated in UI) | `state::schedule::RULE_TEXT` | "Thư hẹn giờ chỉ được gửi khi MailBox đang mở. Nếu lúc đến giờ MailBox đang đóng hoặc máy đang ngủ, thư được gửi ngay khi MailBox mở lại trong vòng 12 giờ sau giờ hẹn; trễ hơn 12 giờ, thư không tự gửi mà chờ bạn quyết định." A test keeps the hour count equal to `MISSED_GRACE_SECS`. Missed rows show the `MissedSchedule` reason with "Thử lại" (explicit retry) or "Sửa". |
| Sent copy through the live worker | `crates/mailbox-imap/src/borrow.rs` (new), `adapters/send.rs` | `live_or_fresh(handle, job, fresh)`: runs a `RepeatableJob` on the worker's held session when connected, else on its own connection (logged out after). If the borrowed connection breaks (`AppendError::Connection`) it is discarded (worker reconnects) and the job runs once more on a fresh connection. `reconcile_sent_copy` (Sent copy: Message-ID search first, then APPEND) and `check_sent_folder` (read-only search) now go through it. Folder: persisted role first, server LIST fallback inside the job. |
| Forward keeps attachments | `state/outbox.rs` (`attach_forwarded`, `forward_attachments`, `ForwardFiles`), `components/forward.rs`, `adapters/body.rs` (`cached_attachments`, `attachment_data`), `adapters/send.rs` (`forwarded_bytes`, `prepare_with`), core `AttachmentRef::forwarded` + `ForwardedPart`, store JSON `fwd_msg`/`fwd_part` | Opening a forward in Real mode reads the cached body's attachment list (no network) and adds its non-inline attachments as forwarded references (shown as "Chuyển tiếp từ thư gốc — tải lại từ máy chủ khi gửi"). Not cached: "Nội dung thư gốc chưa được tải về máy này … tệp đính kèm (nếu có) chưa được chuyển tiếp. Mở thư gốc để tải nội dung, rồi bấm Chuyển tiếp lại." At send time the original is fetched once per message (live session when available) and the parts extracted with the MIME limits; size limit applies to the total; message gone → `AttachmentMissing` (not sent), network → retryable `Network`. Attachment bytes are still never stored. |
| Auto-sync setting | `crates/mailbox-core/src/prefs.rs` (`auto_sync`, default `true`, old records read as `true`), `adapters/worker.rs` (`set_auto_sync`, gate in `ensure_running`), `state/prefs.rs::apply_prefs`, `screens/settings_sync.rs`, `screens/settings.rs`, `state/mod.rs` (settings loaded before workers start) | Real-mode Sync panel switch bound to the stored preference. Off: every worker is stopped at once and none is started on launch; a note says manual "Đồng bộ" still works (it falls back to `resync_all` when no worker runs, unchanged). On: workers start again. Saved with "Lưu thay đổi" like the other settings. Demo keeps its in-memory toggle. |

## Mocked / not built

| Item | State |
|---|---|
| Server Drafts sync (scope item 4) | **Not built**, drafts stay local. The outbox header already says "Bản nháp chỉ lưu trên máy này; chưa đồng bộ lên thư mục Nháp của máy chủ". Reason: a safe version needs own-UID tracking under UIDVALIDITY, UIDPLUS-only deletes, a no-UIDPLUS path and a rule for edits made in another client; not doable safely in this track's time. LIMITATIONS row added. |
| Sending while MailBox is closed | Not possible; labelled in the composer; LIMITATIONS row. |
| Time zone on non-Unix | UTC, labelled; LIMITATIONS row. |

## Tests

New or changed, all written before or with the code (core RED seen: two failures on first run, fixed — a saturating-add bug in `check_schedule` and a fixture id).

- `mailbox-core` `outbox::tests`: transition table extended with every new state/event pair; `a_send_time_must_be_ahead_but_not_absurdly_far`, `due_rule_sends_within_the_grace_period_and_holds_after_it`, `scheduling_freezes_the_draft_and_cancel_brings_it_back`, `due_messages_are_sent_once_and_missed_ones_wait_for_the_user`, `a_scheduled_send_that_breaks_off_is_unknown_like_any_other`. `prefs::tests::auto_sync_defaults_on_and_older_records_keep_it_on`.
- `mailbox-store` `tests/schedule.rs`: SQLite contract (frozen content, owner scoping, reschedule/cancel, send once, cancel refused after start, missed, time cleared on back-to-draft), restart survival + recovery untouched, forwarded attachment round trip; PostgreSQL contract `#[ignore]` (not run: no PostgreSQL here).
- `mailbox-store` `tests/live_worker.rs` (scripted server extended with APPEND literal + `UID SEARCH HEADER Message-ID`): `track_g_sent_copy_uses_the_held_session_and_falls_back_without_duplicates` — live route with no new LOGIN; second run `AlreadyThere`; no worker → fresh connection + LOGOUT; held connection killed right after APPEND → `LiveThenFresh`, exactly one copy.
- `mailbox-ui`: `state::schedule::tests` (civil dates, strict input parsing, UTC+7 round trip, DST offsets per date, presets across local midnight, rule text vs grace, wait bounds), `adapters::clock::tests` (system offset plausible), `state::outbox::tests::track_g_forward_takes_real_attachments_and_skips_inline_images`, `adapters::send::tests::track_g_forwarded_attachments_use_the_fetched_bytes_and_never_a_guess`; SSR `ui_tests` appended: `track_g_real_composer_offers_scheduling_and_demo_does_not`, `track_g_scheduled_draft_is_locked_and_states_the_rule`, `track_g_outbox_lists_scheduled_items_with_time_and_cancel`, `track_g_a_missed_schedule_waits_for_the_user_with_a_retry`, `track_g_auto_sync_setting_is_bound_to_the_stored_preference`, `track_g_forward_carries_the_original_files_or_says_why_not`.
- Real provider (new, opt-in) `crates/mailbox-store/tests/real_sent_copy.rs`: APPENDs one message made by the test (unique Message-ID, no mail sent) to Sent over the worker's held connection, runs again, deletes exactly that UID.

No Playwright: no Web flow exists for these features (Web is demo-only; TEAM_RULES forbids running Playwright/`dx serve` in worker sessions).

## Commands and results

All five TEAM_RULES gates, run on the final tree (commit after `prepare` was made test-only),
`CARGO_TARGET_DIR` shared with the main checkout:

```
$ cargo fmt --all -- --check                                                         exit 0
$ cargo clippy --workspace --all-targets -- -D warnings                               exit 0
$ cargo clippy -p mailbox-ui --no-default-features --features desktop -- -D warnings  exit 0
$ cargo clippy -p mailbox-ui --no-default-features --features web --target wasm32-unknown-unknown -- -D warnings   exit 0
$ cargo test --workspace                                                              exit 0
  58 test binaries: 469 passed, 0 failed, 25 ignored (opt-in real-provider / PostgreSQL)
  mailbox-ui (bin, default features): 143 passed; 0 failed
```

(An earlier gate run failed workspace/desktop clippy with "function `prepare` is never used"; fixed by
`#[cfg(test)]` on the test-only wrapper.)

Real provider:

```
$ MAILBOX_SMTP_ENV_FILE=<repo>/.env.smtp MAILBOX_SMTP_SEND_TO=self MAILBOX_TEST_IMAP_HOST=mail2.provider.example \
    cargo test -p mailbox-store --test real_sent_copy -- --ignored --nocapture
running 1 test
first run: Ok(true) via Live
second run: already there = true
cleanup: Removed
worker state online-idle, generation 1 -> 1
test real_sent_copy_uses_the_worker_connection_and_makes_one_copy ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.11s
```

Flaky timing tests observed (pre-existing, not changed by this track): with the machine at load
average ~86 (other sessions building), `live_worker::other_folders_follow_on_the_slower_rotation_while_idling`
(and once `idle_is_reissued_on_the_configured_timer` / `a_server_without_idle_is_polled_on_the_interval`)
failed their 150 ms slack in 4 of 6 runs of `--test live_worker` ("rotation after 506.8555ms"); the
full `cargo test --workspace` gate run recorded above passed them. The track-G test in that file
passed every run.

## Not verified

- No Desktop click-through (picker, toasts, loop timing, settings switch) — no GUI session; SSR tests
  cover rendering only. The schedule loop itself (`state::schedule::run`) is not unit-tested; the
  pieces it calls (`take_due`, `submit`, `next_wait`) are.
- No real SMTP send of a scheduled message or of a forward with attachments (the SMTP path is the
  unchanged MB-151 `submit`; forwarded bytes are tested at the `prepare_with` level only).
- `attachment_data` / `cached_attachments` (body adapter) have no scripted-server test of their own;
  they reuse `fetch_raw` and `mailbox_mime::attachment_bytes`, which have tests in track A.
- PostgreSQL schedule contract not run. Windows time zone not run.

## Risks

- Clock changes: the loop compares wall-clock UTC; a user changing the system clock can make a
  message due early or late (still bounded by the 12 h missed rule).
- A forward's attachment is fetched from the message's current UID: if the original was moved,
  the send fails "attachment missing" instead of following it.
- Auto-sync off takes effect immediately when toggled (like the other settings' live preview); it
  persists only after "Lưu thay đổi".
- `apps/mailbox-ui/src/screens/compose.rs` and `screens/outbox.rs` got small inserted lines; track H
  (i18n) also edits strings there — expect trivial merge conflicts.

## Proposed TRACKING rows (TEAM_RULES: workers do not edit TRACKING.md)

| Task | Proposed status |
|---|---|
| MB-218 Scheduled send | DONE (Desktop; click-through pending; closed-app send not possible, see LIMITATIONS) |
| MB-151 Sent reconciliation | note: Sent copy and Sent check now on the held worker session with fallback |
| MB-150 Composer | note: forward keeps original attachments (needs cached body) |
| MB-133 Local drafts | unchanged: server Drafts sync not built (LIMITATIONS) |
| MB-140 Live sync | note: stored auto-sync setting wired (off = no workers, manual sync works) |

## Shared files touched

- `apps/mailbox-ui/src/app.rs`: start the schedule loop (2 lines).
- `apps/mailbox-ui/src/state/mod.rs`: `pub mod schedule;`; load settings before starting workers (auto-sync).
- `apps/mailbox-ui/src/adapters/mod.rs`, `components/mod.rs`, `screens/mod.rs`, `crates/mailbox-imap/src/lib.rs`: module lines.
- `apps/mailbox-ui/Cargo.toml` / `Cargo.lock`: `libc = "0.2"` for `cfg(unix)` (already in the lock tree).
- `apps/mailbox-ui/src/main.rs`: tests appended only.
- `crates/mailbox-core/src/prefs.rs`: `auto_sync` field (additive, serde default).
- `apps/mailbox-ui/src/screens/settings.rs`: Real mode uses the stored auto-sync switch.
- `crates/mailbox-store/tests/live_worker.rs`: scripted server handles APPEND/UID SEARCH; one test appended.
- `apps/mailbox-ui/src/adapters/body.rs`: two additive functions for forwarding.
