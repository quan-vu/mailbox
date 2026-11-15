# MailBox UI

This crate contains **all-Rust Dioxus UI**, compiled to WebAssembly for Web and to native Rust/system WebView for Desktop. No React or Electron.

Run from this directory:

```sh
# Install Rust and Dioxus CLI (see repository README)
dx serve --web
dx serve --desktop
```

Screens have real URLs through Dioxus Router: `/inbox`, `/messages/{id}/thread`, `/compose`, `/accounts/connect`, `/settings/{tab}`. Source layout: `src/app.rs` (shell), `src/routes.rs`, `src/components/`, `src/screens/`, `src/state/` (pure logic, unit-tested). Tokens are in `assets/tokens.css`, styles in `assets/main.css`, documented under `docs/03-ui/`.

Run with `dx serve -p mailbox-ui --web` or `--desktop` (the `-p` flag is required with dx 0.7.10).

**Safety:** The current UI only shows in-memory fixture email data. The compose "Gửi" button does not send; the connection form does not accept or store a password. Implement backend adapters before enabling these actions.
