# Engineering quality gates / test strategy

## Source standards

- Rust Edition 2024; `cargo fmt`, Clippy, warnings tracked.
- Test every use case independently from UI; avoid coupling core to network adapters.
- Keep SQL migrations versioned and independently tested on SQLite and PostgreSQL.
- Enforce DTO backward compatibility for `/api/v1`; avoid silent response shape changes.
- Add `Cargo.lock` after first successful toolchain resolution.

## Where checks run (owner decision, 2026-11-14)

macOS and Linux are built and verified **on the local machine** to keep CI cost down. GitHub Actions is used **for the Windows release build only**, and only when someone starts the workflow by hand and names the version tag to build (`gh workflow run ci.yml -f tag=v1.0.1`). Pushing a commit, a tag or a pull request starts nothing. The compiler is pinned to the same version in `rust-toolchain.toml` and in the workflow.

| Check | Where | Command |
|---|---|---|
| format | local | `cargo fmt --all -- --check` |
| unit | local | `cargo test --workspace --exclude mailbox-ui` |
| UI unit/ssr | local | `cargo test -p mailbox-ui` |
| lint | local | `cargo clippy` for workspace, UI desktop and UI web (wasm32), `-D warnings` |
| Web E2E | local | start `dx serve -p mailbox-ui --web --port 8080`, then `cd e2e && pnpm exec playwright test --workers=1` |
| macOS desktop bundle | local | `dx bundle -p mailbox-ui --desktop --release --package-types macos` |
| Linux desktop bundle | local (Linux host/VM) | not yet run |
| Windows tests + desktop bundle | GitHub Actions, by hand for a tag | `windows-desktop` job; two runs failed (test cleanup, new compiler warnings), both fixed; no passing run yet |
| imap/smtp integration, security, performance | future | — |

## Acceptance test suites

**Unit**: UIDVALIDITY transitions, sender identity rules, email validation, quota display, MIME part routing, duplicate-send state.

**Integration**: TLS cert errors, invalid auth, IDLE/no-IDLE provider, missing QUOTA, message deleted elsewhere, server disconnect while syncing, duplicate Message-ID.

**UI**: all five screens in Light/Dark; enter/select/focus; responsive mobile; empty/loading/error/reauth; after account select From remains clear. Screen references under `docs/03-ui/references/`.

**Security**: HTML contains JS/event handler/remote pixel; ZIP bombs/huge MIME; path traversal filename; DNS rebinding/loopback/cloud metadata; session account spoofing; send outcome unknown.

## Release blocker criteria

- Real secret visible in log or plaintext DB.
- Email HTML can escape isolation or access desktop native APIs.
- TLS bypass/downgrade allowed by default.
- Sending wrong From identity or automatic duplicate SMTP send.
- Account data visible across authenticated users.
- Sync cursor ahead of persisted data after a crash.

## Performance budgets (targets, not measured)

- Cached inbox initial interaction P95 < 500ms on development benchmark machine.
- UI scrolling stable for 1000 visible summary items (virtualize at scale).
- Bounded process memory while syncing massive folder/attachments, no unbounded fetch.
- Low connection fanout independent of browser tab count.

## Evidence format in task tracker

`MB-123 | DONE | command: cargo test -p mailbox-core | result: 18 passed | commit: <hash> | reviewer: <name>`.

If commands were not run, write `NOT VERIFIED` and leave the task not-DONE.
