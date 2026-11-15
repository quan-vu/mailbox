# Security policy

MailBox handles mail account credentials and message content, so security reports are welcome.

## Reporting a vulnerability

Please **do not open a public issue**. Use GitHub's private reporting instead: the repository's
**Security** tab, then **Report a vulnerability**. Include the version or commit, the platform,
and steps to reproduce. Do not include real passwords, tokens or other people's mail.

There is no paid bounty. This is a small project; expect a first answer within about a week.

## Supported versions

Only the latest released version gets fixes.

## What to know before relying on MailBox

- The Desktop app is the supported product. The Web build and `mailbox-api` are a **demo
  without authentication**; do not expose them to a network.
- Desktop 1.0 passwords are kept in the operating system keychain. The local database (cached
  mail) is **not encrypted at rest**.
- The macOS build is unsigned. Windows and Linux builds are not verified.
- Known open findings and limits are public: `docs/04-security/AUDIT_1.0.md`,
  `docs/06-delivery/LIMITATIONS_1.0.md`, `docs/04-security/THREAT_MODEL.md`.
