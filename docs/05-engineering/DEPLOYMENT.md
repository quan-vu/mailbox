# Web and Desktop deployment plan

**The current code is a demo skeleton. Do not expose to the public Internet, treat Docker/desktop builds as production, or enter real credentials.**

## Web target topology

`HTTPS reverse proxy → Axum authenticated API → core/use cases → PostgreSQL + encrypted vault → IMAP/SMTP providers`; Dioxus WASM static assets served under same origin or hardened CDN policy. Browser never receives provider credentials or talks to IMAP sockets directly.

Required production capabilities: authenticated session, TLS termination, strict CSP and security headers, rate limiting, CSRF policy, database migrations, vault key management, egress SSRF protections, structured redacted logs, lifecycle backup/restore, liveness/readiness, least-privilege container user.

## Development commands

```sh
# From repo root
cargo run -p mailbox-api          # demo API localhost:3000, read-only
cd apps/mailbox-ui
dx serve --web                    # UI runs from fixture directly
```

**Note**: UI is not yet wired to the Axum API. MB-103 adds a typed `MailboxDataSource` and browser-safe HTTPS adapter. Keep browser auth/session rules in the API, not in RSX components.

## Desktop target topology

`Dioxus native Rust + system WebView → native core/use cases → SQLite + OS keychain → IMAP/SMTP providers`. No embedded open localhost server or unsafe native IPC exposed to untrusted mail HTML by default.

```sh
cd apps/mailbox-ui
dx serve --desktop
# After verifying bundle config and code signing:
dx bundle --desktop
```

Signing, auto-update, sandbox entitlement, OS notifications, native filesystem access and Linux WebKitGTK packaging require explicit platform QA.

## Suggested production environment variables (future, currently unused)

- `MAILBOX_PUBLIC_BASE_URL`
- `MAILBOX_DATABASE_URL`
- `MAILBOX_ENCRYPTION_KEY_ID` (not secret material in logs)
- `MAILBOX_BIND`
- `MAILBOX_LOG_LEVEL`
- `MAILBOX_OUTBOUND_ALLOWED_PORTS`
- `MAILBOX_MAX_ACCOUNTS_PER_USER`
- `MAILBOX_CACHE_MAX_BYTES`

## Backup and rollback

Perform backup before migrations. Restore tested in isolated environment; align DB schema version and application binary. Credentials require separately documented key recovery/re-enrollment; never assume DB-only backup recovers secrets.

## Observability

Readiness should report DB/migration state, ability to accept jobs and secret vault connectivity (without exposing sensitive config). Per-account IMAP outage should degrade account status but not necessarily fail global liveness.
