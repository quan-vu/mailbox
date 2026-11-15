-- MailBox schema v3 (sqlite): local drafts and the outbox state machine (MB-133, MB-151).
-- Extends the v1 `drafts` table; `send_jobs` stays reserved for a later attempt log.
-- Portable types (ADR 0005). No credentials: only mail content the user wrote and file paths.
ALTER TABLE drafts ADD COLUMN message_id_header TEXT NOT NULL DEFAULT '';
ALTER TABLE drafts ADD COLUMN body_text TEXT NOT NULL DEFAULT '';
ALTER TABLE drafts ADD COLUMN body_html TEXT;
ALTER TABLE drafts ADD COLUMN in_reply_to TEXT;
ALTER TABLE drafts ADD COLUMN references_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE drafts ADD COLUMN attachments_json TEXT NOT NULL DEFAULT '[]';
-- draft | queued | sending | sent | failed | unknown. A row found as 'sending' at start-up was
-- interrupted mid-send and is turned into 'unknown' (never re-sent automatically).
ALTER TABLE drafts ADD COLUMN outbox_state TEXT NOT NULL DEFAULT 'draft';
ALTER TABLE drafts ADD COLUMN fail_reason TEXT;
ALTER TABLE drafts ADD COLUMN retryable INTEGER NOT NULL DEFAULT 0 CHECK(retryable IN (0,1));
-- none | pending | saved | already_there | failed: the copy in the server's Sent folder.
ALTER TABLE drafts ADD COLUMN sent_copy TEXT NOT NULL DEFAULT 'none';
CREATE INDEX IF NOT EXISTS idx_drafts_owner ON drafts(owner_id, updated_at DESC, id);
