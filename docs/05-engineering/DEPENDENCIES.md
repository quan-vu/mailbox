# Dependency decision register

**Date checked:** 2026-11-14. `Cargo.lock` committed in MB-101.

### Tested toolchain (MB-101, macOS aarch64)

| Tool | Version | Note |
|---|---|---|
| rustc / cargo | 1.100.0, pinned in `rust-toolchain.toml` and in the Windows workflow (1.97.0 until 2026-11-15) | Put `~/.cargo/bin` before Homebrew in PATH; Homebrew cargo ignores `rust-toolchain.toml` and lacks the wasm target |
| wasm32-unknown-unknown | same toolchain | `rustup target add wasm32-unknown-unknown` |
| dioxus / dioxus-cli (`dx`) | 0.7.10 / 0.7.10 | Run `dx serve -p mailbox-ui ...` (dx 0.7.10 panics on workspace `default-members` otherwise) |
| wasm-bindgen (auto-installed by dx) | 0.2.130 | |
| wry (desktop WebView) | 0.53.5 | macOS WKWebView |
| axum / tokio | 0.8.9 / 1.x | demo API |
| dioxus-ssr | 0.7.10 | dev-dependency, renderer-independent UI tests |
| sqlx | 0.8 (sqlite, postgres, migrate, macros; tokio runtime) | `mailbox-store` only; not compiled into the Web bundle |
| tokio-rustls 0.26 (ring) + webpki-roots 1 | `mailbox-net`: verified TLS for IMAP/SMTP | bundled Mozilla roots; private/corporate CAs are not trusted |
| keyring | 3 (apple-native, windows-native, sync-secret-service) | `mailbox-secrets`; tested on macOS only |
| gloo-net | 0.6 | Web data source only (wasm32) |
| mail-parser | =0.11.9 (`full_encoding`) | `mailbox-mime` only, native. MIME/charset decoding (Apache-2.0 OR MIT). Pinned exactly: 1.0.x needs Rust 1.98, this machine has 1.97. Our own limits (size, depth, part count) sit on top; re-run `cargo test -p mailbox-mime` on any upgrade |
| ammonia | =4.2.5 | `mailbox-mime` only, native. Allow-list HTML sanitizer on html5ever (MIT OR Apache-2.0). Pinned exactly; every URL decision is made by our attribute filter. Re-run the hostile corpus (`tests/sanitize.rs`) on any upgrade |
| tower-http | 0.6 (cors) | demo API |
| @playwright/test | 1.64.1 | `e2e/` only, test tooling, never shipped in the app bundle |
| mailbox-compose (in-repo) | workspace | Address rules and RFC 5322/MIME builder (MB-150). No dependencies, no I/O; compiled into both Web and Desktop |
| rfd (through dioxus-desktop 0.7.10) | 0.16 / 0.17 as locked | Native file dialog for the composer's attachment picker: a plain `<input type="file">` that Dioxus Desktop opens with its own `rfd`. No direct dependency in this repo; not verified by click-through yet |
| tokio (in `mailbox-ui`, native only) | 1.x (rt, time) | Autosave debounce and reading attachments off the UI thread. Never compiled into the Web bundle |
| notify-rust | =4.19.0 | `mailbox-ui`, native only (MB-211). OS notifications for new mail (MIT OR Apache-2.0); on macOS it uses `mac-notification-sys`, on Windows WinRT toasts, on Linux D-Bus. Pinned exactly. The text shown is decided by `mailbox_core::notify` (count only by default; never the body). An unsigned or unbundled macOS build may be refused by the system: reported in `LIMITATIONS_1.0.md` |
| webbrowser | =1.2.7 | `mailbox-ui`, native only (AUDIT S-09). Opens a link from mail in the system browser or mail program, only after the user confirmed the address. Same version dioxus-desktop 0.7.10 already depends on; no new crate in the lock file |
| url | 2 (lock: 2.5.8) | `mailbox-core` (AUDIT S-09, `link.rs`). Parses the address of a link from mail to show its real host and refuse deceptive forms. Already in the lock file through ammonia; no new crate |
| mailbox-autoconfig (in-repo) | workspace | MB-202: provider table, DNS SRV (own wire parser, UDP to the system name servers), Thunderbird autoconfig XML (own reader that refuses any DTD/entity), verified guesses. Only `mailbox-core` for everyone; `mailbox-net` + `tokio` (net, io-util, time) on native targets. No HTTP client crate on purpose: a hand-written GET over `mailbox-net`'s validated TLS, to the address that the SSRF guard checked, with no redirects and hard size and time caps |
| serde (in `mailbox-core`) | 1.x | Typed, versioned preferences record (MB-210). No new transitive crates: `serde_json` was already used by `mailbox-store` |
| serde_json (in `mailbox-store`) | 1.x | Recipient, reference and attachment lists of a draft as JSON text, so SQLite and PostgreSQL share one query set |

