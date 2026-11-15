# Data design — local metadata & sync cursors

See reference schema files `migrations/sqlite/0001_init.sql` and `migrations/postgres/0001_init.sql`. Applied by `crates/mailbox-store` (MB-120); the apps do not use the store yet. Each dialect has its own migration directory and both run the same contract tests. Column types are deliberately portable — see ADR 0005.

## Entity inventory

| Entity | Key | Purpose | Retention |
|---|---|---|---|
| `accounts` | `id` + `owner_id` | Provider endpoints, auth scheme, **secret reference only** | Until detached |
| `folders` | `id` + `account_id` | IMAP name, delimiter, special-use flags, UIDVALIDITY | Reconciled |
| `messages` | `id` | Metadata, flags, preview, account/folder/UID identity | Configurable cache |
| `attachments` | `id` | MIME part metadata, not raw binary by default | Associated with message |
| `sync_states` | `account_id,folder_id` | Last known UID epoch, cursor, sync status | Until account detached |
| `drafts` | `id` | User-authored local draft, sending identity | Explicit delete |
| `send_jobs` | `id` | SMTP submission status and ambiguity audit | Retention policy |
| `preferences` | `owner_id,key` | UI settings | Until deletion |

## Uniqueness and constraints

- Unique `(account_id, folder_id, uidvalidity, uid)` for cached messages.
- `account_id` must be ownership-scoped on Web; **every** read/write must verify owner belongs to authenticated principal.
- Never use bare UID as globally unique key; never rely on `Message-ID` uniqueness.
- `from_identity` must reference a configured account or verified alias on submit.
- `credential_ref` points to OS keychain or encrypted vault; cannot hold plaintext password.

## Cache content policy

Store envelope and preview only for initial sync. Full body cache is opt-in, encrypted at rest if enabled, size-limited and purged on account deletion. Attachment bytes are transient downloads, stored only when explicitly saved. No automatic indexing of sensitive body text without a documented consent option.

## Web vs Desktop differences

- **Web** PostgreSQL: owner IDs mandatory; secret vault under separate trust boundary, encryption key cannot be stored alongside encrypted records in same DB.
- **Desktop** SQLite: single OS user, app-data directory, restrictive file permissions; local DB contains no credentials, OS keychain references only.
- Migrations are additive and tested with backup/restore; index and maintenance plan review before large-scale sync.

## Future migration acceptance

Run fresh install, upgrade, rollback strategy documentation, unique constraint conflict tests, UID epoch rebuild, account delete cascade and restart after partial failure. Test with 100k metadata rows. No production migration should erase credentials or email content without explicit opt-in/backup.
