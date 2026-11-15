# Track C evidence — drafts, composer, SMTP send and Sent reconciliation (MB-133, MB-150, MB-151)

Branch `claude/mb-150-compose-send-22e6ff`, 2026-11-15, macOS aarch64, rustc 1.97.0. Desktop real mode only; Demo mode still sends nothing.

## What was built

| Area | Where | Notes |
|---|---|---|
| Address rules + MIME builder | new crate `crates/mailbox-compose` (pure, compiles for Web and Desktop) | `address` (validate, parse `Name <addr>`, split lists), `message` (RFC 5322/MIME: Date, From, To, Cc, Subject, Message-ID, In-Reply-To, References; `multipart/alternative`; `multipart/mixed` with base64 attachments, RFC 2231 file names, type by extension, size caps), `reply` (subject prefixes, quoting). Bcc is envelope-only. Deterministic: date and Message-ID are passed in. |
| SMTP submission | `crates/mailbox-smtp/src/submit.rs` | Several recipients, one result per recipient, `SIZE`, `BODY=8BITMIME` with 7-bit fallback, `SMTPUTF8` required for non-ASCII addresses, dot-stuffing on bytes. `Accepted / Rejected / Unknown` unchanged. Existing `send`/`check` untouched. |
| IMAP write path | `crates/mailbox-imap/src/append.rs` | `APPEND` (literal, `\Seen`/`\Draft`, `APPENDUID`), `UID SEARCH HEADER Message-ID` after read-only `EXAMINE`, `save_sent_copy` (search first, append only if absent), UID-scoped delete (needs UIDPLUS, never plain `EXPUNGE`), `find_folder` (SPECIAL-USE, then name). |
| Outbox state machine | `crates/mailbox-core/src/outbox.rs` | `Draft -> Queued -> Sending -> Sent / Failed{reason, retryable} / Unknown`, `DraftStore` and `Transport` ports, `submit`, `recover`, `decide`. |
| Draft store | `crates/mailbox-store/src/outbox.rs`, `migrations/{sqlite,postgres}/0003_outbox.sql` | Extends the v1 `drafts` table. Versioned saves, Message-ID fixed at creation, compare-and-set state, owner isolation. |
| Desktop adapter | `apps/mailbox-ui/src/adapters/send.rs` | Save/list/discard, `prepare` (reads attachments at send time), SMTP transport, Sent reconciliation, Sent-folder check for `Unknown`, separate SMTP password in the keychain (`smtp.<account-id>`). |
| UI | `state/compose.rs`, `state/outbox.rs`, `screens/compose.rs`, `screens/outbox.rs`, `assets/compose.css` | Real From picker, chips validated by the builder's rules, reply/reply-all/forward prefill with quoting and threading, file picker, truthful autosave indicator, send states, drafts/outbox list under the existing Drafts sidebar entry, discard confirmation, SMTP password prompt. |

### Decisions worth knowing

- **One refused recipient stops the send before DATA** (`PartialPolicy::AbortIfAnyRejected`). The user sees every refused address and nothing was sent to anybody. `SendToAccepted` exists and is tested but the UI does not use it.
- **`Sending` is written to SQLite before the socket is opened.** A crash leaves `Sending`; the first store access of the next run turns it into `Unknown`. `recover` has no transport parameter, so it cannot send.
- **`Unknown` is never a plain retry.** `submit` refuses it unless the intent is `ResendAfterUnknown`, which the UI only sends from a confirmation dialog that warns about a duplicate. The other choices are "check Sent folder" (Message-ID search), "it arrived, mark as sent" and "edit".
- **Message-ID** is generated on the first save (`<time.random.counter@sender-domain>`), stored with the draft and cannot be changed by later saves, so a resend is detectable.
- **Content is frozen once Send is pressed**: the store refuses content saves unless the state is `draft`.
- **File picker**: `<input type="file">`, which Dioxus Desktop opens as the native dialog through its own `rfd` dependency. No new direct dependency. Only the path is stored; the file is read when sending.
- **Server Drafts sync is not in this slice.** Drafts are local only; the list says so. Reason: replacing the previous server copy safely needs UIDPLUS plus a recorded server UID per draft, and a failure between append and delete would leave duplicates. The building blocks (`append` with `\Draft`, `delete_uids`) are there and tested.

