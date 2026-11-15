-- MailBox schema v5 (postgres): reply headers, conversations and the local search index
-- (MB-200, MB-201). Portable types as in 0001 (ADR 0005). Every row goes away with its message
-- or account (ON DELETE CASCADE), so a folder reset after a UIDVALIDITY change or an account
-- removal leaves nothing behind. No credentials; this is mail metadata the user already has.
--
-- Threading and recipient headers, fetched with the envelope (or backfilled lazily for rows
-- cached before this version). `references_list` holds message ids separated by spaces.
CREATE TABLE IF NOT EXISTS message_headers (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    in_reply_to TEXT,
    references_list TEXT NOT NULL DEFAULT '',
    to_line TEXT NOT NULL DEFAULT '',
    cc_line TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_message_headers_account ON message_headers(account_id);
-- Conversation of each message, computed per account from the headers above.
CREATE TABLE IF NOT EXISTS message_threads (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_message_threads_thread ON message_threads(account_id, thread_id);
-- Folded (lower case, no diacritics) text for local search: subject, sender, recipients, body.
-- `body_at` is the indexed body's fetched_at, so a newly downloaded body is indexed again. A
-- message whose headers are rewritten loses its row and is indexed again.
-- PostgreSQL searches these columns with ILIKE (no full-text index yet; see the search ADR).
CREATE TABLE IF NOT EXISTS message_search (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    meta TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL DEFAULT '',
    body_at BIGINT NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_message_search_account ON message_search(account_id);
CREATE INDEX IF NOT EXISTS idx_message_search_body ON message_search(body_at);
-- Newest-first lists that span folders (unified inbox, role views, search results).
CREATE INDEX IF NOT EXISTS idx_messages_date ON messages(internal_date DESC, id);
