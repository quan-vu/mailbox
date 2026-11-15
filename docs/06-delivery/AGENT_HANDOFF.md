# Agent handoff — start from this repository

## Ready-to-paste Claude Code / Codex first prompt

> You are the lead engineer of MailBox, an open-source Rust Web + Desktop email client. Read `AGENTS.md`, `CLAUDE.md`, `README.md`, `docs/README.md` and accepted ADRs. Follow `docs/06-delivery/TRACKING.md` using task IDs. Start with MB-101: verify and fix Dioxus 0.7 Web/Desktop builds and make the five existing UI screens work. Do NOT change the all-Rust Dioxus architecture, do NOT connect real credentials or enable SMTP send in demo, and do NOT replace UI screenshots with unrelated design. Run and record actual compile/tests. Then perform MB-102 (component extraction and responsive visual fidelity) and MB-103 (typed cross-platform data source). Update tracker and changelog after each task, and report what remains unverified. Implementation should be small, clean, tested, secure and documented.

## Session 1 acceptance checklist

- [ ] Rust stable, Wasm target, compatible `dx` installed; versions recorded.
- [ ] `cargo fmt --all -- --check` succeeds.
- [ ] `cargo test --workspace --exclude mailbox-ui` succeeds.
- [ ] `cargo check -p mailbox-ui --no-default-features --features web --target wasm32-unknown-unknown` succeeds.
- [ ] `dx serve --web` loads the five navigation screens.
- [ ] `dx serve --desktop` loads on at least one native target.
- [ ] Screens visually compared against `docs/03-ui/references/*.png`.
- [ ] `Cargo.lock` committed, actual tested version documented.
- [ ] No real password accepted, no real SMTP send possible, no public unauthenticated route.
- [ ] Tracker updated with command logs and blockers.

## Session 2 target: MB-102

Extract `app`, `components/` (Header, Sidebar, Button, StatusBadge, MessageRow, AttachmentCard), `screens/` (inbox, connect, thread, compose, settings), `state/` (navigation, account state, mail view state), `theme/`. Centralize labels/tokens and replace Unicode icon placeholders with a reviewed SVG set. Implement correct one-pane mobile reading UX. Add screenshot regression and keyboard tests.

## Session 3 target: MB-103

Introduce `trait MailboxDataSource` or equivalent to decouple Dioxus from in-process fixtures. Browser adapter calls authenticated Axum `/api/v1` (develop with safe demo-only route), Desktop adapter directly calls native Rust use cases. Keep auth/secret boundaries intact. Never leak passwords/tokens to browser WASM bundle.

## Reviewer questions

1. Does each new UI state map to a spec ID and screen reference?
2. Is any mock data passed off as a working real provider?
3. Is there a test for UIDVALIDITY, SMTP uncertain outcome or mail HTML isolation where relevant?
4. Are credentials, email metadata and attachment handling explicitly privacy-reviewed?
5. Is Web and Desktop behavior covered and recorded rather than assumed?

## Final handoff format for every agent run

```text
TASK: MB-###
FILES: ...
IMPLEMENTED: ...
TESTED: commands + summary
NOT VERIFIED: ...
SECURITY: ...
DOCS UPDATED: ...
NEXT: MB-###
```
