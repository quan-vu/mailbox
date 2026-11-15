# Security audit for Desktop 1.0 (MB-304)

Date: 2026-11-15. Branch `claude/mb-sec-audit-5b9b7e`, based on `main` at `c8d6ef1`.
Scope: this repository only (Desktop build, plus the `mailbox-api` demo and the Web build where
they share code). Defensive code review, tests and the dependency advisory scan. No outside
system was scanned; tests contact loopback only.

Method: manual review of every crate and of the Desktop UI adapters, in four areas (credentials
and logging; TLS and protocol parsing; MIME, rendering and compose; storage, API and privacy),
followed by a failing test and a fix for each item that was clearly wrong and small.
Framework behaviour (dioxus-desktop 0.7.10, wry 0.53.5, ammonia 4.2.5, keyring 3.6.3) was read in
the vendored sources, not run.

Severity: **Critical** (credential or mail exposure to a remote party, code execution),
**High** (remote attacker gets data or reliable control), **Medium** (remote denial of service of
a core function, local exposure of mail data, or a missing defence layer a likely attack needs),
**Low** (hardening, needs local access or an unusual setup), **Info** (noted, no action needed
for 1.0).

Status: **fixed** (with test, on this branch), **accepted** (reason given; listed in
`docs/06-delivery/LIMITATIONS_1.0.md` when it limits 1.0), **open** (needs work after this
branch; patch outline given).

## Summary

| Severity | Fixed | Accepted | Open |
|---|---|---|---|
| Critical | 0 | 0 | 0 |
| High | 0 | 0 | 0 |
| Medium | 8 | 0 | 2 |
| Low | 1 | 6 | 12 |
| Info | 0 | 7 | 0 |

No Critical or High finding. Every Medium that is small and local is fixed. S-08 and S-09 were
fixed on `main` after the audit branch was merged (2026-11-15). The two open Mediums are S-10
(Windows only, needs a Windows build) and S-11 (waits for an sqlx release).

## Findings

