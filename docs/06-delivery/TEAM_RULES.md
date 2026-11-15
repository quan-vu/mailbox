# Rules for parallel work sessions

Read with `AGENTS.md` and `CLAUDE.md`. A supervising session reviews every diff, reruns every
gate and the real-provider tests on the merged tree, then merges. Report exactly what ran and
what did not.

## Product rules

- Rust/Dioxus architecture and the reference UI stay. Desktop first; mobile is deferred.
- Demo mode (fixtures) and Real mode coexist (`AppMode`). Demo is never presented as real.
- Build for real whatever can be built. Where an outside dependency blocks it, ship a mock that is
  labelled as a mock in the UI, sends nothing, stores no credential, contacts no server, and has a
  row in `LIMITATIONS_1.0.md` saying what is needed to make it real.
- Test first (RED then GREEN). Record the commands that ran and their real output.

## Security

- TLS is always validated; there is no bypass.
- Never log credentials, addresses, search terms or mail content.
- UIDs are valid only with their folder's UIDVALIDITY.
- Never read, print or copy values from any `.env*` file. Real-provider tests are opt-in
  (`#[ignore]`) and load the file inside the test process:
  `MAILBOX_SMTP_ENV_FILE=<repo>/.env.smtp MAILBOX_TEST_IMAP_HOST=<host in TEST_MAIL_PROVIDER.md> MAILBOX_SMTP_SEND_TO=self`
  (pattern: `crates/mailbox-store/tests/real_actions.rs`).
- Server mutations and sends only on messages the test itself created; sends only to the test
  mailbox itself; a handful per run. Never commit real addresses, mail content or database files.
- Secrets live in the OS keychain only, never in SQLite, settings or logs.

## Avoiding conflicts

- New code goes in new files and modules. Do not change existing signatures in `MailStore`; add
  a separate trait.
- Edits to shared files (`state/mod.rs`, `app.rs`, `adapters/mod.rs`, `adapters/account.rs`,
  `components/shell.rs`, `Cargo.toml` files, crate `lib.rs`) are small and additive.
- New CSS goes in the track's own asset file, not `main.css`.
- In `apps/mailbox-ui/src/main.rs` `ui_tests`: append only; helper names unique to the track.
- Each track gets a migration number in its brief (SQLite and PostgreSQL, portable types per
  ADR 0005).
- Do not edit `TRACKING.md` or `CHANGELOG.md`. Write evidence to
  `docs/06-delivery/evidence/track-<letter>-<name>.md`. `LIMITATIONS_1.0.md`: append or update rows.

## Git

- Commit on the track's own branch as you go (small commits, so work survives an interruption).
- Titles `MB-###: subject`. No agent or AI attribution of any kind. No push, no tags.

## Environment

- `export PATH="$HOME/.cargo/bin:$PATH"` first. `dx` needs `-p mailbox-ui`.
- Do not run `dx serve`, Playwright or `dx bundle`; do not touch `/Applications/MailBox.app`.
- `rsx!` component props cannot take `if` expressions: use `pick(cond, a, b)` from
  `components/icon.rs`.
- Build only what is needed; no release builds. Do not start Docker and never remove or prune
  containers that do not belong to this project.

## Done means

All of these pass, and the evidence file lists what was built, what is a mock, commands with real
results, what was not verified, and risks:

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p mailbox-ui --no-default-features --features desktop -- -D warnings
cargo clippy -p mailbox-ui --no-default-features --features web --target wasm32-unknown-unknown -- -D warnings
```

Final message: branch name, commit list, test counts, open risks, every shared file touched with a
one-line reason.
