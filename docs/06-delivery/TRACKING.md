# Implementation tracker — stable task IDs

**Status as of 2026-11-14:** `AUTHORED` means files exist; `NOT VERIFIED` means no Cargo/Dioxus compilation was run. Tasks stay `NOT STARTED` until agent has produced real evidence. Never equate a screenshot with working email integration.

| ID | Phase | Task | Depends on | Status | Evidence / exit criteria |
|---|---|---|---|---|---|
| MB-001 | P0 | Init Cargo workspace, contracts, core, demo API, UI | — | DONE | Compiles on rustc 1.97.0 (MB-101 log) |
| MB-002 | P0 | Docs + five UI reference PNGs | — | AUTHORED | Docs index and PNGs present |
| MB-101 | P0 | Compile/run Web + Desktop; pin Cargo.lock | MB-001 | DONE | All gates PASS; Web E2E smoke; Desktop app launched and all 5 screens checked on macOS (see MB-101 log below) |
| MB-102 | P0 | Refactor components/styles, responsive real one-pane mobile, icon library | MB-101 | DONE | Screenshot diff + a11y test for 5 screens |
| MB-103 | P0 | UI data adapter: fixture/web HTTPS/native core | MB-101 | DONE | UI web reads API fixture; desktop reads native fixture; tests |
| MB-104 | P0 | Dioxus router stable deep links and navigation history | MB-102 | DONE | `/inbox`, `/compose`, `/settings` reload tests |
| MB-105 | P0 | Add localization infrastructure VI/EN | MB-102 | DONE (Desktop; VI default, EN from Settings) | Runtime locale switch and snapshot tests |
| MB-110 | P1 | Web authentication, session, CSRF, authorization guards | MB-103 | NOT STARTED | Unauthorized requests 401, owner tests |
| MB-111 | P1 | IMAP connect, TLS, auth error states | MB-113 | IN PROGRESS (Desktop keychain done; Web vault pending MB-110) | Real test provider connects with verified certificate |
| MB-112 | P1 | SMTP capability/identity test | MB-113 | DONE | Auth/TLS tests, no send without intent |
| MB-113 | P1 | Desktop OS keychain + Web AEAD vault | MB-110 | NOT STARTED | No plaintext secret in data/log; rotation test |
| MB-120 | P1 | SQLite/Postgres repositories and migration harness | MB-103 | DONE | Both dialects same contract tests |
| MB-121 | P1 | Folder discovery, UIDVALIDITY cursor persistence | MB-111, MB-120 | DONE (Desktop) | Restart and epoch-change tests |
| MB-122 | P1 | Provider capability mapping and SPECIAL-USE | MB-121 | DONE (Desktop) | Dovecot + alternate provider tests |
| MB-130 | P2 | Pagination, body fetch, MIME parser | MB-121 | DONE (Desktop; pagination of long folders open) | Unicode/multipart/huge MIME corpus tests |
| MB-131 | P2 | Secure HTML preview, attachment handling | MB-130 | DONE (Desktop; click-through pending) | XSS isolation, remote image, path traversal tests |
| MB-132 | P2 | Read/unread/star and folder mutations | MB-121 | DONE (Desktop; click-through pending) | Cross-client state reconciliation tests |
| MB-133 | P2 | Local draft schema/repository | MB-120 | DONE (local drafts; server Drafts sync open) | Survives restart, proper owner scoping |
| MB-140 | P2 | Sync worker IDLE/poll, retry, conflict handling | MB-121, MB-130 | DONE (Desktop; click-through pending) | Fault injection with disconnects, UID change |
| MB-141 | P2 | Status UI: partial error/offline/reconnect | MB-140 | DONE (Desktop; click-through pending) | Per-account status under outage |
| MB-143 | P2 | Sent/Drafts special folder mapping | MB-122 | DONE (Desktop) | Provider-specific mapping tests |
| MB-144 | P2 | Archive/trash/move/expunge | MB-132, MB-122 | DONE (Desktop; click-through pending) | Remote folder operations test |
| MB-150 | P2 | Composer, drafts, identity and MIME build | MB-133, MB-112, MB-131 | DONE (Desktop, plain text; click-through pending) | Sender account correct, BCC private, offline draft |
| MB-151 | P2 | SMTP submission state + Sent reconciliation | MB-150, MB-143 | DONE (Desktop; click-through pending) | Post-DATA disconnect unknown; no blind retry |
| MB-200 | P3 | Scalable unified inbox + scoped search | MB-140 | DONE (Desktop, local search; server search not built) | Pagination across providers, performance tests |
| MB-201 | P3 | Threading from Message-ID/References | MB-130 | DONE (Desktop) | No unrelated thread merging |
| MB-202 | P3 | Provider autoconfig with SSRF-safe discovery | MB-111 | DONE (Desktop) | DNS rebinding, fallback manual tests |
| MB-203 | P3 | OAuth2 major providers | MB-113, MB-202 | MOCK (needs owner's OAuth client) | PKCE, token refresh/revocation tests |
| MB-210 | P3 | Templates, signatures, shortcuts | MB-150 | DONE (Desktop; click-through pending) | VI/EN and keyboard test |
| MB-211 | P3 | Desktop OS notifications | MB-141 | DONE (Desktop; OS banner unverified) | No content displayed without opt-in |
| MB-218 | P3 | Scheduled send | MB-151 | DONE (Desktop; sends only while the app is open) | Restart, cancel, timezone tests |
| MB-221 | P3 | Full i18n coverage | MB-105 | DONE (Desktop surface; mail content and docs not translated) | All labels extracted and translated |
| MB-300 | P4 | Web hardened deployment / SSRF audit | MB-110, MB-202 | NOT STARTED | Pen test / threat model closed |
| MB-301 | P4 | Backups, restores, migrations | MB-120, MB-113 | NOT STARTED | Recovery drill documented |
| MB-302 | P4 | Desktop signing, update, sandbox config | MB-101, MB-211 | IN PROGRESS (Windows build on GitHub Actions: 2 runs failed, fixes on `main`, third run not started; no signing) | Signed installers macOS/Win/Linux |
| MB-303 | P4 | E2E CI 3 providers + OS targets | MB-151 | NOT STARTED (the Desktop 1.0 security audit briefed as "MB-303" is recorded under MB-304) | Green automated matrix |
| MB-304 | P4 | Security/PII/fuzz/load review | MB-303, MB-300 | IN PROGRESS (Desktop audit + 9 fixes done; 2 Medium open: S-10 Windows, S-11 sqlx; fuzz/load not run) | No critical findings + issue log: `docs/04-security/AUDIT_1.0.md` (0 Critical, 0 High), evidence `docs/06-delivery/evidence/track-i-security.md` |
| MB-305 | P4 | v1.0 tagged release & migration notes | MB-304 | IN PROGRESS (version 1.0.0 built and installed locally, unsigned; tag `v1.0.0` created locally on 2026-11-15, nothing pushed, no Windows build) | Release artifacts and complete ops docs |

## Update template

```markdown
### MB-### — short title
- Status: IN PROGRESS | BLOCKED | DONE
- Date:
- Owner:
- Related feature IDs:
- Files changed:
- Tests executed (command + exact output summary):
- Security/privacy review:
- Demo vs real behavior:
- Follow-ups / blockers:
- Commit/PR:
```

## Priority (owner decision, 2026-11-14)

Desktop (Dioxus native, macOS first) is the primary target. Mobile/responsive layout stays in the code but its remaining gaps (mobile RED evidence, mobile visual review) are deferred and do not block the UI Acceptance Gate. Exactly one installed copy of the app is kept on the dev machine (`/Applications/MailBox.app`); new builds replace it.

Build/CI cost policy: macOS and Linux are built and checked locally; GitHub Actions runs the Windows job only, on a `v*` tag or manual start. The earlier `ui-desktop-macos` and `e2e-web` jobs written in MB-101 were removed before ever running (see `docs/05-engineering/QUALITY_GATES.md`).

## Release tags

| Tag | Date | Contents | Notes |
|---|---|---|---|
| `v0.2.0` | 2026-11-14 | MB-101, 102, 103, 104, 112, 120, 121; MB-111 and MB-113 partly | Local annotated tag, not pushed (pushing a `v*` tag starts the Windows CI job). The app installed at `/Applications/MailBox.app` was built just before the version bump, so it still reports 0.1.0. |

## Work log

### MB-101 — Compile/run Web + Desktop, pin Cargo.lock
- Status: DONE (2026-11-14)
- Date: 2026-11-14
- Related feature IDs: UI-01, UI-02 (foundation only)
- Files changed: `apps/mailbox-ui/src/main.rs` (2 compile fixes, `data-testid`s, ssr tests), `apps/mailbox-ui/Cargo.toml` (dev-dep `dioxus-ssr`), rustfmt across workspace, `Cargo.lock` (new), root dotfiles restored from `MailBox-Rust-Starter/` (`.gitignore`, `.editorconfig`, `.env.example`, `.github/workflows/ci.yml`), CI workflow (later reduced to a tag-triggered Windows job), `e2e/` Playwright harness.
- Toolchain: rustc/cargo 1.97.0 via rustup (`~/.cargo/bin` must precede Homebrew cargo 1.96.1 in PATH), target `wasm32-unknown-unknown`, `dx` 0.7.10, Dioxus 0.7.10, wry 0.53.5, Playwright 1.64.1 (test-only).
- Compile fixes (no architecture change): `format!("{description.to_lowercase()}")` → positional arg (Compose templates); `active_tab.to_lowercase()` → `active_tab().to_lowercase()` (Settings); removed unused `mut`.
- Tool quirk: `dx serve` panics when run inside `apps/mailbox-ui` because the workspace `default-members` paths are canonicalized relative to cwd (dioxus-cli 0.7.10 `workspace.rs:325`). Workaround: always pass `-p mailbox-ui`.
- Tests executed:
  - `cargo fmt --all -- --check` → clean
  - `cargo test --workspace --exclude mailbox-ui` → api 1, contracts 2, core 2 passed
  - `cargo test -p mailbox-ui` (dioxus-ssr) → 2 passed (`shell_exposes_navigation_test_ids`, `shell_never_claims_real_sync`)
  - `cargo clippy --workspace --exclude mailbox-ui --all-targets -- -D warnings` → clean
  - `cargo clippy -p mailbox-ui --no-default-features --features desktop -- -D warnings` → clean (future-incompat note from transitive `block 0.1.6`)
  - `cargo check -p mailbox-ui --no-default-features --features web --target wasm32-unknown-unknown` → Finished
  - `cargo check -p mailbox-ui --no-default-features --features desktop` → Finished
  - `dx serve -p mailbox-ui --web` → "Build completed successfully in 87.31s"
  - `dx serve -p mailbox-ui --desktop` → "Build completed successfully", `MailboxUi.app` process launched (macOS, aarch64)
  - E2E `pnpm exec playwright test smoke-real.spec.ts`: RED first (`getByTestId('app-shell')` not found) → GREEN `1 passed (desktop-1440)`, mobile project skipped by design (MB-102). Video/screenshot under `test-results/e2e/` (not committed).
- Security/privacy review: no new network surface, no credential handling; demo badge asserted by test; password field still disabled.
- Demo vs real behavior: unchanged — fixture data only, no IMAP/SMTP.
- Desktop evidence (2026-11-14, later session): `dx serve -p mailbox-ui --desktop` → "Build completed successfully", `MailboxUi.app` running; with user-granted screen access, Inbox, Compose, Settings, Connect and Thread were opened by clicking through the native window (800px window → icon-rail layout). Semi-manual check, not an automated Desktop E2E.

### UI Design review — Gate G1 (before MB-102)
- Status: APPROVED by user 2026-11-14 ("Duyệt, build UI")
- Date: 2026-11-14
- Deliverable: private design canvas "MailBox UI Review" (10 artboards: design system sheet, S01 light/dark, S03 thread with icon rail, S04 compose + demo-send dialog, S02 connect step 1, S05 settings, mobile 390 ×5, state gallery, remaining settings tabs).
- Intentional deviations from reference PNGs: demo badge instead of green "Đồng bộ hoàn tất"; password field disabled; settings account status "Demo"; quota marked "minh họa"; neutral provider tiles instead of vendor logos; `--muted` darkened to #5F6D84 for 4.5:1 contrast.
- Open questions: Lucide icon set, account colors, mobile bottom-nav layout, dark mode token-only, system font stack.
- Exit: user approval → start MB-102.1.

### Desktop install (dev machine, 2026-11-14)
- `dx bundle -p mailbox-ui --desktop --release --package-types macos` → `MailboxUi.app` (7.6 MB, id `app.mailbox.client`, v0.1.0, unsigned), moved to `/Applications/MailBox.app`; intermediate and debug `.app` bundles under `target/dx` removed so Spotlight lists one copy.
- Launched from `/Applications`: process runs and the styled Inbox renders (bundled assets load). The capture showed the window in a shrunken system thumbnail state, so the release build was not clicked through; the five-screen click-through evidence is from the debug Desktop build.

### MB-102 — UI design system + all screens (mock data)
- Status: DONE — UI Acceptance Gate approved by the owner on 2026-11-14 ("Duyệt, làm MB-103"), Desktop-first.
- Date: 2026-11-14
- Related feature IDs: NAV-01, NAV-03, NAV-04 (local demo state only), NAV-05/06 (client-side filter), READ-04 (fixture thread), SEND-01/02 (compose rules, no send), ACC-01/03 (form + detach confirm, no network), UI-01, UI-02, UI-03 (partial)
- Sub-slices: 102.1 shell/design system · 102.2 S01 inbox · 102.3 S03 thread · 102.4 S04 compose · 102.5 S02 connect · 102.6 S05 settings
- Files: `apps/mailbox-ui/src/{app.rs,components/,screens/,state/}`, `assets/{tokens.css,main.css}`, `crates/mailbox-contracts/src/lib.rs`, `e2e/*-real.spec.ts`, `e2e/page-objects/*`
- TDD evidence:
  - RED (desktop-1440, against the MB-101 UI snapshot): 32 failed / 1 passed (smoke) — failures were missing test ids/behaviour. No RED run exists for the `mobile-375` project (gap).
  - GREEN: `cd e2e && pnpm exec playwright test --workers=1` → **75 passed, 0 failed, 17 skipped** (skips are viewport-scoped tests by design; includes MB-104 specs). 5.0 min.
  - Unit/ssr: `cargo test -p mailbox-ui` → 28 passed (state logic for mail/compose/connect, route rendering, demo-truth and accessible-name checks). `cargo test --workspace --exclude mailbox-ui` → 8 passed.
  - `cargo fmt --all -- --check` clean; clippy `-D warnings` clean for desktop (all targets) and web (wasm32).
- Flakiness note: runs taken while the machine load average was 100–600 (other workloads) produced timeout failures (2–8 per run); every one passed on `--last-failed` and in the clean full run. One spec (`filter chips`) was tightened to assert `aria-pressed` after each un-toggle instead of relying on click timing. `AppShell.open` waits up to 45 s for first paint of the debug WASM bundle.
- Fixes found by E2E/visual review: duplicate `empty-state` test id (reader placeholder renamed), mobile compose button covering form actions (now inbox-only), gear icon, mobile brand clipping, stretched default badge, rail buttons without accessible names, squeezed compose button in the icon rail.
- Security/privacy review: password input disabled (ssr + E2E test); send opens a dialog with `data-sent="false"`, no network; connect wizard has no certificate bypass and stores nothing; detach is local-only and says so; no credentials, tokens or mail content logged; no new network surface.
- Demo vs real behavior: everything is fixture data in memory; status is always "Demo".
- Not verified / gaps: mobile RED; screenshot-diff baselines (`toHaveScreenshot`) not added; VoiceOver/NVDA and 200–400% zoom not tested; Desktop checked by hand at one window size only; global shortcuts other than Esc not implemented (Ctrl+K placeholder text only).

### MB-104 — Routes and deep links
- Status: DONE — approved with the UI Acceptance Gate on 2026-11-14
- Date: 2026-11-14
- Files: `apps/mailbox-ui/src/routes.rs`, `app.rs`, `state/mod.rs`, `components/shell.rs`, `Cargo.toml` (dioxus `router` feature), `e2e/routes-real.spec.ts`
- Routes: `/` → `/inbox`, `/messages/{id}/thread`, `/compose`, `/accounts/connect`, `/settings`, `/settings/{tab}`, catch-all not-found (shell kept).
- TDD evidence: RED `routes-real.spec.ts` desktop → 10 failed (9 for missing routes; 1 was a page-load timeout while the server was still building). GREEN is part of the 75-pass run above (reload, back/forward, unknown path, settings tab in URL). ssr test `every_route_renders_its_screen_on_shared_code` covers 10 paths with in-memory history. Desktop: navigation between all five screens checked by hand.
- Limits: filters, selected message and drafts are not in the URL; a reload resets demo state by design.

### MB-103 — UI data adapter
- Status: DONE
- Date: 2026-11-14
- Related feature IDs: NAV-01 (read path), SYNC-06 (load/error state), SEC-05
- Files: `apps/mailbox-ui/src/adapters/{mod,native,http}.rs`, `state/mod.rs` (`LoadState`, `reload`), `app.rs` (loading skeleton, error + retry), `crates/mailbox-core/src/demo.rs` (`BootstrapPort`, `DemoStore`, `validate`, `load_bootstrap`), `apps/mailbox-api/src/main.rs` (CORS, core use case), `e2e/data-adapter-real.spec.ts`, `e2e/api-server.ts`
- Design: `trait MailboxDataSource` (`bootstrap`). Desktop → `NativeDataSource` → `mailbox-core` use case in-process (no HTTP). Web → `HttpDataSource` → `GET /api/v1/demo/bootstrap` (gloo-net, wasm-only dependency), validated with the same core rules. No silent fallback: if the API is down the Web UI shows an error with "Thử lại". API origin is fixed at build time (`MAILBOX_API_URL`, default `http://127.0.0.1:3000`), never taken from the page URL.
- TDD evidence: RED `data-adapter-real.spec.ts` → failed (`waitForRequest` timeout: UI made no API call). GREEN → 3 passed, including a real outage test that stops and restarts the API process. Full suite: **78 passed, 0 failed, 18 skipped** (viewport-scoped). `cargo test`: core 6, api 4 (payload, CORS allow/deny, loopback-only origin list, GET only), contracts 5, UI 30 (adapter contract, error copy). fmt clean; clippy `-D warnings` clean for workspace, UI desktop and UI web.
- Security/privacy review: CORS accepts only `http://127.0.0.1:*` / `http://localhost:*` origins even when `MAILBOX_CORS_ORIGINS` is set to something else (tested); GET only; API still binds 127.0.0.1 and has no auth — still not deployable. DTOs carry no secrets (tested through the adapter contract and the API test). Error copy shows no host or internals. Nothing is logged about mail content.
- Demo vs real behavior: data is still fixtures on both platforms; only the delivery path changed.
- Desktop: release bundle rebuilt and installed over `/Applications/MailBox.app` (single copy); it shows the inbox loaded through the native source and navigates to Settings and Compose.
- Limits: only the initial snapshot goes through the adapter. Star/read/archive/delete, drafts and settings remain in-memory UI state (server mutations are MB-132/MB-144). Desktop source is synchronous fixture access; a real async store arrives with MB-120.

### MB-120 — SQLite/PostgreSQL repositories and migration harness
- Status: DONE
- Date: 2026-11-14
- Related feature IDs: SYNC-01 (cursor persisted), SEC-01 (reference only, no secret columns), ACC-03 (local delete cascades)
- Files: `crates/mailbox-store/` (new), `crates/mailbox-core/src/store.rs` (port + records), `migrations/{sqlite,postgres}/0001_init.sql` (rewritten with portable types), `docs/02-architecture/adr/0005-portable-sql-schema.md`
- Scope: accounts, folders, messages (envelope cache), sync cursors. Drafts, send jobs and preferences have tables but no repository methods yet (MB-133, MB-151).
- Tests executed: `MAILBOX_TEST_PG_URL=postgres://…@127.0.0.1:55432/postgres cargo test -p mailbox-store -- --include-ignored` → 4 passed: SQLite contract, PostgreSQL 16 contract (same function), SQLite restart + re-run of migrations, no secret-like columns in either schema. The contract covers owner isolation (read, write, takeover, delete), `(account, folder, uidvalidity, uid)` identity, same UID under a new UIDVALIDITY, atomic batch rejection, cursor replace, cascade on account delete. Workspace: 18 passed. clippy `-D warnings` clean.
- TDD note: tests and implementation were written together and passed on the first run; no separate RED run was recorded for this task.
- Security/privacy review: no credential columns (tested); every query filters by owner and foreign rows look like `NotFound`; errors are static codes with no SQL or data; the PostgreSQL URL is never logged. PostgreSQL test used a throwaway local container with a test-only password, removed afterwards.
- Demo vs real behavior: the store is real and tested, but the apps do not use it yet — Desktop and API still serve fixtures. It gets wired in with MB-121.
- Not verified / gaps: PostgreSQL connection uses no TLS (local test only; required before any deployment, MB-300); no 100k-row performance test; no backup/restore drill (MB-301).

### MB-113 — Secret store (Desktop half)
- Status: IN PROGRESS — Desktop OS keychain done; Web AEAD vault not started (depends on MB-110, deferred under the Desktop-first priority).
- Date: 2026-11-14
- Related feature IDs: SEC-01, ACC-04
- Files: `crates/mailbox-core/src/secrets.rs` (`Secret`, `SecretStore`, reference validation), `crates/mailbox-secrets/` (new: `KeychainStore`, `MemoryStore`)
- Tests executed: `cargo test -p mailbox-core -p mailbox-secrets` → core 8, secrets 2 passed. `cargo test -p mailbox-secrets -- --ignored` → 1 passed on the real macOS Keychain (put, rotate, idempotent delete under service `app.mailbox.client.test`; `security find-generic-password` afterwards finds nothing). clippy clean.
- Security/privacy review: `Secret` has no `Display`/`Clone`/`PartialEq`/serde, prints `<redacted>` and wipes its bytes on drop; backend errors are replaced by static codes so no secret or keychain attribute reaches a log; references are limited to a safe alphabet; the crate is native-only and not a dependency of the Web bundle.
- Demo vs real behavior: real store, but nothing calls it yet — the connect form still does not accept a password. It is used from MB-111.
- Not verified / gaps: Windows Credential Manager and Linux Secret Service backends compile-configured but untested; Web vault (AEAD, key management, rotation) not started; wipe-on-drop does not cover copies made by the OS keychain API itself.

### MB-111 — IMAP connect, TLS, auth error states
- Status: IN PROGRESS — implemented and tested; the real login must be done by the owner in the Desktop app (the agent does not handle real passwords).
- Date: 2026-11-14
- Files: `crates/mailbox-imap/` (new), `apps/mailbox-ui/src/adapters/connect.rs`, `screens/connect.rs`, `main.rs` (Desktop window 1280×700, min 800×560)
- Behaviour: implicit TLS only, certificate chain + host name always verified (webpki roots), no bypass switch; STARTTLS refused as unsupported rather than downgraded; credentials sent as IMAP literals only after TLS is verified; 15 s step timeouts; 8 KB line limit. Desktop connect form now takes a password, runs the real check and shows the real result; nothing is saved (account persistence is MB-121). Web keeps the password field disabled.
- Tests executed: `cargo test -p mailbox-imap` → dialogue 7 passed (exact password bytes, wrong password, LOGINDISABLED sends no credentials, non-IMAP peers, oversized lines, CR/LF injection refused), endpoint 1 passed. `cargo test -p mailbox-imap --test network -- --ignored` (internet): expired / wrong-host / self-signed / untrusted-root certificates → `CertificateInvalid`; unreachable host → `Unreachable`; real provider host probed without credentials → valid certificate, `IMAP4rev1 IDLE LITERAL+ AUTH=PLAIN AUTH=LOGIN`. The fifth network test (silent TLS peer → `Timeout`) had its expectation corrected and was NOT re-run. `cargo test -p mailbox-ui` → 31 passed.
- Update (same day): added a Demo/Real mode switch (Desktop only, top bar). Real mode drops all fixture data and shows an honest empty state; the connect check additionally reports the INBOX message count through a read-only `EXAMINE`. Tests: `cargo test -p mailbox-imap` 7 + 1 passed, `cargo test -p mailbox-ui` 32 passed (includes "real mode shows no demo data"), clippy clean on workspace, UI desktop and UI web, Web E2E 78 passed / 0 failed. Release bundle rebuilt and installed as the single copy at `/Applications/MailBox.app`; it opens at 1280×700 with the full sidebar layout.
- NOT verified: real login against the provider (owner must type the password in the app); the mode switch and the new connect form were not clicked on the installed build (background capture of the window was unusable); the corrected network test (silent TLS peer → `Timeout`) was not re-run. The mode choice is not persisted: the app starts in Demo.
- Provider details for testing are in `docs/05-engineering/TEST_MAIL_PROVIDER.md` (no secrets).
- Security note: the owner pasted a mailbox password into the chat. It was not used, stored or written to any file; rotating it is recommended.

### MB-112 — SMTP capability / identity test
- Status: DONE (dependency note: listed after MB-113; the Desktop half of MB-113 it needs is done).
- Date: 2026-11-14
- Related feature IDs: SEND-03 (TLS + auth, no "sent" before confirmation), ACC-06 (sender identity), ACC-01
- Files: `crates/mailbox-net/` (new: verified-TLS transport shared by IMAP and SMTP, 30 s step timeout), `crates/mailbox-smtp/` (new), `crates/mailbox-imap/` (uses mailbox-net), `apps/mailbox-ui/src/adapters/connect.rs`, `screens/connect.rs`
- Behaviour: implicit TLS on 465, STARTTLS on other ports; no STARTTLS → stop (`StartTlsUnavailable`), never plaintext; pre-TLS capabilities discarded; bytes smuggled after the STARTTLS reply rejected; AUTH PLAIN (LOGIN fallback) only after verified TLS; a check never issues MAIL/RCPT/DATA. A separate explicit `send` exists for one plain-text message with outcomes `Accepted` / `Rejected` / `Unknown` (connection lost after DATA → `Unknown`, no retry). Desktop connect form now checks IMAP and SMTP separately and shows both results.
- Tests executed:
  - `cargo test -p mailbox-smtp` → unit 5, dialogue 11 (scripted server: exact credential bytes, wrong password, no password mechanism, non-SMTP peers, CR/LF injection, STARTTLS required/refused/smuggled bytes, send accepted with dot-stuffing, rejected recipient sends nothing, lost connection → Unknown with exactly one attempt, failed auth never reaches MAIL), endpoint 1.
  - Real network, no credentials: expired / wrong-host / self-signed certificates → `CertificateInvalid`; provider host on 465 (implicit TLS) and 587 (STARTTLS) → `PIPELINING | SIZE 52428800 | ETRN | AUTH PLAIN LOGIN | ENHANCEDSTATUSCODES | 8BITMIME | DSN`.
  - Real network, owner's `.env.smtp` (authorised by the owner; values read by the test process, never printed). Correction: that file holds a different mailbox on `mail2.provider.example`, not `test@example.org`. `owner_login_is_accepted` → login OK on 465, From matches login. `owner_send_one_test_message` → one message to the owner's own address, outcome `Accepted`.
  - `cargo test -p mailbox-imap` 7 + 1 still pass after the transport extraction; `cargo test -p mailbox-ui` 32 pass; workspace 47 pass; clippy `-D warnings` clean (workspace, UI desktop, UI web); Web E2E 78 passed / 0 failed. Release bundle rebuilt and installed as the single copy at `/Applications/MailBox.app`.
- Security/privacy review: no option to skip certificate validation anywhere; credential buffers (base64 token, command line) are wiped after use; errors are static enums; the test recipient and password never enter the repository; `.env.smtp` is git-ignored.
- Not verified / gaps: delivery to the recipient's inbox (server acceptance only — owner to confirm); AUTH LOGIN fallback and `Rejected`/`Unknown` only against the scripted server; no OAuth; the sender does not yet build multipart/attachments, save to Sent, or persist a send job (MB-150/MB-151); Desktop form click-through with SMTP not done by the agent.

### MB-121 — Folder discovery, UIDVALIDITY cursor persistence (Desktop)
- Status: DONE for Desktop. (Dependency note: MB-111's real login was confirmed here with the `.env.smtp` account; the owner's manual check with the other mailbox is still open.)
- Date: 2026-11-14
- Related feature IDs: NAV-01, NAV-02 (discovery + roles), SYNC-01, SYNC-04, SEC-01, ACC-01, ACC-03
- Files: `crates/mailbox-imap/src/{lib,parse}.rs` (read-only `Session`, parsers), `crates/mailbox-core/src/sync.rs`, `store.rs` (`reset_folder`), `crates/mailbox-store/` (reset + tests), `apps/mailbox-ui/src/adapters/account.rs` (new), `state/mod.rs`, `screens/{connect,inbox,settings}.rs`, `app.rs`
- Behaviour: session issues only LIST, EXAMINE and FETCH with `BODY.PEEK` (never SELECT/STORE/EXPUNGE…). First sync takes the newest 100 envelopes of INBOX; later syncs fetch only UIDs above the cursor; a UIDVALIDITY change discards the old epoch and rebuilds; messages are stored before the cursor moves; a server without UIDVALIDITY is refused. Desktop: "Lưu tài khoản và đồng bộ" puts the password in the OS keychain, metadata and envelopes in SQLite under the app data directory, and shows real mail in Real mode; the app starts in Real mode when an account is saved; refresh fetches new mail using the keychain secret; removing an account deletes local cache and keychain entry only. If the first sync fails, the account and secret are rolled back.
- Tests executed:
  - `cargo test -p mailbox-imap` → parsers 8 (LIST variants, modified UTF-7, EXAMINE, FETCH with literal, RFC 2047, hostile headers, dates), dialogue 7, session 1 (scripted server; asserts no mutating command and `BODY.PEEK` only), endpoint 1.
  - `cargo test -p mailbox-store` → contract 3 + PostgreSQL 1 (with `reset_folder`), sync 5 (discovery, window then incremental across a restart on a real SQLite file, UIDVALIDITY rebuild, failed fetch leaves cursor/data untouched, missing UIDVALIDITY refused).
  - Real provider, read-only (`real_sync`, `.env.smtp` account on `mail2.provider.example`): login over verified TLS; 5 folders with roles; after one self-addressed test message, first sync stored 1 row with subject, sender and date; second sync incremental, 0 new.
  - `cargo test -p mailbox-ui` → 36 (snapshot mapping without credential reference, account id/secret reference safety, time formatting, real mode with and without a saved account).
  - Whole workspace: 97 passed; clippy `-D warnings` clean (workspace, UI desktop, UI web); Web E2E 78 passed / 0 failed. Release bundle rebuilt and installed as the single copy at `/Applications/MailBox.app`; on launch it created `mailbox.sqlite3` in the app data directory.
- Security/privacy review: password only in the OS keychain (`imap.<account-id>` under service `app.mailbox.client`), never in SQLite, logs or errors; the UI snapshot carries no `credential_ref` (tested); the password stays in UI memory only between a successful check and save and is cleared on back, retry, save and unmount; local database holds subjects/senders in plain text under the user's app-data directory (no encryption at rest yet — PRIVACY.md item, open); nothing is written to the server.
- Demo vs real behavior: Real mode shows only server data; message bodies are a fixed placeholder (MB-130). Demo mode unchanged.
- Not verified / gaps: the Desktop UI flow (save, refresh, remove) was not clicked through by the agent — logic is covered by tests of the same functions, the keychain prompt and UI wiring are not; two wrong-password IMAP logins were sent to `mail.provider.example` by mistake before the mailbox mix-up was noticed; only INBOX is synced; folder names are stored in modified UTF-7 (decoder exists, not yet used for display); times are shown in UTC; Windows/Linux data dir and keychain untested; Web has no real mode data (needs MB-110).

### 2026-11-15 — MB-122, MB-130, MB-131, MB-132, MB-133, MB-143, MB-144, MB-150, MB-151 (Desktop, wave 1)

- Built by three parallel work branches, merged into `main` (merge commits `MB-130`, `MB-150`, `MB-132`) and joined in `e394ba5`. Details and per-track evidence: `docs/06-delivery/evidence/track-a-read-bodies.md`, `track-b-flags-folders.md`, `track-c-compose-send.md`.
- Real now (Desktop, Real mode): message bodies fetched read-only, MIME parsed with limits, HTML sanitized and shown in a sandboxed frame with remote images blocked until asked, attachments saved on request; folder roles and multi-folder sync; star / read / archive / trash / move reach the server through a pending-operation queue with undo; local drafts with autosave; real SMTP send with `Sent / Failed / Unknown` states, no automatic resend, one copy in Sent.
- Join work after merge: Sent folder taken from stored folder roles (server listing as fallback); reply quotes the cached body text; removing an account also removes its separate SMTP keychain entry.
- Verification on the merged tree (2026-11-15):
  - `cargo fmt --all -- --check`: pass.
  - `cargo test --workspace`: 278 passed, 0 failed, 17 ignored (opt-in network / PostgreSQL).
  - `cargo clippy --workspace --all-targets -- -D warnings`, UI `--features desktop`, UI `--features web --target wasm32-unknown-unknown`: pass.
  - Real provider (test mailbox, self-send only): `real_body`, `real_sync`, `real_actions` (flag + move to Trash + clean-up), `real_send` (self-send, exactly one copy in Sent): 4 of 4 pass.
  - Web E2E `--project=desktop-1440`: 45 passed, 2 skipped, 1 failed once under load (`compose-real.spec.ts:79`) and passed on rerun.
  - Desktop bundle built and installed as the single copy at `/Applications/MailBox.app`; the app starts.
- Not verified: the new flows by click in the installed app (window could not be captured while the screen was idle); PostgreSQL migrations `0002`-`0004` (Docker was not running); Windows and Linux; servers without MOVE / UIDPLUS (scripted only); HTML mail and attachments from a real mailbox (synthetic fixtures only).
- Known gaps carried forward: one IMAP connection per action (kept-alive session is MB-140); undo data is lost on restart; no settings UI for folder-role override; reply `References` chain has one element; forward does not carry original attachments; editor is plain text; Sent copy has no Bcc header.
- Mocked or limited items: `docs/06-delivery/LIMITATIONS_1.0.md`.

### 2026-11-15 — MB-140, MB-141, MB-200, MB-201, MB-202, MB-203 (mock), MB-210, MB-211 (Desktop, wave 2)

- Built by three parallel work branches on other models (two Opus 5.5, one Sonnet 5.5), reviewed and merged by the supervising session (merge commits `MB-200`, `MB-140`, `MB-202`). Evidence: `docs/06-delivery/evidence/track-d-live-sync.md`, `track-e-search-threads.md`, `track-f-settings-features.md`. Rules for such sessions: `docs/06-delivery/TEAM_RULES.md`.
- Real now (Desktop, Real mode): one kept-alive IMAP session per account with IDLE or a 2-minute poll, reconnect with capped backoff, per-account status in the top bar, sidebar and settings; actions and body fetch borrow that session; local search (FTS5, Vietnamese with or without diacritics) with scope and filters, "load older", conversations from `Message-ID` / `References`, full `References` on replies; provider autoconfig behind an SSRF guard; stored settings, templates, signatures, keyboard shortcuts with a help overlay; new-mail notifications (count only by default).
- Mock: OAuth sign-in (MB-203) is a labelled explanation panel; no browser, token or network.
- Join work after merge: changes made by the live worker now also recompute conversations, refresh the search index and announce new mail (before, only a manual sync did).
- Review before merge: no agent attribution in any commit; no real address, credential or mail content in the diffs (only the test host name and public provider host names).
- Verification on the merged tree, rerun by the supervising session (2026-11-15):
  - `cargo fmt --all -- --check`: pass.
  - `cargo test --workspace`: 444 passed, 0 failed, 23 ignored.
  - clippy for workspace, UI desktop, UI web (wasm32): pass.
  - Real provider, self-send only: `real_body`, `real_sync`, `real_actions`, `real_live` (mail arrives without a manual refresh), `real_threads` (reply pair becomes one conversation, found by search), `real_send`: 6 of 6 pass.
  - Web E2E `--project=desktop-1440`: 45 passed, 2 skipped, 1 failed (`compose-real.spec.ts:79`) in the full run on two runs; the same spec alone passed 3 of 3. Treated as load-dependent, not fixed.
  - Desktop bundle built and installed as the single copy at `/Applications/MailBox.app`; the app starts.
- Not verified: any of the new flows by click in the installed app (the window could not be captured while the screen was idle); the OS notification banner; PostgreSQL migrations `0002`-`0006` (Docker not running); Windows and Linux.
- Known gaps: Sent reconciliation and the first save still open their own connection; settings "auto-sync" toggle and interval are not wired to the worker; search has no body snippet and no server-side search; `j`/`k` order was written before conversation rows existed and needs a check in the app; the connect wizard's final check resolves the host again outside the SSRF guard (certificate validation still applies); a draft whose body is only a line starting with `-- ` counts as empty.

### 2026-11-15 — MB-218, MB-105, MB-221, MB-304 (Desktop, wave 3)

Three worker sessions on other models; the coordinator reviewed the diffs, merged and reran every gate on the merged tree.

- Real: scheduled send (pick a time, edit or cancel before it, sent while the app is open; more than 12 hours late waits for the user), Sent copy over the held live session, forward keeps the original attachments, the auto-sync setting starts and stops the live workers.
- Real: English interface, chosen in Settings, applied without restart; Vietnamese is the default and unchanged. Strings added by the other two tracks were translated at merge (`i18n/en_wave3.rs`).
- Security audit (`docs/04-security/AUDIT_1.0.md`): 36 findings, none Critical or High; 7 fixed with tests (wizard connects only to SSRF-checked addresses, draft emptiness, MIME work off the UI task, database file 0600, three parser panics). Open Medium: no total byte budget per IMAP command (S-08), links in mail open without confirmation (S-09), no Mark-of-the-Web on Windows (S-10), SQLite advisory through sqlx (S-11).
- Not built: server Drafts sync (drafts stay local, LIMITATIONS row).
- Gates on the merged tree: `cargo fmt --all -- --check` pass; clippy workspace, UI desktop, UI web pass; `cargo test --workspace` 506 passed, 0 failed, 25 ignored; real provider 7/7 (`real_body`, `real_sync`, `real_actions`, `real_live`, `real_threads`, `real_sent_copy`, `real_send`); Web E2E desktop-1440 49 passed, 2 skipped, 0 failed.
- Version 1.0.0 bundled for macOS (unsigned) and installed as the single copy in `/Applications`; it starts.
- Not verified: click-through of any flow in the installed app, the macOS notification banner, PostgreSQL migrations 0002-0007, Windows and Linux, fuzz and load tests.

### 2026-11-15 — MB-304: audit findings S-08 and S-09 fixed (Desktop)

- S-08: one IMAP command may read at most 32 MiB of lines and literals (`COMMAND_BUDGET` in `mailbox-imap`); the budget resets per command and is charged before a literal is allocated. The requested message body keeps its own 25 MB limit. RED first: `crates/mailbox-imap/tests/budget.rs` failed 2 of 3 (48 MiB floods were accepted), then 3 of 3 pass.
- S-09: links in the mail frame are rewritten to the inert `mailbox-link:` scheme at display time; the Desktop WebView navigation handler blocks the click and the UI shows the real host and full address and asks before opening. Addresses with a user name before the host, other schemes, over 2048 bytes or malformed are refused. RED first: 7 of 8 core tests and 5 SSR tests failed on stubs; now 8 + 1 + 5 pass. One older SSR test that required a live `https://` link in the frame was changed to require the guarded form.
- New direct dependencies, both already in the lock file: `url` (mailbox-core), `webbrowser` (mailbox-ui, native).
- Gates: `cargo fmt --all -- --check` pass; clippy workspace, UI desktop, UI web pass; `cargo test --workspace` 523 passed, 0 failed, 25 ignored; real provider IMAP 6/6 (`real_body`, `real_sync`, `real_actions`, `real_live`, `real_threads`, `real_sent_copy`); `real_send` not rerun (SMTP unchanged); Web E2E desktop-1440 49 passed, 2 skipped, 0 failed.
- Release bundle rebuilt and installed as the single copy in `/Applications`; it starts and loads the interface with the navigation handler in place.
- Not verified: a click on a real link in the installed app (the screen was locked during this session, so no click-through was possible). The claim "the WebView reports the blocked `mailbox-link:` click on macOS" rests on reading wry/dioxus-desktop sources, not on a run. If the report does not arrive, the link does nothing (fail closed). Windows and the Web build: a click does nothing by design (LIMITATIONS row).
- Open Medium: S-10 (Mark-of-the-Web on Windows), S-11 (SQLite advisory through sqlx).

### 2026-11-15 — MB-304: S-09 link click made to work, checked by hand (Desktop)

- Finding: with the first S-09 fix a click on a link did nothing in the installed app. A trace in a dev build showed the navigation handler is called for `about:srcdoc` but never for a link clicked inside the sandboxed frame (real mouse click): WKWebView does not report that navigation. Fail closed, but unusable.
- Fix: the frame's sandbox is now `allow-top-navigation-by-user-activation` and the frame document has `<base target="_top">`, so a click asks the top frame to navigate to the inert `mailbox-link:` address; the shell refuses it and opens the confirmation dialog. `FRAME_SANDBOX` in `adapters/body.rs` holds the reasoning.
- Second finding: `guard_links` panicked on text with multi-byte characters before a tag (any Vietnamese HTML mail with the first S-09 fix). Fixed, with a regression test.
- Demo: the message "Cảnh báo bảo mật cho tài khoản" carries five labelled sample links so the dialog can be tried without an account (LIMITATIONS row).
- Checked by hand (dev build with a trace, then the installed release build): https link shows host and full address; http shows the unencrypted warning; `xn--` shows the look-alike warning; `https://name@host/` is refused with no open button; mailto offers the mail program; Cancel and Escape close; "open" closes the dialog and hands over to the browser.
- Tests added: `link::tests::text_with_multi_byte_characters_is_copied_unchanged`, `ui_tests::demo_security_message_offers_guarded_sample_links`, `mailbox-mime` `nothing_but_a_link_can_start_a_navigation`; sandbox and `<base>` assertions updated. The demo test was written together with the code, not before it.
- Not verified: a link inside a real synced message (no account is saved in the installed app); Windows.

### 2026-11-15 — MB-305 / MB-302: tag pushed, Windows build not passing yet

- `main` and tag `v1.0.0` (at `afc4ebd`) pushed on the owner's request.
- Windows build, run 43675072909 (tag): failed in "Core and API tests". `bodies_survive_reopening_an_existing_database` removed its temporary directory while the SQLite file was open (os error 32). Three store tests fixed (cleanup is best effort), commit `910659d`.
- Windows build, run 43677148430 (manual, `main`): core tests and UI tests passed on Windows; failed in "UI lint". CI installs rustc 1.100, local is 1.97; two new warnings (`clippy::needless_bool` in `mailbox-store/src/search.rs`, `unreachable_code` from the `Routable` derive). Fixed in `31e9ad1`.
- Checked locally with rustc 1.100.0: `cargo test --workspace --locked` 526 passed, 0 failed, 25 ignored; clippy workspace and UI desktop clean. With rustc 1.97: clippy workspace, UI desktop, UI web clean; fmt clean.
- Not run: a third Windows build; the steps "Install Dioxus CLI" and "Bundle Windows desktop app" have never run. Web clippy with rustc 1.100 (no wasm target for that toolchain locally). Web E2E after the second S-09 fix.
- Open work and questions for the owner: `docs/06-delivery/SESSION_HANDOFF_2026-11-15.md`.

### 2026-11-15 — MB-302 / MB-305: public-release preparation

- Web E2E rerun after the second S-09 fix (Playwright, desktop-1440, real `dx serve` + `mailbox-api`): 49 passed, 2 skipped, 0 failed. That Web build used rustc 1.97.
- Toolchain pinned to rustc 1.100.0 in `rust-toolchain.toml` and in the workflow. All local gates rerun on 1.100.0: fmt clean; clippy workspace, UI desktop, UI web clean; `cargo test --workspace --locked` 526 passed, 0 failed, 25 ignored.
- Workflow renamed "Windows release build": `workflow_dispatch` only, with a required `tag` input that is checked and checked out; no trigger on pushes, tags or pull requests. Not run in this form yet.
- README: six screenshots (demo mode, Web build), licence line, pointers to `SECURITY.md` and `CONTRIBUTING.md` (both new).
- Removed from the tree: the starter copy, its 8 MB zip, the stale manifest, the 2026-11-14 handoff. Provider host names, the test address and local paths replaced by placeholders in docs and in one unit test.
- Secret scan over the whole history: no password, token, private key, real `.env` or database file; commit author uses a GitHub noreply address. The history still contains the real test address and provider host names in old doc commits: owner decision before going public (see the handoff, section 7).
- Not done: repository visibility unchanged (private); no history rewrite; no `v1.0.1`; no Windows build run; GitHub security settings not changed.

### 2026-11-15 — MB-305: version 1.0.1, history squashed (owner decision)

- Version 1.0.1 (same features as 1.0.0). `cargo test --workspace --locked` on rustc 1.100.0: 526 passed, 0 failed; fmt clean.
- Owner chose a clean history before going public: the repository now starts at one root commit with the 1.0.1 tree. Tags `v0.2.0` and `v1.0.0` removed; `v1.0.1` is the current tag. Commit hashes quoted in older entries of this file no longer exist in the repository.
- Full previous history kept in a local bundle outside the repository (owner's machine only).
- Reason: old doc commits held the real test address and provider host names. GitHub may still serve old commits by SHA for a while; old Actions run logs remain until deleted.

## Blockers / risks

- Dioxus UI currently has one `main.rs` and Unicode icon placeholders. Refactor to components and use reviewed icon assets.
- No database engine wiring and no connector implementation.
- Generated design screenshots contain fictional dates/names/capability statuses; production UI must show real data, statuses and localized dates.
