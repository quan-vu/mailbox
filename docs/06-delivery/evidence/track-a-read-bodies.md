# Track A evidence — MB-130 (message bodies) and MB-131 (safe HTML, attachments)

Branch `claude/mb-130-read-bodies-e5aa44`, 2026-11-15, macOS aarch64, rustc 1.97.0. Desktop only; the Web build compiles but has no body source (real mode on Web is still empty).

Status: code and automated tests are complete. **Not DONE by the repository's definition**: the reader was never opened in a running app (see "Not verified").

## What was built

| Area | Files | Content |
|---|---|---|
| MIME | `crates/mailbox-mime` (new crate) | `parse()` on `mail-parser` with our limits on top; `attachment_bytes()`; `safe_filename()`; `sanitize()` on `ammonia` |
| IMAP | `crates/mailbox-imap/src/body.rs` | `Session::fetch_body(uid, max_bytes)`: `UID FETCH n (RFC822.SIZE)`, then `UID FETCH n (BODY.PEEK[])` |
| Port | `crates/mailbox-core/src/body.rs` | `BodyStore`, `BodyRecord`, `AttachmentMeta`, `MessageLocator` |
| Storage | `crates/mailbox-store/src/body.rs`, `migrations/*/0002_message_bodies.sql` | `BodyStore` for SQLite and PostgreSQL |
| UI adapter | `apps/mailbox-ui/src/adapters/body.rs` | `load_body`, `save_attachment`, `frame_document`, view types |
| UI | `apps/mailbox-ui/src/components/body.rs`, `assets/reader.css` | `MessageBody` (demo: fixture text; real: `BodyPane`) |

Limits (`mailbox_mime::Limits::default()`): message 25 MiB, nesting depth 10, 200 parts, attachment 25 MiB, text 1 MiB (cut), HTML 2 MiB (dropped, text shown), inline images 1 MiB each and 2 MiB per message. Errors: `TooLarge`, `TooDeep`, `TooManyParts`, `AttachmentTooLarge`, `NoSuchAttachment`, `Malformed`.

New `data-testid`s: `message-body`, `body-loading`, `body-error`, `body-retry`, `body-too-large`, `body-truncated`, `body-remote-blocked`, `body-show-images`, `body-images-shown`, `body-hide-images`, `body-view-toggle`, `body-html-frame`, `body-text`, `body-attachments`, `attachment-real-{part}`, `attachment-save-{part}`. Existing ids are unchanged.

## Decisions

- **Attachment bytes are fetched again on demand, never stored.** Reasons: `PRIVACY.md` asks for attachment files only when needed; the SQLite file is not encrypted; a cache of executable content in the app data directory is a risk with no benefit yet. Cost: saving an attachment downloads the whole message again (at most 25 MiB). Only name, type, size, content id and the inline flag are stored (existing `attachments` table plus two columns).
- **Two sanitized HTML variants are stored**: one with every remote reference removed, and, only when the message has remote `<img>`, one that keeps those image sources. `ammonia` cannot rename an attribute, so "show images" switches variant instead of rewriting HTML in the UI. Cost: such a message is stored twice.
- **`mail-parser` is pinned to `=0.11.9`**, not 1.0.x: 1.0 needs Rust 1.98 and this machine has 1.97. `ammonia` is pinned to `=4.2.5`. Both are in `DEPENDENCIES.md`.
- **Save location is the Downloads folder** (`MAILBOX_DOWNLOAD_DIR` overrides it). There is no folder picker: that needs a new native dependency.
- **`<style>` blocks are removed entirely**; inline `style` attributes are kept after filtering. Mail that depends on a style sheet looks plainer.
- **The frame has a fixed height** (70% of the window, at most 640 px) and scrolls inside: without scripts it cannot size itself to the content.

## TDD record

- `mailbox-mime`: tests written against stubs first. RED: `cargo test -p mailbox-mime --no-fail-fast` → 17 failed, 4 passed (the 4 only check "does not panic"). GREEN after implementation: 21 passed.
- `mailbox-imap` body: RED with a stub → 2 failed, 1 passed. GREEN: 3 passed (+1 unit test).
- Store contract tests and UI tests were written together with the code; no separate RED run exists for them. Two UI tests failed on first run because the assertions expected `&lt;` where `dioxus-ssr` writes `&#60;`; the assertions were corrected, the component was not changed.

## Commands and results (final state)

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo test --workspace` | exit 0; 136 passed, 0 failed, 13 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo clippy -p mailbox-ui --no-default-features --features desktop -- -D warnings` (also with `--all-targets`) | exit 0 |
| `cargo check -p mailbox-ui --no-default-features --features web --target wasm32-unknown-unknown` | exit 0 |
| `cargo clippy -p mailbox-ui --no-default-features --features web --target wasm32-unknown-unknown -- -D warnings` | exit 0 |
| `cargo tree -p mailbox-ui --features web --target wasm32-unknown-unknown` filtered for `mail-parser`, `ammonia`, `mailbox-mime`, `mailbox-imap` | 0 matches: none of them is in the Web bundle |

`cargo test --workspace` ran before two lint-only attribute edits in `adapters/body.rs`; both clippy runs and the wasm check ran after them.