## Tests first

Each slice had its tests written and run before the implementation:

| Slice | RED | GREEN |
|---|---|---|
| `mailbox-compose` | compile errors: modules `address`, `message`, `reply` missing | 24 passed |
| SMTP `submit` | `unresolved import mailbox_smtp::submit` | 11 scripted + 2 unit passed (3 first failed on a test-harness `unwrap`, fixed in the harness) |
| IMAP `append` | `unresolved import mailbox_imap::append` | 8 scripted + 2 unit passed (1 first failed on a wrong modified-UTF-7 fixture) |
| Outbox | 10 of 11 tests failed on `todo!()` stubs | 11 passed (transition table test was missing one row; the code was right) |
| Draft store | `SqliteStore: DraftStore is not satisfied` | 2 passed on SQLite, 1 on PostgreSQL |
| UI screens | 9 new dioxus-ssr tests failed on missing controls | 62 passed |

The pure UI state and adapter unit tests were written in the same pass as their code, not before it.

## Commands and results (final tree)

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --all -- --check                      # no output
cargo test --workspace                          # 183 passed, 0 failed, 13 ignored
cargo clippy --workspace --all-targets -- -D warnings                                   # Finished, no warnings
cargo clippy -p mailbox-ui --no-default-features --features desktop -- -D warnings      # Finished, no warnings
cargo check -p mailbox-ui --no-default-features --features web --target wasm32-unknown-unknown   # Finished, no warnings
cargo clippy -p mailbox-ui --no-default-features --features web --target wasm32-unknown-unknown -- -D warnings   # Finished
```

Passed per target: mailbox-api 4, mailbox-compose 24, mailbox-contracts 5, mailbox-core 19, mailbox-imap 10 + append 8 + dialogue 7 + network 1 + session 1, mailbox-secrets 2, mailbox-smtp 7 + dialogue 11 + network 1 + submit 11, mailbox-store contract 3 + outbox 2 + sync 5, mailbox-ui 62.

PostgreSQL (throwaway `postgres:16-alpine` container on port 55439, stopped afterwards):

```sh
MAILBOX_TEST_PG_URL=postgres://postgres:test@127.0.0.1:55439/postgres cargo test -p mailbox-store -- --ignored postgres
# postgres_meets_the_same_contract ... ok ; postgres_meets_the_draft_contract ... ok
```

Real provider (test mailbox from `.env.smtp`, read inside the test process), run once:

```sh
MAILBOX_SMTP_ENV_FILE=<repo>/.env.smtp MAILBOX_SMTP_SEND_TO=self \
  cargo test -p mailbox-smtp --test real_send -- --ignored --nocapture
# smtp outcome: Accepted, 8-bit transfer: true
# sent copy: appended by MailBox
# arrived in INBOX: 1 message(s)
# cleanup: removed 2 test message(s)
# test self_send_arrives_and_sent_holds_exactly_one_copy ... ok   (16.03s)
```

One message, to the test mailbox itself, Vietnamese subject and body plus a small attachment. The second `save_sent_copy` returned `AlreadyThere`, Sent held exactly one copy, and only the two messages carrying this run's Message-ID were removed (UID-scoped).

Build note: the disk had about 1 GB free during the final runs. They were made with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_STRIP=false` (same for `TEST`) to fit; this changes debug info only.

## Not verified

- **No click-through of the Desktop app.** `dx serve`, the bundle and Playwright were out of bounds for this track. The native file dialog, the autosave timing, focus handling in the new dialogs and the layout of the new states were never seen running. UI evidence is dioxus-ssr only.
- The Playwright suite in `e2e/` was not run. `compose-real.spec.ts` targets Demo mode, whose test ids are unchanged.
- `Unknown`, partial RCPT refusal, 4xx/5xx, SIZE, missing 8BITMIME and SMTPUTF8 were exercised against scripted servers only. The real provider offered 8BITMIME and accepted everything.
- The SMTP password prompt path (server refuses the account password) was not exercised against a real server.
- Sent reconciliation was verified on one provider (Dovecot, does not auto-save). A provider that saves the sent copy itself a few seconds late could still get a second copy, because the check runs once, right after the send.
- Windows and Linux: nothing was run there.

