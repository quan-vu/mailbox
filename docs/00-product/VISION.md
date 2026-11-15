# Product Vision — MailBox

## The problem

Small businesses and developers often buy SMTP/IMAP inboxes from hosting providers whose webmail is hard to navigate. Switching vendors just for better UX adds operational cost. Users need a **beautiful overlay on existing email accounts**, not another mail server.

## Value proposition

> A Gmail-quality inbox for email accounts you already own — self-hosted, transparent, provider-neutral and built in Rust for the browser and desktop.

## Audiences

1. **Individual with 2–10 hosted mailboxes**: simple setup, account switching, unread overview.
2. **Small business operator**: branded aliases, reply from correct account, attachments, shared workstation.
3. **Self-hosting developer**: Docker, observable sync, secure credentials, reproducible upgrades, source code audit.
4. **Privacy-conscious team** (later): authorization and multi-user separation, controlled storage retention.

## Differentiation hypotheses to validate

- Hosted IMAP onboarding is more dependable and comprehensible than generic webmail.
- Unified Inbox feels instant and keeps per-account context visible.
- Sender identity defaults correctly and protects against accidental cross-account replies.
- One Rust UI source targets Web and Desktop while core business rules are shared.
- Self-hosting uses one documented install path without requiring a mail-server stack.

## Alternatives worth testing

Before committing features, compare the user journey against MailFlow, Cypht, SnappyMail and Roundcube. Avoid copying brand/asset/code without reviewing third-party licenses. Prior research links are indexed in `DEPENDENCIES.md`.

## Product principles

- Clear > feature-heavy; familiar email concepts > proprietary jargon.
- Original mail server is source of truth.
- No customer content or credentials used for training/analytics.
- Privacy and secure defaults over convenience shortcuts.
- UI must visibly distinguish demo, syncing, offline, stale, failed and sent states.

## Questions for actual users

Can they add a cPanel/Zoho/custom mail account in under 3 minutes without knowing IMAP jargon? Can they recover from an expired app password without support? Will they trust sending from the correct alias? What matters more: fast search, unified unread count, or desktop notifications?
