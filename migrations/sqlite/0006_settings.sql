-- MailBox schema v6 (sqlite): message templates (MB-210). Settings themselves reuse the v1
-- `preferences` table: one row per owner with key `settings` holding a versioned JSON record.
-- Nothing secret belongs in either table. Portable types as in 0001 (ADR 0005).
CREATE TABLE IF NOT EXISTS message_templates (
    id TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    name TEXT NOT NULL,
    subject TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL DEFAULT '',
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(owner_id, id)
);