| ID | Sev | Where (file:line) | Finding | Status |
|---|---|---|---|---|
| S-01 | Medium | `apps/mailbox-ui/src/adapters/connect.rs:125`, `crates/mailbox-net/src/lib.rs:103`, `crates/mailbox-imap/src/lib.rs:679`, `crates/mailbox-smtp/src/lib.rs:500` | Connect wizard: the final IMAP/SMTP check of a host proposed by autoconfig resolved DNS again outside the SSRF guard, so an answer changed after detection (DNS rebinding) could point the check at an internal address. | **fixed** `a00b02b`: hosts taken from the detection are resolved once through `checked_addrs`; the check connects only to those pinned addresses; the certificate is still validated for the host name. A typed host keeps the normal path (LAN servers allowed). Tests: rebinding resolver resolved exactly once, private/mixed answers refused, pinned connect uses the address without DNS. |
| S-02 | Medium | `crates/mailbox-core/src/signature.rs:83`, `apps/mailbox-ui/src/state/compose.rs:280` | `Draft::is_empty` counted any body starting with a `-- ` line as empty, so text typed below the signature, or a note starting with `-- `, was neither autosaved nor saved on close (data loss). | **fixed** `7d59be2`: empty only when the body is exactly one of the user's stored signatures; with none known only whitespace is empty. |
| S-03 | Medium | `apps/mailbox-ui/src/adapters/body.rs:211` | MIME parsing, HTML sanitising and attachment decoding (up to 25 MB) ran on the async task that drives the Desktop UI; a large or hostile message froze the window. | **fixed** `b6613c5`: `spawn_blocking`; output identical to a direct call (test), limits unchanged. |
| S-04 | Medium | `crates/mailbox-store/src/lib.rs:48` | The SQLite database (headers, cached bodies, drafts, search index) was created with the process umask, usually 0644: readable by other local users on Linux or with `MAILBOX_DATA_DIR`. | **fixed** `85e1165`: file pre-created 0600; existing file and `-journal`/`-wal`/`-shm` tightened on open. Directory mode: see S-27. |
| S-05 | Medium | `crates/mailbox-imap/src/parse.rs:438` | `parse_internal_date` sliced a server-supplied zone on bytes (`+1é1` panics) and multiplied unbounded numbers (overflow). A hostile server could stop every sync. | **fixed** `62e72e5`: fields range-checked, zone must be 4 ASCII digits. |
| S-06 | Medium | `crates/mailbox-mime/src/sanitize.rs:138` | `is_safe_data_image` sliced the untrusted `src` at byte 32 on a `str`; a non-ASCII character there panicked, so the message could never be shown (the body loader turned it into a generic failure on every retry). | **fixed** `665dc76`: byte comparisons for the data-URI and `cid:` checks. Test over every offset 0..40. |
| S-07 | Low | `crates/mailbox-autoconfig/src/http.rs:28` | Chunked decoder: a chunk size near `usize::MAX` wrapped the length checks and panicked. Reachable from an HTTPS autoconfig host with a valid certificate. | **fixed** `14cfff9`: size compared with the 64 KiB cap before any arithmetic. |
| S-08 | Medium | `crates/mailbox-imap/src/lib.rs` (`COMMAND_BUDGET`, `Dialogue::charge`), `mutate.rs`, `body.rs` | No total byte budget per IMAP command: per-literal (64 KiB), per-response and per-command line counts were capped, but the product allowed ~2.9 GB for one FETCH/EXAMINE and more in `fetch_flags` (100 000 lines). A hostile server could exhaust memory. | **fixed** (after the audit branch, on `main`). Every command gets 32 MiB for all lines and literals it reads; the budget is reset when the next command is tagged and is charged before a literal is allocated. Over budget is `NotImap` (read path) or `Protocol` (flag scan and actions), and the session is dropped. The requested message body is not counted: it keeps its own caller-set limit (25 MB). Tests `crates/mailbox-imap/tests/budget.rs`: a 48 MiB LIST flood and a 48 MiB flag-scan flood are cut off; 20 MiB per command passes and a second command gets a fresh budget. `fetch_flags` still buffers its lines, now bounded by the budget. |
| S-09 | Medium | `crates/mailbox-core/src/link.rs`, `apps/mailbox-ui/src/adapters/{body,link}.rs`, `components/link_confirm.rs`, `main.rs` | Links in the mail frame: on macOS/Linux the Dioxus default opened http(s)/mailto links in the system browser/mail client without confirmation or showing the real URL (phishing aid); on Windows (wry hooks only main-frame navigation) a link would load the remote page inside the sandboxed frame. | **fixed** (after the audit branch, on `main`). `frame_document` rewrites every `href` to the inert `mailbox-link:<hex>` scheme at display time (also for bodies cached earlier), so no platform can open or load a link by itself. The frame's sandbox has one permission, `allow-top-navigation-by-user-activation`, and its document carries MailBox's own `<base target="_top">`: a click by the user asks the top frame to go to the inert address (the sandboxed frame navigating itself is not reported by WKWebView at all; that first design did nothing on a click and was replaced on 2026-11-15). On Desktop the WebView navigation handler (`adapters::link::gate`) blocks that navigation and queues it; the UI decodes and inspects the address and shows a dialog with the real host and the full parsed, ASCII-only address (what is shown is what is opened), a warning for plain http and for `xn--` hosts. Addresses with a user name before the host, other schemes, over 2048 bytes or malformed are refused with no open button. The handler also refuses every other navigation except `about:`. The permission is safe only while a message has no other way to navigate: the sanitizer removes `<base>`, `<meta>`, forms, `formaction`, SVG, `<area>` and `target`, every remaining `href` is guarded, and no script runs. A follow-up fix in the same change: `guard_links` panicked on text with multi-byte characters before a tag (any Vietnamese HTML mail), found by the demo message. Tests: 9 in `mailbox-core` (`link::tests`), 1 in `adapters::link`, 6 SSR tests in `main.rs`, 1 in `mailbox-mime` (`nothing_but_a_link_can_start_a_navigation`). Checked by hand on macOS in a dev build and in the installed app with the demo message's five sample links (https, http, `xn--`, deceptive, mailto). Limits: not checked on Windows (a top-frame navigation is reported there, so it should work); in the Web build a click does nothing (LIMITATIONS row); a link is only as trustworthy as the user's reading of the host. |
| S-10 | Medium | `apps/mailbox-ui/src/adapters/body.rs:385` | Windows: saved attachments get no Mark-of-the-Web (`Zone.Identifier`); macOS sets the quarantine flag. | **open** (Windows untested in 1.0). Outline: write `:Zone.Identifier` with `ZoneId=3` after the file is written. |
| S-11 | Medium | `crates/mailbox-store` (sqlx 0.8.6 → `libsqlite3-sys` 0.30.1) | `cargo audit`: RUSTSEC-2026-0459 (CVE-2026-119375), SQLite < 3.54 can return uninitialised memory when a database is opened with a crafted `-wal` file. Local only; the app opens only its own owner-only file (S-04). | **open** (Low practical risk after S-04). Needs an sqlx release that accepts `libsqlite3-sys` ≥ 0.39; not a small change. |
| S-12 | Low | `crates/mailbox-imap/src/lib.rs:133`, `crates/mailbox-imap/src/idle.rs:98-144` | `completion()` and the IDLE reader read line by line; a `{n}` literal in an unsolicited response would be read as protocol lines and could desync the session. | **open**. Outline: use the literal-aware `logical()` reader, or fail on a line ending in `{n}`. |
| S-13 | Low | `crates/mailbox-net/src/lib.rs:15`, SMTP/IMAP readers | No overall deadline per command: each read has a 30 s step timeout, so a slow-drip server can hold one reply for many minutes (and the live worker's session lock). | **open**. Outline: wrap each command in an overall deadline (2-5 min, scaled for large FETCH/DATA). |
| S-14 | Low | `crates/mailbox-autoconfig/src/dns.rs:97`, `system.rs:21` | SRV reply not tied to the question (qname/qtype/owner name not compared); query ID from a SipHash of the time, not a CSPRNG. Source port is OS-random and `connect()` filters the peer; every candidate still needs a verified-TLS probe and a user click. | **open**. Outline: compare the echoed question and answer owner/class; ID from `getrandom`. |
| S-15 | Low | `apps/mailbox-ui/src/adapters/body.rs:120` | After "show images" the frame CSP also allows `http:` images (readable/alterable in transit). | **open**: behaviour change for newsletters; decide whether to drop `http:` or upgrade to `https:`. |
| S-16 | Low | `crates/mailbox-compose/src/message.rs:348`, `apps/mailbox-ui/src/adapters/send.rs:117` (track G) | One malformed `Message-ID` in a received `References`/`In-Reply-To` makes `build` fail, so that mail cannot be replied to (no injection). | **open**. Outline: drop invalid ids in `threading()` instead of failing the build. |
| S-17 | Low | `apps/mailbox-ui/Cargo.toml:8` (dioxus default `logger` feature) | Dioxus starts a stderr logger (INFO in release); dioxus-desktop logs the raw event body at ERROR when an event fails to parse, which for the password field would include the typed value. Debug builds also enable WebView devtools. The app itself logs nothing. | **open**. Outline: disable dioxus default features or filter `dioxus_desktop` to WARN without event bodies; check the bundle config disables devtools. |
| S-18 | Low | `apps/mailbox-ui/src/screens/connect.rs:25`, `:142`; `screens/compose.rs:572` | Passwords live as plain `String` in Dioxus signals (controlled input, `held_secret` until Save/Back) and pass through `String` parameters in `adapters/send.rs`/`state/outbox.rs` (track G); dropped copies are not wiped. | **open**. Outline: uncontrolled password input, hold `Option<Secret>`, pass `Secret` across the UI boundary. |
| S-19 | Low | `crates/mailbox-core/src/secrets.rs:26` | `Secret` wipes only `len` bytes with `write_volatile`, no fence, spare capacity not wiped. | accepted: adequate for 1.0; `zeroize` would be the follow-up. |
| S-20 | Low | `crates/mailbox-secrets/Cargo.toml:10` | keyring `sync-secret-service` without `crypto-rust`: on Linux the secret crosses D-Bus unencrypted (session bus, same user). | **open** (Linux untested in 1.0). Add `crypto-rust`. |
| S-21 | Low | `apps/mailbox-api/src/main.rs:23` | `MAILBOX_BIND` accepts any address for the unauthenticated demo API; no Host-header check (DNS rebinding could read demo data). Default is `127.0.0.1:3000`; CORS allows loopback origins only, GET only. | **open**. Outline: refuse non-loopback binds unless an explicit flag; check `Host`. Demo data only. |
| S-22 | Low | `apps/mailbox-ui/src/adapters/account.rs:330` | Removing an account leaves its signature and `default_identity` (address-derived id) in the preferences JSON. | **open**. Outline: drop them inside `remove_account`. |
| S-23 | Low | `crates/mailbox-store/src/lib.rs` | No `secure_delete`: deleted bodies/drafts/search rows stay in free pages and journals. Draft rows stay after a successful send until deleted (track G). | accepted for 1.0 (owner-only file, S-04); encryption at rest is the PRIVACY.md open item. |
| S-24 | Low | `crates/mailbox-mime/src/filename.rs:6-48` | `safe_filename`: Windows device names incomplete (COM0, LPT0, superscripts, CONIN$, trailing-space stem), 120 characters can exceed 255 bytes, some invisible characters (U+061C, U+00AD, U+180E, ...) kept. Path components are always removed. | accepted for macOS 1.0 (create_new + 0600 + quarantine); Windows follow-up with S-10. |
| S-25 | Low | `crates/mailbox-mime/src/parse.rs:133` | Bidi control characters (U+202A-202E, U+2066-2069) are kept in subjects and sender names, so they can be visually spoofed. | **open**. Outline: strip them in `clean_line`. |
| S-26 | Low | `crates/mailbox-mime/src/parse.rs:166-214`, `:322` | Depth/part limits apply after mail-parser built the tree; no header count/length cap; HTML-to-text runs on the full HTML part. All bounded by the 25 MB raw limit and now off the UI thread (S-03). | accepted: bounded CPU/memory, background thread. |
| S-27 | Low | `apps/mailbox-ui/src/adapters/account.rs:82`, `body.rs:223`, `send.rs:65` | The data directory is created with the umask (0755 on Linux); `.` fallback when HOME is unset; same `.` fallback for downloads. | **open**. Outline: create the directory 0700 in one helper used by the three openers (send.rs is track G). macOS `~/Library` is 0700 already. |
| S-28 | Low | `apps/mailbox-ui/src/adapters/body.rs:371`, `:412` | macOS quarantine set via `/usr/bin/xattr` after write, failure ignored; a failed write leaves a partial file. | accepted for 1.0; follow-up: `setxattr` before returning, remove on error. |
| S-29 | Low | `apps/mailbox-ui/src/adapters/send.rs:182` (track G) | Attachments are read at send time from stored absolute paths; symlinks followed; small gap between `metadata()` and `read()`. Paths come from the file picker. | accepted: local only. |
| S-30 | Info | `crates/mailbox-autoconfig/src/detect.rs:181` | SRV targets may be any public host (RFC 6186 asks for a domain check or confirmation). The source is shown and nothing applies without a click; probe needs a valid certificate. | accepted. |
| S-31 | Info | apps/mailbox-ui adapters (worker, send, body, actions, account) | Connections after setup resolve the stored host through the system resolver without the public-IP check. Rebinding to an internal host fails TLS name validation; at most an EHLO/STARTTLS reaches an internal port 587. | accepted: a desktop client must reach LAN servers the user configured; certificate validation is the control. |
| S-32 | Info | `crates/mailbox-smtp/src/lib.rs:213` | `command()` builds a formatted copy of the AUTH token line that is not wiped (the original is). | accepted; fold into S-18 follow-up. |
| S-33 | Info | `crates/mailbox-net/src/lib.rs:42` | Roots are the bundled Mozilla set; OS trust settings, enterprise CAs and revocation are ignored; no ALPN. | accepted for 1.0 (fail closed); `rustls-platform-verifier` is a later option. |
| S-34 | Info | `apps/mailbox-ui/src/adapters/body.rs:321` | Cached sanitized HTML is shown without sanitizing again; a later sanitizer fix does not reach old cache rows. | accepted; follow-up: store a sanitizer version and re-parse on change. |
| S-35 | Info | `crates/mailbox-core/src/store.rs:6` and others | `#[derive(Debug)]` on records holding addresses/subjects/bodies (`AccountRecord`, `Draft`, `Message`, `Envelope`); no password; no `{:?}` of them in non-test code. | accepted; a redacting `Debug` would protect future logging. |
| S-36 | Info | dependency scan | `cargo audit`: RUSTSEC-2023-0071 `rsa` 0.9.10 is in `Cargo.lock` only (sqlx-mysql, not compiled: `cargo tree -i rsa` is empty). Warnings: unmaintained `fxhash`, `paste`, `proc-macro-error`; unsound `glib` 0.18.5 and `rand` 0.7.3 (Linux GTK stack, not in the macOS build graph). | accepted; revisit with the sqlx upgrade (S-11). |

## Controls confirmed (no finding)

- **Credentials**: password only in the OS keychain (`app.mailbox.client`); `Secret` has no
  `Clone`/`Display`/`Serialize`, `Debug` is redacted; keychain errors map to static codes; no secret
  column in any SQLite or PostgreSQL migration (`credential_ref` is an opaque pointer); preferences
  hold no secret; every error enum is `Copy`/static, none carries server text, addresses or SQL; no
  `println!`/`eprintln!`/`dbg!`/`log`/`tracing` call in app code (the API prints its bind address
  only); no panic hook, the only `expect`s have static messages.
- **TLS**: rustls 0.23.46 with the webpki verifier (chain, validity, host name); no custom
  verifier, no `dangerous()`, no flag or environment variable touches TLS; SNI and the name check
  use the configured host, also on pinned addresses. IMAP is implicit TLS only. SMTP STARTTLS: no
  plaintext fallback, non-220 refused, bytes buffered after the 220 refused (CVE-2011-0411 class),
  capabilities re-read after TLS, AUTH only after verified TLS. EHLO name is `[127.0.0.1]`.
- **Protocol input**: IMAP LOGIN sends user and password as synchronizing literals (CR/LF/NUL
  refused); mailbox names quoted and limited to printable ASCII; the only SEARCH is a validated
  `HEADER Message-ID`; APPEND waits for `+`; tags cannot prefix-match. Line limits 8 KiB (IMAP),
  4 KiB / 64 lines (SMTP); body FETCH checks `RFC822.SIZE` and the literal length before
  allocating; IDLE bounded (≤ 25 min), cancel-safe, always sends DONE. SMTP envelope addresses
  refuse controls, whitespace and `<>`; DATA requires CRLF and dot-stuffs; ≤ 100 recipients.
- **Autoconfig**: one SSRF guard (`checked_addrs`) on every path, now also the wizard check
  (S-01); ports 993/143/465/587 and 443 only; no redirects; 16 KiB header / 64 KiB body caps; 12 s
  total; XML without DTD/entities; only the domain leaves the machine.
- **MIME and rendering**: ammonia allow-list without form, input, meta, base, link, style, svg,
  math, iframe, object, embed; `href` only http/https/mailto on `<a>` with `rel=noopener
  noreferrer`; `img src` only `cid:` (inlined), raster base64 `data:` ≤ 1 MB, and http(s) in the
  explicit remote variant; `srcset`/`background`/`poster`/`ping` and CSS `url()` always removed.
  Limits: 25 MB raw (checked before fetch), depth 10, 200 parts, 2 MB HTML, 1 MB/2 MB inline
  images. The frame is `iframe sandbox="allow-top-navigation-by-user-activation"` (see S-09; no scripts, forms, same-origin or pop-ups) with `srcdoc`, CSP `default-src 'none'`, no-referrer;
  no script can run in it, so the Dioxus IPC bridge is unreachable from mail content. The only
  `dangerous_inner_html` renders fixed SVG icons; every `eval` takes fixed script text.
- **Attachments**: name reduced to one safe component twice, `create_new` (no overwrite, no
  following an existing symlink), mode 0600, never opened, quarantined on macOS.
- **Compose**: control characters in subject, names, filenames and content type are refused;
  addresses exclude controls, whitespace, `<>,":`; threading ids validated; RFC 2047/2231
  encoding; Bcc only in the envelope; boundary collision impossible; 25 MB attachments / 40 MB
  message.
- **Storage**: all dynamic SQL uses bound parameters (only compile-time constants are
  formatted); FTS5 terms are quoted with `"` doubled, so `NEAR`, `col:`, `-`, `^`, `(` cannot act
  as operators; LIKE escapes `%`, `_`, `\` with `ESCAPE`.
- **mailbox-api**: default bind `127.0.0.1:3000`; CORS loopback `http://` origins only, GET only;
  DTOs have no secret or `credential_ref`; serves demo fixtures only.
- **Privacy**: no telemetry, crash reporting, update check or third-party host; notifications
  default to a count only. Details in `PRIVACY.md`.

## Not checked

- Runtime behaviour in the installed app (file modes on a real profile, heap residency of
  passwords, the keychain ACL of an unsigned build). The link click of S-09 was checked by hand on macOS
  on 2026-11-15 with the demo message; a link in a real synced message was not clicked.
- Windows and Linux builds (S-10, S-20, S-24 are paper findings).
- Fuzzing: none run in this pass (MIME, IMAP and DNS parsers are candidates; no `cargo fuzz`
  harness exists). Load testing: not run.
- `cargo deny check`: tool not installed; installing takes ~13 minutes of build on this machine
  and was skipped. Licences were not re-reviewed.
- The PostgreSQL (Web) store at runtime; the Web build stays demo-only in 1.0.
