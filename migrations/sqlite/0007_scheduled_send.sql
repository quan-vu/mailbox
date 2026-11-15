-- MailBox schema v7 (sqlite): scheduled send (MB-218). Portable types (ADR 0005).
-- Send time while outbox_state = 'scheduled', in seconds since the Unix epoch (UTC). Kept after
-- the send so the outbox can name a missed time; cleared when the message is a draft again.
ALTER TABLE drafts ADD COLUMN scheduled_at INTEGER;
CREATE INDEX IF NOT EXISTS idx_drafts_scheduled ON drafts(owner_id, outbox_state, scheduled_at);
