# Track F evidence — settings persistence, templates/signatures/shortcuts, OAuth mock, autoconfig, notifications (MB-202, MB-203, MB-210, MB-211)

Branch `claude/mb-202-settings-features-01943e`, 2026-11-15, macOS aarch64, rustc 1.97.0. Desktop Real mode; Demo mode keeps its in-memory settings and labels.

## What was built

| Task | Where | Notes |
|---|---|---|
| Settings persistence | `crates/mailbox-core/src/prefs.rs` (types, `PreferencesStore`, template placeholders), `crates/mailbox-store/src/prefs.rs`, `migrations/{sqlite,postgres}/0006_settings.sql`, `apps/mailbox-ui/src/adapters/prefs.rs`, `state/prefs.rs`, `screens/settings_real.rs` | One versioned JSON record per owner in the existing v1 `preferences` table (key `settings`); templates in a new `message_templates` table (0006). Typed: theme, density, reduce-motion, default identity, per-account signature (+ "use for replies"), notification mode/sound/sync-errors, remote images (forced `false` on every load and save), shortcuts on/off. A record with a newer `version` or broken JSON is an error (defaults shown, stored record untouched). "Restore defaults" resets appearance, notifications, shortcuts and security; it keeps signatures and the chosen identity (text the user wrote). Real-mode panels: Appearance, Security, Signature (+ templates), Notifications, Accounts (default identity), Advanced (shortcuts). Unsaved changes show a dirty marker. Web build says nothing is stored. No secret column or field exists (test scans both migrations). |
| MB-210 templates | `state/prefs.rs` (`apply_template_to`), `screens/compose.rs` rail, `screens/settings_real.rs` CRUD | Placeholders `{{name}}`, `{{first_name}}`, `{{email}}` filled from the first valid recipient in a single pass: values are plain text, control characters become spaces, a value is never scanned again (a recipient named `{{email}}` stays literal), subject is one line. Unknown placeholders or no recipient: left as typed. Real mode reads the store; Demo keeps fixtures. Limits: 100 templates, name 80, subject 200, body 20 000. |
| MB-210 signature | `crates/mailbox-core/src/signature.rs`, `state/prefs.rs::use_signature_sync` | Block `\n\n-- \n<text>` inserted below the cursor block and above any quote; replaced in place when From changes (even in a draft reloaded from disk: all known signatures are recognised); never duplicated; the quoted text stays byte-identical; an identity without a signature, or `use_for_replies = false` on a reply, removes it. A draft holding only the automatic signature still counts as empty (`Draft::is_empty`). |
| MB-210 shortcuts | `state/shortcuts.rs` (registry + pure `resolve`), `components/shortcuts.rs` (page listener, dispatch, help overlay, settings block) | Bindings: `c` compose, `r` reply, `a` reply all, `f` forward, `e` archive, `#`/Delete delete, `s` star, `u` mark unread, `j`/`k` next/previous, Enter open, `/` and Ctrl/Cmd+K search, Ctrl/Cmd+Enter send, Esc close, `?` help. Plain keys never fire while the focus is in an input, textarea, select or contenteditable; Enter also not on a focused button/link. Ctrl/Cmd+K and Ctrl/Cmd+Enter work while typing. The on/off setting silences everything except Esc. Reuses existing actions (`folders::archive/trash/toggle_star/set_unread`, `Draft::reply_to`, the send button's own click so all blockers apply). |
| MB-203 | `components/oauth_mock.rs`, `screens/connect.rs`, `LIMITATIONS_1.0.md` | Two buttons "Đăng nhập bằng Google / Microsoft" each carrying the badge "mock / chưa hoạt động thật" open an explanation panel: OAuth not available, what is needed (owner's OAuth client), the real path (app password + autoconfig). The panel has no input, form, link, browser launch, token field or network call. Task state is MOCK. |
| MB-202 | new crate `crates/mailbox-autoconfig`; `adapters/connect.rs::detect_servers`, `components/autoconfig.rs`, `state/connect.rs::apply_detection`, `screens/connect.rs` | Order: built-in table → DNS SRV (`_imaps._tcp`, `_submissions._tcp`, `_submission._tcp`) → Thunderbird XML (`autoconfig.<domain>`, `<domain>/.well-known/...`, HTTPS) → guesses (`imap.`, `mail.`, `smtp.`). Every candidate not from the table must pass a probe (verified TLS + protocol greeting) before it is offered. The result shows each value with its source; nothing is applied until "Dùng đề xuất này"; manual entry always stays; Demo keeps its scripted scenarios. |
| MB-211 | `crates/mailbox-core/src/notify.rs` (policy + `Notifier` port), `adapters/notify.rs` (OS adapter, `notify_new_mail`, snapshot/diff), `components/notify_view.rs` | See "Notifications" below. |

### SSRF and privacy rules in MB-202 (all enforced in code and tested)

- **One guard, `detect::checked_addrs`**: validates the host name (ASCII labels, at least two labels, not an IP/`0x`/numeric-TLD shape), resolves **once**, refuses the whole host if **any** answer is loopback, private, link-local (incl. 169.254.169.254), CGNAT 100.64/10, multicast, unspecified, reserved, TEST-NET, benchmarking, ULA, site-local, Teredo, documentation, ORCHID, discard — IPv4 and IPv6, with IPv4-mapped, IPv4-compatible, NAT64 and 6to4 judged by the embedded IPv4. The caller connects to the returned `SocketAddr`s and never resolves the name again (no DNS rebinding; test counts resolver calls and checks a mixed public+private answer is refused).
- HTTPS only, certificate validated through `mailbox-net` against the host name, port 443 only, **every 3xx refused** (no redirects at all, stricter than "same host"), `Connection: close`, `Accept-Encoding: identity`, 64 KiB body cap (also against a lying `Content-Length`, chunked bodies and an endless stream), 16 KiB header cap, 12 s total timeout.
- XML read by a small in-house reader: a DTD, ENTITY, CDATA, processing instruction other than `<?xml ?>`, or any `<!` construct is refused; only five predefined entities are decoded and any other `&` voids the value; `%EMAILLOCALPART%` / `%EMAILADDRESS%` in a host name voids the candidate.
- Ports limited to 993/143/465/587 (mail) and 443 (HTTPS); each SRV record may only name the port its label stands for; candidates must also be a mode the app supports (IMAP implicit TLS 993; SMTP 465 implicit or 587 STARTTLS). At most 12 probes per detection, 3 SRV targets per service.
- Only the **domain** of the address leaves the machine (DNS name, Host header, SNI). The request text is asserted to contain no `@`, no query string, no cookie or authorization header; the result panel contains no `@`.
- Nothing is saved by detection.

### Notifications (MB-211) behaviour

Off / count only (default) / sender + subject (explicit opt-in). The `NewMail` type has no body field. Text is one line, control characters stripped, shortened. One notification per batch. Silent for Sent, Drafts, Trash, Junk; for mail in the folder shown in a **focused** window (`NotifyViewHost` publishes list folder + focus to the adapter); in Demo mode; and for read mail or mail older than two days (so a freshly synced folder does not announce its history). `notify_new_mail(account_id, &[NewMail])` reads the stored choice itself so any caller gets the same behaviour; the manual sync path (`adapters/account.rs::resync_all`) calls `adapters::notify::after_sync`, which snapshots message ids before the sync and diffs after.

## Tests (written alongside the code; RED/GREEN noted where it actually happened)

I wrote most tests in the same pass as their code, so there is no clean RED for every slice. Real RED moments: `<?xml-stylesheet ...?>` was wrongly accepted as the XML declaration (found by the entity test, fixed); two UI tests failed first (autoconfig button also showed in Demo on a Desktop build — gated on `REAL_CHECK` instead of the mode; HTML-escaped quotes in an assertion); clippy rejected a test with field reassignment and an item after a test module.

| Layer | Count | Covers |
|---|---|---|
| `mailbox-core` (prefs, signature, notify) | 26 new | placeholder escaping/no re-expansion, remote images forced off, newer/broken record refused, restore-defaults keeps user text, signature insert/replace/no-duplicate/above-quote/foreign swap, notification policy with a fake notifier |
| `mailbox-store` | 3 (+1 `#[ignore]` PostgreSQL) | SQLite contract (CRUD, owner scope, order, limits, reset leaves templates), corrupt/newer record is an error, no secret columns. PostgreSQL contract runs with `MAILBOX_TEST_PG_URL` (not run here) |
| `mailbox-autoconfig` | 50 unit | IP classes (public/non-public v4 and v6 incl. wrappers), port and host-name policy, DNS wire format (compression loop, truncation, wrong id), SRV ordering, XML (DTD/entity/bomb/CDATA/PI refused, size, local-part), HTTP (redirects, oversize declared/actual/chunked/endless, timeout, compressed), provider table, guard (single resolve, mixed answer, bad names), orchestration with fake resolver/HTTP/probe (order of sources, priority/weight, wrong port, private target, hostile XML, fetch failure fallthrough, budget, domain-only requests), probe never connects to a loopback resolution (real listener) |
| `mailbox-ui` | 11 unit (`state/prefs`, `state/shortcuts`, `state/connect`, `adapters/prefs`, `adapters/notify`) + 7 new dioxus-ssr tests (`f_*` in `ui_tests`) | shortcut registry (typing, disabled, modifiers, Enter on button, unique keys), signature/template flows on `Draft`, settings survive reopening the DB file, new-mail diff + policy; OAuth mock label/no inputs, autoconfig result with sources, persistence controls only in Real mode, template rail from the store, shortcut help |
| Opt-in network (`tests/real_autoconfig.rs`, `MAILBOX_TEST_AUTOCONFIG_DOMAIN`) | 4 domains run | see below |

Workspace totals after the last change: `cargo test --workspace` **380 passed, 0 failed, 19 ignored** (opt-in network/PostgreSQL/real-provider tests).

## Commands and real results

```
cargo fmt --all -- --check                                                         FMT_OK
cargo test --workspace                                                             380 passed, 0 failed, 19 ignored
cargo clippy --workspace --all-targets -- -D warnings                              clean
cargo clippy -p mailbox-ui --no-default-features --features desktop -- -D warnings clean
cargo clippy -p mailbox-ui --no-default-features --features web --target wasm32-unknown-unknown -- -D warnings   clean
```

Opt-in read-only network test (`MAILBOX_TEST_AUTOCONFIG_DOMAIN=<d> cargo test -p mailbox-autoconfig --test real_autoconfig -- --ignored --nocapture`; no credentials, TLS handshake and greeting only):

| Domain | Result |
|---|---|
| `fastmail.com` | built-in: `imap.fastmail.com:993`, `smtp.fastmail.com:465`; both pass the TLS probe |
| `posteo.de` | **DNS SRV** (real UDP query): `posteo.de:993`, `posteo.de:465`; both pass the probe |
| `mailbox.org` | **autoconfig XML over HTTPS** (`autoconfig.mailbox.org`): `imap.mailbox.org:993`, `smtp.mailbox.org:465`; both pass |
| `a-domain-without-mail.example` | nothing found, test fails by design ("nothing detected"): no SRV, no autoconfig, no answering `imap.`/`mail.`/`smtp.` host. The wizard shows "Không tự phát hiện được cấu hình... nhập thủ công" |

## Mocks and limits

- MB-203 OAuth: **mock** (row updated in `LIMITATIONS_1.0.md`).
- MB-211 macOS: code complete and unit-tested; whether macOS shows the banner for the unsigned bundle was **not verified** (no GUI session). New row in `LIMITATIONS_1.0.md`.
- MB-202: new rows for the missing Vietnamese provider entries (hosts could not be verified from this build, so none were invented), STARTTLS probing (greeting only), SRV on Windows (needs `/etc/resolv.conf`), Microsoft 365 custom domains (no MX step).

## Not verified

- No click-through of the Desktop app (no `dx serve`, per brief): the page-level shortcut listener (`document::eval`), `NotifyViewHost` focus tracking, the signature effect on a live composer and the OS banner are covered by compile + pure-logic tests, not by running the app.
- PostgreSQL contract for preferences/templates (needs `MAILBOX_TEST_PG_URL`).
- Windows and Linux notification and SRV paths.
- Real SMTP STARTTLS candidates from SRV/XML were not exercised against a live server (posteo/mailbox.org returned 465).

## Risks

- The wizard's own connection check (existing code, `mailbox-net::connect_tcp`) resolves the host name again without the SSRF guard. A suggested host that passed the probe could change its DNS answer before the user confirms. TLS certificate validation still applies, but the guard is not on that path. Worth moving the guard into `mailbox-net` for user-typed hosts too (separate change; the brief limits shared-crate edits).
- `j`/`k` use `rows_in_view(mail.visible(&filter))`; track E changes list/search/paging, so the selection order may need adjusting after merge.
- `Draft::is_empty` now ignores an automatic signature: it relies on the body starting with the `-- ` marker. A user who types a body beginning with `-- ` on its own line and nothing else would see the draft treated as empty.
- The signature effect writes the draft body programmatically; autosave runs on the next edit/send, so a fresh draft that only has the signature is not saved (intended: it counts as empty).
- `notify-rust =4.19.0` pulls `mac-notification-sys` on macOS (objc) and `zbus` on Linux: check the Linux build cost before enabling Linux packaging.
- `after_sync` diffs the newest 500 cached messages per folder; on a huge burst the surplus is not announced (the notice is batched anyway).