## Gaps and follow-ups

- Reply threading uses the original's `Message-ID` only; its own `References` are not synced yet, so long threads get a one-element chain. `state::compose::threading` already accepts the original's references.
- A reply quotes the original only when its body is available. Real-mode envelopes carry a "not downloaded" notice, which is detected by its text in `quotable_body` and never quoted. Replace that check when body fetch lands.
- Forwarding does not re-send the original's attachments (they are not on disk); such attachments block a real send with a clear message.
- `adapters::send::forget_smtp_password` is not called yet: `adapters/account.rs` was off limits. Account removal should call it, otherwise a `smtp.<account-id>` keychain entry outlives the account.
- `find_folder` is the temporary Sent/Drafts lookup; swap it for the persisted folder roles from the flags/folders track.
- Plain text only: the builder supports an HTML alternative, the editor does not produce one.
- The Sent copy is rendered without a Bcc header, so the Sent folder does not show Bcc recipients.
- `send_jobs` (v1 schema) is still unused; the state lives on the draft row.
- Demo-mode discard still goes through the shared confirm; Real mode has its own dialog.
- Migration `0003` only adds columns and one index to `drafts`.

## Security notes

- TLS: every send and append goes through `mailbox-net` (validated certificates, no option to skip). `FailReason::Certificate` copy says it cannot be bypassed; a test checks that no failure text suggests weakening TLS.
- Header and command injection: CR, LF and NUL are rejected (not stripped) in subject, names, addresses, message ids, file names and content types; envelope addresses are checked again in the SMTP layer before any I/O; bare CR/LF in message bytes is refused; the IMAP Message-ID search only accepts ids that are safe inside a quoted string.
- Line length: every output line is at most 998 bytes; long or 8-bit text becomes quoted-printable, headers fold at 78.
- Credentials: passwords are read from the keychain at the point of use as `Secret`. The separate SMTP password is checked against the server first and stored only under `smtp.<account-id>`. Nothing is written to SQLite, settings or logs. The password field is masked, starts empty and is cleared from UI state on submit.
- Logging: no new log or trace statements. Errors are enum codes; SMTP reports keep reply codes, not server text. Refused recipient addresses are shown to the user in the UI only.
- Local data: draft subject, body, recipients and attachment paths are stored in plain text in the local SQLite file, like the envelope cache. Attachment contents are not copied.
- Attachment names are reduced to their base name; sizes are checked from metadata before a file is read.
- Deleting on the server is limited to explicit UIDs and requires UIDPLUS. The app itself never deletes mail in this slice; only the opt-in test does, for its own messages.
- Demo mode: `send_action` cannot return `Submit` in Demo mode (unit test), the dialog keeps `data-sent="false"`, and real drafts are not listed in Demo mode.

## Shared files touched

| File | Change |
|---|---|
| `Cargo.toml`, `Cargo.lock` | workspace member `crates/mailbox-compose` |
| `crates/mailbox-core/src/lib.rs` | `pub mod outbox;` |
| `crates/mailbox-imap/src/lib.rs` | `pub mod append;` |
| `crates/mailbox-smtp/src/lib.rs`, `Cargo.toml` | `pub mod submit;`; dev-dependencies for the real-provider test |
| `crates/mailbox-store/src/lib.rs`, `Cargo.toml` | `mod outbox;`; `serde_json` |
| `apps/mailbox-ui/Cargo.toml` | `mailbox-compose`; `tokio` (rt, time) on native |
| `apps/mailbox-ui/src/adapters/mod.rs` | `pub mod send;` (native only) |
| `apps/mailbox-ui/src/state/mod.rs` | `pub mod outbox;` |
| `apps/mailbox-ui/src/screens/mod.rs` | `pub mod outbox;` |
| `apps/mailbox-ui/src/routes.rs` | `Inbox` route shows `OutboxScreen` for Drafts in Real mode |
| `apps/mailbox-ui/src/main.rs` | 11 ssr tests appended to `ui_tests` |
| `docs/05-engineering/DEPENDENCIES.md` | dependency notes |
