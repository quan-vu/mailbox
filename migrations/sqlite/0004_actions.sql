-- MailBox schema v4 (sqlite): folder roles (MB-122) and pending server operations (MB-132, MB-144).
-- Portable types as in 0001 (ADR 0005): opaque TEXT ids, Unix seconds, 0/1 flags.
-- A folder's role is data, not code: detected from the server ('attribute', 'name') or chosen by
-- the user ('user'). A NULL role means the folder has none.
CREATE TABLE IF NOT EXISTS folder_roles (
    folder_id TEXT PRIMARY KEY REFERENCES folders(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    role TEXT CHECK(role IS NULL OR role IN ('inbox','sent','drafts','trash','archive','junk')),
    source TEXT NOT NULL CHECK(source IN ('attribute','name','user'))
);
CREATE INDEX IF NOT EXISTS idx_folder_roles_account ON folder_roles(account_id);
-- Changes made locally that the mail server has not confirmed yet. Each row keeps the full IMAP
-- identity it was queued under (folder, UIDVALIDITY, UID). Rows go away with their message, so a
-- folder reset after a UIDVALIDITY change can never leave an operation pointing at a stale UID.
CREATE TABLE IF NOT EXISTS pending_ops (
    id TEXT PRIMARY KEY,
    seq INTEGER NOT NULL,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    folder_id TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uidvalidity INTEGER NOT NULL,
    imap_uid INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('seen','flagged','move','expunge')),
    flag_value INTEGER CHECK(flag_value IS NULL OR flag_value IN (0,1)),
    previous_value INTEGER CHECK(previous_value IS NULL OR previous_value IN (0,1)),
    dest_folder_id TEXT REFERENCES folders(id) ON DELETE CASCADE,
    state TEXT NOT NULL DEFAULT 'queued' CHECK(state IN ('queued','failed')),
    error_code TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_pending_ops_account ON pending_ops(account_id, seq);