New tests: `mailbox-mime` 21 (parser 13, sanitizer 7, file names 1); `mailbox-imap` 4; `mailbox-store` 3 (+1 PostgreSQL, ignored; +1 real provider, ignored); `mailbox-ui` 11 (adapter 6, `ui_tests` 5; 47 in total).

Real provider, read-only (the test loads the env file itself; nothing from it was read or printed by the agent):

```
MAILBOX_SMTP_ENV_FILE=<repo>/.env.smtp MAILBOX_TEST_IMAP_HOST=mail2.provider.example \
  cargo test -p mailbox-store --test real_body -- --ignored --nocapture
message: 2149 bytes raw; text 168 bytes (plain part: true); html: 0; remote blocked: false; attachments: 0; truncated: false
bodies fetched: 1; with html: 0; with attachments: 0; with blocked remote content: 0
test real_message_body_is_fetched_parsed_and_cached_read_only ... ok
```

It also checks that `RFC822.SIZE` equals the literal length and that the `\Seen` flag is unchanged after the fetch.

## Not verified

- **The reader in a running app.** `dx serve`, Playwright and the installed app were off limits for this track. Loading, retry, the frame, "show images", view switch and saving an attachment have never been clicked. Everything UI-related rests on `dioxus-ssr` output.
- **Link clicks inside the frame.** From the sources (`dioxus-desktop` 0.7.10 `webview.rs`, `wry` 0.53.5 `wkwebview/navigation.rs`): on macOS every frame navigation goes through the Dioxus navigation handler, which opens `http`, `https` and `mailto` in the system browser and cancels the navigation in the WebView. This was read, not tested. On Windows (WebView2) frame navigation may be handled differently, so a clicked link could load inside the frame there.
- **That the sandbox is active before content loads** in WKWebView. `sandbox=""` is a static attribute and `srcdoc` a dynamic one, and the frame is re-created per message and mode; not observed in a real WebView.
- **Real HTML mail and real attachments.** The test mailbox holds one plain-text message. HTML, inline images, attachments and Vietnamese legacy charsets are covered by synthetic fixtures only.
- **PostgreSQL.** `postgres_meets_the_body_contract` did not run: the disk was full (below 300 MB free), so no container was started. Migration `0002` for PostgreSQL is untested.
- **Fuzzing.** The hostile corpus is hand-written plus 300 pseudo-random inputs; no coverage-guided fuzzing.
- **Sanitizer speed.** The pathological-HTML test takes 8 to 14 s in a debug build (20,000 nested elements). Release-build timing and a 2 MiB hostile document were not measured; sanitising runs on the UI task.
- **macOS quarantine attribute** on saved files is set best-effort through `/usr/bin/xattr`; not checked by a test.

## Security notes

- IMAP: only `UID FETCH ... (RFC822.SIZE)` and `UID FETCH ... (BODY.PEEK[])` were added; the dialogue test asserts no `SELECT`, `STORE`, `EXPUNGE`, `DELETE`, `APPEND`, `COPY`, `MOVE` and no `BODY[` without `PEEK`. The announced size is checked before the body is requested and the literal length again before any byte is read or allocated; a lying server gets `TooLarge`. The adapter compares UIDVALIDITY before fetching.
- TLS: unchanged; bodies use the existing `Session::connect` over `mailbox-net`. No bypass was added.
- HTML: allow-list of tags and attributes; `href` only on `<a>` with `http`, `https`, `mailto`, plus `rel="noopener noreferrer"`; `src` only on `<img>` and only `cid:` that resolves to an image in the same message (embedded as `data:`), small raster `data:` images, or remote `http(s)` in the opt-in variant. `srcset`, `background`, `poster`, relative URLs, `<style>`, `<link>`, `<base>`, `<meta>`, forms, frames, objects, SVG and MathML are removed. A `style` attribute with a backslash, a comment or markup is dropped whole; declarations containing `url`, `image`, `expression`, `behavior`, `@`, `//`, `var(` and similar are dropped one by one.
- Isolation: `iframe` with an empty `sandbox` attribute (no scripts, no same origin, no forms, no popups) and `srcdoc`; CSP meta `default-src 'none'; img-src data: cid:; style-src 'unsafe-inline'`. After "show images" for that one message: `img-src data: cid: https: http:`. The choice is kept in memory per message and is gone after switching message or restarting.
- Attachments: names are reduced to one path component in the parser and again when writing; files are created with `create_new` (never overwrite), mode `0600`, never opened or executed. A test proves `../../../evil.sh` lands as `evil.sh` inside the target folder and nowhere above it.
- Logging: none added. Errors are enums without content; UI messages contain no address, host or mail text.
- Data at rest: decoded text and sanitized HTML are now in the unencrypted SQLite file (`PRIVACY.md` updated). No secrets are stored; a schema test rejects `password`, `secret`, `token`, `blob`, `bytea` in migration `0002`.
- Demo and real stay apart: in demo mode `MessageBody` renders the fixture text and starts no load; a test asserts no real-mode control appears in demo.

## For the coordinator

- Migration `0002` adds two columns to the existing `attachments` table (`content_id`, `is_inline`). If another track also alters that table, check the merge.
- Shared files touched are listed in the final report of this track; all edits are a `mod` line, a dependency line, two stylesheet lines, or one replaced line per screen.
- `docs/06-delivery/TRACKING.md` and `CHANGELOG.md` were not edited, as instructed.
