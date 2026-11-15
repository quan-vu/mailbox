# Changelog

## Unreleased

## 1.0.1 — 2026-11-15 (Desktop; same features as 1.0.0, first tag meant to build on Windows)

- Toolchain pinned to rustc 1.100.0 (`rust-toolchain.toml` and the Windows workflow).
- The Windows release build starts only by hand for a named version tag.
- Store tests and two compiler warnings fixed so the Windows build can pass its test and lint steps.
- README screenshots, `SECURITY.md`, `CONTRIBUTING.md`; starter leftovers removed; provider details in docs replaced by placeholders.

## 1.0.0 — 2026-11-15 (Desktop; built locally, unsigned; tag `v1.0.0` local, not pushed)

- Desktop, Real mode: read message bodies (MIME, sanitized HTML in a sandboxed frame, remote images blocked until asked, attachments saved on request).
- Desktop, Real mode: folder roles, multi-folder sync; star, read, archive, trash and move reach the server, with undo.
- Desktop, Real mode: local drafts with autosave; real sending with sent / failed / unknown states and one copy in Sent. Demo mode still sends nothing.
- Desktop, Real mode: live sync (IDLE or poll) with reconnect and per-account status.
- Desktop, Real mode: local search, "load older", conversations.
- Desktop, Real mode: provider autoconfig, stored settings, templates, signatures, keyboard shortcuts, new-mail notifications.
- Desktop, Real mode: scheduled send (only while the app is open), Sent copy over the live session, forward keeps attachments, auto-sync setting.
- Interface language: Vietnamese (default) and English.
- Security audit with 9 fixes; open findings in `docs/04-security/AUDIT_1.0.md`.
- Links in a message never open by themselves: a click shows the real address and asks first.
- Demo: the security notice message has sample links to try that confirmation without an account.
- One IMAP command may read at most 32 MiB of responses (message bodies keep their own limit).
- OAuth sign-in is a labelled mock.
- `docs/06-delivery/LIMITATIONS_1.0.md` lists what is mocked or limited in 1.0.

## 0.2.0 — 2026-11-14

First version that talks to real mail servers (Desktop, read-only inbox). Web remains a demo.

Not in this version: reading message bodies and attachments (MB-130/131), flag sync and folder actions (MB-132/144), background sync (MB-140), composing and sending from the app (MB-150/151), Web sign-in and Web secret vault (MB-110, MB-113), localisation (MB-105), signed installers, Windows and Linux builds (untested).

- MB-101: workspace compiles for Web (wasm32) and Desktop (macOS) on rustc 1.97.0 / Dioxus 0.7.10; `Cargo.lock` pinned; two compile errors in `main.rs` fixed; rustfmt applied.
- MB-101: Playwright Web E2E harness (`e2e/`, test-only) with real `dx serve` server and smoke spec; `dioxus-ssr` UI tests; CI limited to a tag-triggered Windows job; macOS/Linux checks run locally.
- MB-102: UI split into `components/`, `screens/`, `state/`; token stylesheet, Lucide-style inline SVG icons, light/dark theme, responsive layout with one-pane mobile, drawer and bottom navigation. All five screens interactive on demo fixtures (filters, search, star/read, archive/delete with undo, conversation view, recipient chips, templates, connect wizard with simulated outcomes, nine settings tabs). Demo truth kept: no password input, no SMTP send, no "connected" status.
- MB-102: contracts gain `AccountStatus::Demo`, `thread_id`, `to_line`, `received_at_full`, `labels` (all `serde(default)`, backward compatible); fixtures extended to 13 messages with one 5-message conversation.
- MB-103: data-source adapter. Desktop reads mailbox data through `mailbox-core` use cases in-process; Web reads it from `GET /api/v1/demo/bootstrap`, with a loading skeleton and an error state with retry (no silent fallback). Demo API gains loopback-only CORS.
- MB-121 (Desktop): read-only IMAP session, folder discovery with SPECIAL-USE roles and cursor-safe envelope sync (UIDVALIDITY rebuild). The Desktop app can save a verified account (password in the OS keychain, metadata in SQLite), sync its inbox and show real mail in Real mode.
- MB-112: `mailbox-smtp` crate — SMTP check over mandatory verified TLS (465 implicit, 587 STARTTLS, never plaintext) and a minimal explicit plain-text sender with Accepted/Rejected/Unknown outcomes. `mailbox-net` crate holds the shared TLS transport. Desktop connect form checks IMAP and SMTP separately.
- MB-111 (in progress): `mailbox-imap` crate — IMAP connection check over mandatory verified TLS; Desktop connect form runs it and reports the INBOX count (read-only). Demo/Real mode switch on Desktop; Real mode never shows fixture data. Desktop window defaults to 1280×700.
- MB-113 (Desktop half): secret-store port and `mailbox-secrets` crate with an OS keychain store; secrets are redacted in debug output and wiped on drop. Web vault still pending.
- MB-120: `mailbox-store` crate with SQLite and PostgreSQL implementations of the storage port, per-dialect migrations and a shared contract test suite (ADR 0005). Not yet used by the apps.
- MB-104: URL routes via Dioxus Router (`/inbox`, `/messages/{id}/thread`, `/compose`, `/accounts/connect`, `/settings[/{tab}]`), not-found page, reload/back/forward support.
- Restored root dotfiles (`.gitignore`, `.editorconfig`, `.env.example`, CI workflow).

## 0.1.0-starter — 2026-11-14

- Initialized all-Rust Dioxus Web/Desktop UI, Axum read-only fixture API, domain contracts/ports.
- Added five UI reference images and responsive demo screens.
- Authored PRD, requirements, system design, sync/storage/API/security specifications and agent handoff.
- **Not verified with Cargo/`dx` in generation environment; toolchain not available.**
- No production mail connectivity implemented.
