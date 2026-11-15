# Contributing

Thanks for looking at MailBox. The project is Rust end to end (Dioxus for Web and Desktop);
please do not propose a JavaScript/TypeScript front end.

## Before you start

- Read `AGENTS.md` (rules that apply to every contributor, human or automated) and
  `docs/README.md`.
- Work is tracked by `MB-###` task IDs in `docs/06-delivery/TRACKING.md`. Open an issue first
  for anything larger than a small fix.
- Report vulnerabilities privately; see `SECURITY.md`.

## Local checks

`rustup` installs the pinned toolchain from `rust-toolchain.toml` on first use.

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --exclude mailbox-ui --all-targets -- -D warnings
cargo clippy -p mailbox-ui --no-default-features --features desktop --all-targets -- -D warnings
cargo clippy -p mailbox-ui --no-default-features --features web --target wasm32-unknown-unknown -- -D warnings
```

Web E2E (Playwright, test tooling only): `cd e2e && pnpm install && pnpm exec playwright test --workers=1`.

There is no automatic CI on pushes or pull requests. Run the checks above and paste the result
in the pull request. The Windows release build runs on GitHub Actions only when a maintainer
starts it for a version tag.

## Rules that matter most

- Never commit `.env` files, tokens, passwords, real mail or database files.
- Never log credentials, tokens, addresses or message content.
- TLS stays fail-closed with validated certificates; no bypass options.
- Demo behaviour must stay labelled as demo. Do not mark a feature done from a screenshot or a
  fixture.
- Tests first for anything touching credentials, protocols, MIME or sending.
- Commit titles: `MB-###: subject`.
