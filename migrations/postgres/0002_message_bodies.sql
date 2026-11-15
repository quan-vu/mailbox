-- MailBox schema v2 (PostgreSQL, Web): cached message bodies (MB-130, MB-131).
-- Holds only display-safe content: decoded text and HTML that already passed the allow-list
-- sanitizer. Attachment bytes are never stored; the attachments table keeps metadata only.
-- Portable types as in ADR 0005. Rows disappear with their message (folder reset, account removal).
CREATE TABLE IF NOT EXISTS message_bodies (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    body_text TEXT NOT NULL DEFAULT '',
    body_html TEXT,
    body_html_remote TEXT,
    remote_blocked BIGINT NOT NULL DEFAULT 0 CHECK(remote_blocked IN (0,1)),
    is_truncated BIGINT NOT NULL DEFAULT 0 CHECK(is_truncated IN (0,1)),
    fetched_at BIGINT NOT NULL
);
ALTER TABLE attachments ADD COLUMN IF NOT EXISTS content_id TEXT;
ALTER TABLE attachments ADD COLUMN IF NOT EXISTS is_inline BIGINT NOT NULL DEFAULT 0 CHECK(is_inline IN (0,1));
