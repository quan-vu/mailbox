# Track I evidence — Desktop 1.0 security audit and fixes (MB-304)

Branch `claude/mb-sec-audit-5b9b7e`, based on `main` `c8d6ef1`, 2026-11-15, macOS aarch64,
rustc 1.97.0. Findings table: `docs/04-security/AUDIT_1.0.md` (36 findings: 0 Critical, 0 High,
10 Medium, 19 Low, 7 Info). Privacy update: `docs/04-security/PRIVACY.md`.

Numbering note: the brief called the audit "MB-303" and the fixes "MB-304". In `TRACKING.md`
MB-303 is "E2E CI 3 providers" and MB-304 is "Security/PII/fuzz/load review", so all commits and
docs use MB-304.

## What was fixed (each with a failing test first)

| Commit | Finding | Change | RED (before the fix) | GREEN |
|---|---|---|---|---|
| `a00b02b` | S-01 wizard check re-resolved autoconfig hosts outside the SSRF guard | `mailbox-net::connect_tcp_pinned`/`connect_tls_pinned`, `mailbox-imap::check_login_pinned`, `mailbox-smtp::check_pinned`, UI `guarded_addrs` + `host_from_detection`; new failure `BlockedAddress` ("nhập tay" copy, no bypass) | `cannot find function connect_tcp_pinned / check_login_pinned / check_pinned / guarded_addrs`, `no variant BlockedAddress` (compile errors) | `mailbox-net` 3, `mailbox-imap --test pinned` 2, `mailbox-smtp --test pinned` 2, UI `guard` 4 pass |
| `7d59be2` | S-02 `Draft::is_empty` `-- ` heuristic lost text typed below the signature | `signature::is_only_signature_of(body, known)` exact match; `Draft::is_empty_with`; `is_empty()` reads the stored signatures when the app runs, none otherwise | `cannot find function is_only_signature_of`, `no method is_empty_with`; then `prefs::tests::a_draft_with_only_the_automatic_signature_still_counts_as_empty` failed (it relied on the heuristic outside a runtime) and now passes the signature list explicitly — same assertions | core `signature` 9, UI 134 pass |
| `b6613c5` | S-03 parse + sanitize on the UI task | `off_ui` (`spawn_blocking`) around `mailbox_mime::parse` and `attachment_bytes` | `unresolved import off_ui` | `parsing_runs_off_the_calling_thread_with_identical_output` + existing body tests pass (UI 135) |
| `85e1165` | S-04 database file 0644 | `restrict_to_owner` in `SqliteStore::open`: create 0600, tighten file + journal/WAL/SHM | `left: 420, right: 384` (0644 vs 0600) in both `file_mode` tests | `mailbox-store --test file_mode` 2 pass |
| `62e72e5` | S-05 INTERNALDATE panic/overflow | range checks, ASCII zone | panicked at `parse.rs:463` (byte slice) | `mailbox-imap` all pass |
| `665dc76` | S-06 sanitizer panic on multi-byte `img src` | byte comparisons in `is_safe_data_image` and the `cid:` check | panicked at `sanitize.rs:143` | `mailbox-mime` all pass |
| `14cfff9` | S-07 chunked size overflow | size checked against the cap first | panicked at `http.rs:39` | `mailbox-autoconfig` 51 pass |

No existing check was weakened. The one existing test changed (`prefs.rs`) keeps its assertions
and now states which signatures are known.

Shared files touched (small, additive): `crates/mailbox-net/Cargo.toml` (tokio dev-dependency for
tests), `apps/mailbox-ui/src/screens/connect.rs` (passes `guarded` to the two checks),
`crates/mailbox-store/src/lib.rs` (`open` calls `restrict_to_owner`). No track G file
(`adapters/send.rs`, `adapters/worker.rs`, `state/outbox.rs`) was edited; `Draft::is_empty()` keeps
its signature so `state/outbox.rs` needs no change. No UI string changed except the one new
`CheckFailure::BlockedAddress` message (Vietnamese, same style as its neighbours; track H may
translate it).

## Supply chain

- `cargo audit` (cargo-audit 0.23.0, installed with `cargo install --locked`, 13 min build;
  advisory DB 1509 advisories; 736 crates in `Cargo.lock`): exit 1, **2 vulnerabilities**:
  RUSTSEC-2026-0459 `libsqlite3-sys` 0.30.1 (via sqlx 0.8.6; local, crafted `-wal`; AUDIT S-11,
  open) and RUSTSEC-2023-0071 `rsa` 0.9.10 (lockfile only through sqlx-mysql; `cargo tree -i rsa
  --target all` prints nothing). 5 warnings: unmaintained `fxhash`, `paste`, `proc-macro-error`;
  unsound `glib` 0.18.5, `rand` 0.7.3 (not in the macOS build graph).
- `cargo deny check`: **not run** (not installed; another ~13 min build was skipped).
- Pinned security-relevant versions (Cargo.lock): rustls 0.23.46, tokio-rustls 0.26.6,
  webpki-roots 1.0.12, ring 0.17.14, ammonia 4.2.5, html5ever 0.29.1/0.40.1, sqlx 0.8.6,
  libsqlite3-sys 0.30.1, keyring 3.6.3, notify-rust =4.19.0, dioxus 0.7.10, wry 0.53.5,
  axum 0.8.9, tower-http 0.6.11, tokio 1.54.1.

## Gates (this branch, after the last fix)

Run on commit `14cfff9` (code unchanged after it; later commits are docs only):

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo test --workspace` | exit 0: **464 passed, 0 failed, 23 ignored** (main before this branch: 444 / 0 / 23; +20 new tests) |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo clippy -p mailbox-ui --no-default-features --features desktop -- -D warnings` | exit 0 (only the existing future-incompat note for `block` 0.1.6) |
| `cargo clippy -p mailbox-ui --no-default-features --features web --target wasm32-unknown-unknown -- -D warnings` | exit 0 |

Not run: real-provider (`#[ignore]`) tests, Web E2E, `dx bundle` (not allowed for workers).

## Open items for the coordinator (detail and patch outlines in AUDIT_1.0.md)

- Medium, open: S-08 IMAP total byte budget; S-09 link clicks in the mail frame (navigation
  handler + confirm dialog, needs UI copy); S-10 Windows Mark-of-the-Web; S-11 SQLite advisory
  (sqlx upgrade).
- Low, in track G files: S-16 (`adapters/send.rs` threading ids), S-18 (passwords as `String` in
  `adapters/send.rs` / `state/outbox.rs`), S-29.
- Low, small and local, not done to keep this branch narrow: S-12, S-13, S-14, S-15, S-17, S-20,
  S-21, S-22, S-25, S-27.

## Not verified

- Nothing was clicked in the installed app; the wizard's guarded path was tested with a scripted
  resolver and loopback listeners, not against a real autoconfig domain (no outside host
  contacted).
- `Runtime::try_current()` in `Draft::is_empty` was compiled for desktop and web; the behaviour
  with real stored signatures was covered by `is_empty_with` tests, not by a running app.
- Windows/Linux; fuzzing; load; `cargo deny`.

## Risks

- S-01 changes the wizard: an autoconfig host that resolves to a private address is now refused
  with a message telling the user to type it by hand. A company whose public autoconfig points to
  an internal-only IP must enter the host manually.
- S-02: in a context without a Dioxus runtime a signature-only draft counts as non-empty (kept),
  never the reverse.
