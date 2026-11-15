# AGENTS.md — Required operational rules for Codex, Claude Code and other agents

You are working in the **MailBox** Rust monorepo. Read the following before changing code:

1. `README.md` — factual project status, development commands.
2. `docs/README.md` — source-of-truth document index.
3. `docs/00-product/PRD.md` — problem and strict non-goals.
4. `docs/02-architecture/SYSTEM_DESIGN.md` and `docs/02-architecture/MAIL_SYNC.md`.
5. `docs/03-ui/DESIGN_SYSTEM.md`, `SCREENS.md` and reference PNGs.
6. `docs/06-delivery/TRACKING.md` — numbered tasks, dependencies, acceptance tests.

## Non-negotiables

- **Rust-first/full Rust**: do not replace Dioxus UI with React/Vue/Electron. Web: Rust/WASM; Desktop: native Rust + WebView. Ordinary CSS is fine. Ask before adding JS runtime dependency.
- **Provider-neutral**: no hard dependency on one email vendor. Protocol adapters behind core ports.
- **Do not build a mail hosting/server stack**: use existing IMAP/SMTP accounts.
- **Never claim a demo connection or send is real.** Don't silently enable real account credentials in mock forms.
- **Never log credentials, bearer tokens, raw email bodies, attachments or email addresses in telemetry by default.** Scrub errors and tracing spans.
- **Fail closed on TLS**, validate certificates, prevent SSRF and DNS rebinding, block metadata services and private IP egress for untrusted hosts in multi-tenant Web mode.
- **Safe sending**: explicit sender identity, SMTP result uncertainty handling, idempotency strategy. No blind re-send on network timeout.
- **IMAP correctness**: UIDVALIDITY scoped per folder/account, capability detection, conflict resolution and expunge/move behavior as in `MAIL_SYNC.md`.
- **MIME safety**: sanitize HTML, isolate rendered email, no auto-load of remote resources, limit attachment size and MIME recursion.
- **UI consistency**: work from five provided image references. Use token system; never invent additional primary colors or unrelated visual systems.
- **Small, reviewable changes**: one numbered task or closely related group per PR; never dump unfinished feature stubs as `DONE`.

## Required task workflow

1. Choose a task ID from `docs/06-delivery/TRACKING.md`; check `Depends on` is complete.
2. Read affected requirement(s) in `FEATURES.md`, screen behavior and ADRs.
3. Define observable acceptance tests *before* implementation.
4. Implement core behavior first, then adapter and UI; add tests at correct layer.
5. Run `cargo fmt --all -- --check`, `cargo test --workspace --exclude mailbox-ui`, `cargo clippy` (available targets) and Dioxus Web/Desktop checks.
6. Update `docs/06-delivery/TRACKING.md` with status, evidence and blockers. Update `CHANGELOG.md` and architecture docs when behavior changes.
7. PR/commit title: `MB-###: concise subject` (no AI-agent attribution in commits; disclose external code or generated assets as required by license).

## Repository conventions

- `apps/mailbox-ui`: UI only; no raw socket access, credentials or SQL in RSX components.
- `apps/mailbox-api`: HTTP auth, validation, rate limits, authorization and routing; domain belongs in `crates/mailbox-core`.
- `crates/mailbox-contracts`: DTOs, API versioning, validation model; no secrets serialized.
- `crates/mailbox-core`: use cases/ports/domain errors; no UI assumptions.
- `migrations/*`: versions immutable after release; apply explicit test migrations.
- `docs/02-architecture/adr/*`: new decisions require new ADRs; never silently rewrite accepted decisions.

## Definition of Done

A task is DONE only with implementation + automated tests + tested execution command/output + updated docs + security/privacy review + appropriate UI states. If toolchain is unavailable, status remains `IN PROGRESS` or `BLOCKED`, not DONE.

## First sequence for a new agent

**MB-101 → MB-102 → MB-103 → MB-110 → MB-111 → MB-120 → MB-121 → MB-130 → MB-140 → MB-150.** This is a dependency-aware recommended path, not permission to skip security gates.