| Library/tool | Intended role | Status |
|---|---|---|
| Rust stable, Edition 2024 | All app logic | Workspace configured; compile not verified |
| Dioxus `0.7.10` | Shared Rust Web/Desktop UI | Current code scaffold; verify Web/Desktop compile |
| Dioxus CLI `0.7.10` | Build/serve/bundle | Install/test per platform |
| Axum `0.8` | Server HTTP API | Demo route only |
| Tokio `1.x` | Async runtime | Demo API today, workers planned |
| Serde `1.x` | Shared DTO serialization | Fixture contracts |
| `async-imap` `0.12` | Potential IMAP adapter | Candidate for sync (MB-121+). The MB-111 connection check uses a small in-house dialogue instead (greeting, CAPABILITY, LOGIN, EXAMINE). |
| `lettre` or equivalent | Potential SMTP transport | Not adopted. MB-150/151 extend the in-house dialogue (`mailbox-smtp::submit`) and build messages in `mailbox-compose`, to keep the uncertain-outcome handling explicit and testable. |
| `mail-parser` `=0.11.9` + `ammonia` `=4.2.5` | MIME parsing, HTML sanitising | In use since MB-130/131 (`crates/mailbox-mime`). Limits and hostile corpus are unit-tested; no coverage-guided fuzzing yet |
| SQLx or equivalent | SQLite/PostgreSQL adapters | Candidate; no DB runtime yet |
| `keyring` or OS APIs | Desktop credential vault | Candidate; test macOS/Win/Linux actual permissions |
| AEAD crate audited (`aes-gcm`/`chacha20poly1305`) | Web secret encryption | Candidate; requires key management & rotation design |

### Source documentation

- Dioxus platform support: https://dioxuslabs.com/learn/0.7/guides/platforms/
- Dioxus Desktop: https://dioxuslabs.com/learn/0.7/guides/platforms/desktop/
- Dioxus CLI: https://dioxuslabs.com/learn/0.7/guides/tools/
- Axum: https://docs.rs/axum/latest/axum/
- Async IMAP: https://docs.rs/async-imap/latest/async_imap/
- Rust: https://doc.rust-lang.org/book/
- Tauri 2 + Leptos alternative: https://v2.tauri.app/start/frontend/leptos/
- Similar open-source email products for feature benchmarking: https://github.com/cypht-org/cypht , https://github.com/the-djmaze/snappymail , https://github.com/roundcube/roundcubemail

### Before dependency merge

1. Verify active maintenance and CVEs (RustSec/cargo-audit).
2. Verify actual crate license and transitive dependency compatibility with Apache-2.0 repository.
3. Check target support: WASM vs Desktop native; secret libraries must never compile into browser bundle.
4. Pin and commit `Cargo.lock` for binaries, record tested toolchain and CI matrix.
5. Compare protocol library behavior with IMAP server capabilities; don't assume all extensions are supported.

**License note**: This repo uses proposed Apache-2.0, but dependencies may carry their own licenses and obligations. No code from MailFlow, Cypht, SnappyMail or other projects is copied into this starter.
