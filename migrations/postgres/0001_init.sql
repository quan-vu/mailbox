-- MailBox schema v1 (PostgreSQL, Web). Applied by mailbox-store (MB-120).
-- Web deployments also need authentication and per-owner authorization (MB-110) before use.
-- No plaintext secrets: credential_ref points into the OS keychain or the encrypted vault.
-- Portable types on purpose (see ADR 0005): ids are opaque TEXT, times are Unix seconds (UTC),
-- flags are 0/1 integers, so one repository implementation serves both dialects.
CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    email_address TEXT NOT NULL,
    imap_host TEXT NOT NULL,
    imap_port BIGINT NOT NULL CHECK(imap_port BETWEEN 1 AND 65535),
    smtp_host TEXT NOT NULL,
    smtp_port BIGINT NOT NULL CHECK(smtp_port BETWEEN 1 AND 65535),
    auth_type TEXT NOT NULL,
    credential_ref TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'disabled',
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_accounts_owner ON accounts(owner_id);
CREATE TABLE IF NOT EXISTS folders (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    remote_name TEXT NOT NULL,
    delimiter TEXT,
    special_use TEXT,
    uidvalidity BIGINT,
    uidnext BIGINT,
    UNIQUE(account_id, remote_name)
);
CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    folder_id TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uidvalidity BIGINT NOT NULL,
    imap_uid BIGINT NOT NULL,
    message_id_header TEXT,
    subject TEXT NOT NULL DEFAULT '',
    from_display TEXT NOT NULL DEFAULT '',
    from_address TEXT NOT NULL DEFAULT '',
    preview TEXT NOT NULL DEFAULT '',
    internal_date BIGINT NOT NULL DEFAULT 0,
    size_bytes BIGINT NOT NULL DEFAULT 0,
    is_seen BIGINT NOT NULL DEFAULT 0 CHECK(is_seen IN (0,1)),
    is_flagged BIGINT NOT NULL DEFAULT 0 CHECK(is_flagged IN (0,1)),
    cached_body_ref TEXT,
    UNIQUE(account_id, folder_id, uidvalidity, imap_uid)
);
CREATE INDEX IF NOT EXISTS idx_messages_order ON messages(account_id, folder_id, internal_date DESC, id);
CREATE TABLE IF NOT EXISTS attachments (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    mime_part TEXT NOT NULL,
    display_name TEXT,
    media_type TEXT,
    size_bytes BIGINT,
    UNIQUE(message_id, mime_part)
);
CREATE TABLE IF NOT EXISTS sync_states (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    folder_id TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uidvalidity BIGINT,
    last_uid BIGINT NOT NULL DEFAULT 0,
    state TEXT NOT NULL DEFAULT 'not_started',
    last_success_at BIGINT,
    last_error_code TEXT,
    PRIMARY KEY(account_id, folder_id)
);
CREATE TABLE IF NOT EXISTS drafts (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    version BIGINT NOT NULL DEFAULT 1,
    from_identity TEXT NOT NULL,
    recipient_json TEXT NOT NULL,
    subject TEXT NOT NULL DEFAULT '',
    content_ref TEXT,
    updated_at BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS send_jobs (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL,
    draft_id TEXT NOT NULL REFERENCES drafts(id) ON DELETE CASCADE,
    submission_state TEXT NOT NULL,
    payload_digest TEXT,
    smtp_response_class TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS preferences (
    owner_id TEXT NOT NULL,
    setting_key TEXT NOT NULL,
    setting_value_json TEXT NOT NULL,
    PRIMARY KEY(owner_id, setting_key)
);
