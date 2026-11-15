# CLAUDE.md — Claude Code entrypoint

**Project**: MailBox, a beautiful, provider-agnostic IMAP/SMTP client in Rust on Web and Desktop.

Start by loading `AGENTS.md` (all instructions apply), then `docs/README.md`. This file adds the execution contract for Claude Code, without duplicating every spec.

## Working session contract

1. State selected `MB-###` task ID, expected files to change, and dependencies.
2. Identify whether current behavior is demo-only or production functional; preserve that distinction in labels and docs.
3. Create tests first for sensitive functionality and record executed verification commands.
4. Keep code changes bounded to one vertical slice; request architectural review if changing protocol/credential boundaries.
5. Update `docs/06-delivery/TRACKING.md`, including evidence and remaining gaps.
6. Give concise handoff: implemented, tested, not tested, security concerns, next task.

## Platform commands

```sh
cargo fmt --all -- --check
cargo test --workspace --exclude mailbox-ui
cargo check -p mailbox-ui --no-default-features --features web --target wasm32-unknown-unknown
cd apps/mailbox-ui && dx serve --web
# On compatible native host:
cd apps/mailbox-ui && dx serve --desktop
```

## Never do

- Rewrite this project as JS/TypeScript front end.
- Expose `mailbox-api` demo publicly; it currently has no auth.
- Persist credentials in UI, SQLite plain text, settings files or logs.
- Write an integration using arbitrary insecure IMAP TLS exceptions.
- Mark features DONE based solely on a screenshot or fixture.
- Commit `.env`, tokens, customer email or local database files.

## Source of truth precedence

1. Security rules in `AGENTS.md` and `docs/04-security`.
2. Architecture Decision Records (ADRs).
3. Feature acceptance criteria and API contracts.
4. UI state/interaction specs and reference screenshots.
5. Code examples. If code contradicts approved docs, create a task and resolve explicitly.

## Ready-to-run first task

Implement **MB-101**: install Rust tooling locally, check both Dioxus platform builds, fix any compiler/tool differences without changing architecture, and document actual test output in tracking. If Dioxus API changed since the starter was authored, pin an explicitly tested compatible patch and note the upgrade in `DEPENDENCIES.md`.
