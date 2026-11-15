# Privacy and storage policy (proposed)

MailBox deals with highly sensitive communications. User consent, local control and minimal collection are design requirements.

## Default data handling

- No telemetry of sender, recipients, subject, body, attachment or credentials.
- No cloud AI processing, machine-learning training or third-party email analytics.
- Full message bodies and attachment files are fetched only when needed unless user explicitly enables offline caching.
- If remote image loading is enabled for a message/sender, communicate that this may reveal user IP/device access to sender-operated hosts.
- Desktop metadata stored in OS app data, credentials in OS keychain; no plaintext secret in SQLite.
- Hosted Web metadata is user-scoped, encrypted credentials separate from DB key material and session bound to authenticated identity.

## Retention and deletion

- Disconnect account: revoke applicable tokens, wipe locally stored secret refs and cached metadata for that account, remove pending jobs after confirmation; **do not delete messages hosted by provider**.
- Delete local draft: wipe any local draft content and temporary attachment handles; account for backups/documented retention window.
- Export settings: exclude secrets unless explicit re-encryption and authenticated export.
- Backup: encrypt and protect; avoid copying credentials by default; do not include sample production email content in bug reports.

## Logging policy

Allowed: provider class (not address), numeric account ID scoped to opaque internal identifier, sync duration, error category, successful last-sync timestamp, retry count. Prohibited by default: email address, subject, message content, token, password, attachment filename, server error with echoed credentials.

## User preferences

- Remote content: blocked by default; allow per message or sender with clear confirmation.
- Offline full-body cache: off by default; size/retention limit, easily purged.
- Desktop notifications: show sender/subject only with explicit preview preference; safe default generic notification.
- Diagnostics export: metadata only, view/redact before export.

## Legal review gates

Before public deployment, prepare privacy notice, terms, data deletion/export flow, processor/subprocessor assessment where applicable, and data location policy. Do not claim GDPR/other statutory compliance solely because of this document.

## Current state of local storage (MB-121, Desktop)

- Password: OS keychain only (service `app.mailbox.client`, entry `imap.<account-id>`).
- SQLite database in the per-user app data directory: account settings, folder list, and for the newest 100 inbox messages the subject, sender name/address, date, size and read/star flags. **Stored unencrypted**; protected only by OS file permissions. No message bodies or attachments.
- Since MB-130: once a message is opened, its decoded text, sanitized HTML and attachment names/sizes are cached in the same database, also **unencrypted**. Attachment contents are never stored; saving one fetches the message again and writes a single file into the Downloads folder. Cached bodies are removed with their message (folder rebuild, account removal).
- Removing an account in Settings deletes these rows and the keychain entry. Nothing is deleted on the mail server.
- Open item: encryption at rest for the local database and an explicit cache-retention setting.

## What the Desktop app stores and sends (checked against the code, MB-304, 2026-11-15)

Audit: `docs/04-security/AUDIT_1.0.md`.

### Stored on this computer

| What | Where | Protection |
|---|---|---|
| IMAP and SMTP passwords | OS keychain, service `app.mailbox.client`, entries `imap.<account-id>` / `smtp.<account-id>` | Keychain only; never in SQLite, settings, logs or error messages |
| Account settings, folder list, envelopes (subject, sender, recipients, date, flags) | `mailbox.sqlite3` in the app data directory (macOS `~/Library/Application Support/app.mailbox.client`, Windows `%APPDATA%\MailBox`, Linux `$XDG_DATA_HOME/mailbox`) | Unencrypted; file mode 0600 (owner only) since MB-304, set on every open, also for journal/WAL/SHM files |
| Opened message bodies (text, sanitized HTML), attachment names and sizes | same database | as above; removed with the message or the account |
| Search index (folded subject, names, addresses, the start of opened bodies) | same database | as above |
| Drafts and send jobs (recipients, subject, body, local attachment paths) | same database | as above; a sent draft row stays until the user deletes it |
| Preferences: theme, signatures, templates, notification and shortcut choices | same database (`preferences`, `message_templates`) | no secrets; an account's signature is not yet removed with the account (AUDIT S-22) |
| Saved attachments | `~/Downloads` (or `MAILBOX_DOWNLOAD_DIR`) | written only on request, never overwritten, mode 0600, macOS quarantine flag |

Deleted rows are not overwritten inside the file (no `secure_delete`; AUDIT S-23).

### What leaves the computer

| When | Destination | What is sent |
|---|---|---|
| "Detect settings" in the connect wizard | System DNS resolver; plain UDP SRV queries to the resolvers in `/etc/resolv.conf` | Only the **domain** of the address: `_imaps._tcp.<domain>`, `_submissions._tcp.<domain>`, `_submission._tcp.<domain>`, and address lookups of candidate hosts |
| same | `https://autoconfig.<domain>/mail/config-v1.1.xml`, `https://<domain>/.well-known/autoconfig/mail/config-v1.1.xml` | Host, fixed path, `User-Agent: MailBox-autoconfig/1`; no query string, no `@`, no cookie; no third-party database (no ISPDB) |
| same | Candidate IMAP (993) / SMTP (465, 587) hosts | TLS ClientHello with SNI, or reading the STARTTLS greeting; no login |
| Connection check, sync, actions, send | The account's IMAP and SMTP servers | Login over verified TLS, mailbox commands, outgoing mail; SMTP `EHLO [127.0.0.1]` (no local host name); `Message-ID` `<random@sender-domain>` |
| "Show images" on one message | The image hosts named in that message | The WebView fetches the images: reveals IP address, WebView user agent and that the message was opened; no Referer. Plain `http:` images are allowed after the click (AUDIT S-15) |
| Clicking a link in a message | Nothing until the user confirms in a dialog that shows the real address; then the system browser or mail client (AUDIT S-09) | The link URL, after confirmation |
| New mail | OS notification service (stays on the computer, kept in the OS notification history) | By default "MailBox — Có N thư mới" (a count). Sender and subject only after the user chooses that option; never the body |

Nothing else: no telemetry, analytics, crash reports, update checks or third-party APIs. The
Web build talks only to the API origin fixed at build time (default `http://127.0.0.1:3000`),
which serves demo data.
