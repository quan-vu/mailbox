# PRD — MailBox v0.1 → v1.0

**Status:** Draft approved direction for implementation; not a statement that the app is production ready.

## Problem statement

Users purchase valid email accounts with working IMAP/SMTP but poor provider webmail. MailBox will offer a modern web + desktop client without migrating messages away from the existing mail server.

## Goals

- Connect **3 independent IMAP/SMTP providers** in one personal MailBox installation.
- View account-specific and unified inbox without losing sender/folder identity.
- Read plain/HTML email safely; reply/forward using the correct identity.
- Keep local cache consistent with server flags and changes across reconnect/restart.
- Package Web service for self-hosting and Desktop app for macOS/Windows/Linux.

## Non-goals for MVP

- Hosting MX/SMTP infrastructure, delivering incoming email for others, spam-filtering gateway.
- AI writing assistant, calendar, contacts synchronization, shared/team mailbox delegation.
- Marketing/newsletter bulk send or mass automation.
- Offline-first conflict-free editing, cross-provider server-side move, multi-tenant SaaS.
- Exchange-specific EWS support unless separately approved.

## Scope by release

| Milestone | Includes | Excludes |
|---|---|---|
| UI starter | 5 responsive demo screens, tokens, sample DTOs, docs | Real accounts, send, DB |
| v0.1 MVP | One local user; manual IMAP/SMTP with secure storage, real inbox/read/reply/send, attachments, synced flags, 3-account tested | Cross-provider unified full-text search |
| v0.2 | Reconnect policy, queue robustness, search, OAuth major providers, better virtual folders | Multi-tenancy |
| v0.3 | Threads, templates, signatures, keyboard shortcuts, desktop OS notifications | AI |
| v1.0 | Production hardening, backups, observability, secure onboarding, release/signing | Provider-side mail hosting |

## Release acceptance gates

1. Manual connect to 3 separate test providers works over verified TLS with safe credentials.
2. Inbox/list/read/from/subject/attachment and pagination operate correctly on at least 1k messages per account.
3. Read/unread/star/delete/move reflect server state across separate clients and reconnects.
4. SMTP submission uses the explicitly selected account and handles ambiguous send results without blind retry.
5. No plaintext credential in database, logs, config export or support diagnostics.
6. Untrusted email HTML cannot execute script/access privileged desktop API or auto-load external tracking pixels.
7. Web deployment requires authentication, CSRF/session controls, rate limits and HTTPS; no anonymous mailbox API.
8. Desktop permissions remain least-privilege, filesystem confined to permitted paths, and update channel signed.
9. Functional accessibility checks and error states meet `RESPONSIVE_ACCESSIBILITY.md`.

## User-facing success metrics (proposed)

- Setup completion rate ≥ 85% for test-provider accounts; measure only with opt-in privacy-preserving test data.
- 95th percentile cached inbox opening under 500ms on a mid-range laptop (performance target, not verified).
- Zero incorrect account/sender sends in acceptance suite.
- No data loss or false 'sent' on outage fault-injection suite.

## Open decisions

- OAuth2 provider order by user demand and API access.
- Provider autoconfig source precedence and user override policy.
- How to handle mailbox quota reporting when server lacks QUOTA support.
- Whether Web v1.0 should be local-only single user or permit users over WAN with hardened auth.
