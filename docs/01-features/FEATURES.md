# Complete feature catalog & acceptance criteria

Legend: **P0** MVP must, **P1** next, **P2** later. For screen mapping see `SCREENS.md`. IDs are stable spec identifiers; implementation tasks use `MB-###`.

| ID | Feature | Priority | Acceptance criteria / boundary |
|---|---|---|---|
| ACC-01 | Add IMAP/SMTP account | P0 | Manual host/port/TLS; validate server, never log secret; connection status clear |
| ACC-02 | Auto-discover provider | P1 | Clearly show detected host/ports and allow manual override; no unsafe arbitrary URL fetch |
| ACC-03 | Delete/disable account | P0 | Confirm scope; revoke session, clean local credentials/cache; do not delete server inbox by default |
| ACC-04 | Re-auth / app password | P0 | Dedicated state and recovery, secrets never echoed |
| ACC-05 | OAuth2 providers | P1 | PKCE where applicable; refresh securely; permission minimization |
| ACC-06 | From identity / aliases | P1 | Explicit provider-validated or configured alias; default reply identity = recipient account |
| NAV-01 | Unified Inbox | P0 | Merge sorted inbox views with account markers; no lost provenance |
| NAV-02 | Per-account folders | P0 | Server folder tree, UTF-7 variants and special-use mapping |
| NAV-03 | Counts/read/unread | P0 | Server flags reconciled after sync, counts not silently assumed |
| NAV-04 | Star/archive/trash/move | P0 | Provider capability-aware ops and visible optimistic/rollback states |
| NAV-05 | Search | P1 | Scoped search, server-side IMAP SEARCH fallback, privacy-aware indexing |
| NAV-06 | Advanced filters | P1 | Sender, date, has attachment, account, folder, unread |
| READ-01 | Plain-text read | P0 | RFC-compatible MIME parser, UTF encodings, safe fallback |
| READ-02 | HTML read | P0 | Sanitized, isolated content; remote images blocked by default |
| READ-03 | Attachment metadata/download | P0 | Correct filename handling, MIME sniff/sanitized names, size limits, safe download |
| READ-04 | Conversations/threads | P1 | Subject/reference headers, do not accidentally merge unrelated threads |
| READ-05 | Quoted content collapse | P1 | Preserve full message access; never silently discard text |
| SEND-01 | Compose + sender account | P0 | To/CC/BCC subject and plain/HTML body; sender identity always visible |
| SEND-02 | Reply/reply-all/forward | P0 | Correct headers (`In-Reply-To`, `References`), avoid unintended CC/BCC leaks |
| SEND-03 | SMTP submit | P0 | Verified TLS, supported auth; no 'sent' status before confirmed submission |
| SEND-04 | Attachments upload | P0 | Explicit limits, streaming or bounded temp files, content scanning policy |
| SEND-05 | Drafts | P0 | Local draft survives restart; optional server Drafts sync documented |
| SEND-06 | Sent folder reconciliation | P0 | Detect server auto-copy vs explicit IMAP APPEND; never duplicate Sent |
| SEND-07 | Scheduled send | P2 | Background job and timezone correctness, cancellation and crash recovery |
| SEND-08 | Templates/signatures | P1 | Per-identity templates, HTML sanitization, escaping |
| SYNC-01 | Initial IMAP sync | P0 | Bounded page window; folder selection, UIDVALIDITY persisted |
| SYNC-02 | Incremental sync | P0 | IDLE where supported, polling fallback, reconnect with backoff and jitter |
| SYNC-03 | Flag reconciliation | P0 | Read/star changes across clients converge or show conflict |
| SYNC-04 | UIDVALIDITY change | P0 | Invalidate/rebuild affected folder cache; no wrong message mapping |
| SYNC-05 | Rate limits/quota | P0 | Bounded concurrent connections, avoid retry storms |
| SYNC-06 | Offline/status UX | P0 | Clearly show last synced, pending, failing, reauth required |
| SEC-01 | Protected secrets | P0 | OS keychain Desktop; AEAD encrypted credential store Web with external master key |
| SEC-02 | User authentication | P0 Web | Auth required before any private API; secure session cookies, CSRF policy |
| SEC-03 | HTML/message isolation | P0 | Prevent XSS/remote tracking/privileged desktop bridge from email HTML |
| SEC-04 | SSRF/host validation | P0 Web | Private/link-local/cloud metadata egress forbidden unless explicit admin allowlist, DNS revalidation |
| SEC-05 | Audit & privacy | P0 | No raw message content in telemetry; export/delete data |
| UI-01 | Light/dark | P0 | Tokens consistent; retains contrast and icons |
| UI-02 | Responsive views | P0 | 3-column desktop; 1-column mobile; no clipped critical actions |
| UI-03 | Keyboard & focus | P1 | Shortcut help, focus rings, accessibility, semantic controls |
| UI-04 | Localizations | P1 | Vietnamese and English resource strings, no hardcoded future translations |
| OPS-01 | Docker self-host | P1 | Non-root, health endpoints, TLS behind reverse proxy, persisted data |
| OPS-02 | Desktop installer | P1 | Signed builds and least-privilege OS integration |
| OPS-03 | Backup/restore | P1 | Metadata backup excludes secrets by default; documented restore process |
| OPS-04 | Observability | P0 | Sync duration/errors/retry telemetry without PII |
| OPS-05 | Import/export settings | P2 | Encrypted and explicit, never expose creds accidentally |

## High-risk acceptance examples

**ACC-01**: invalid cert => abort with trust error; password never in server response or log; 993 implicit TLS and 587 STARTTLS require certificate validation; provider-specific auth supported via adapter, not guessed.

**SEND-03**: when SMTP connection drops after DATA but before final response, mark **outcome unknown**, do NOT resend automatically. User can inspect server Sent folder and retry deliberately.

**SYNC-04**: `UID=123` in `(account=A, folder=Inbox, UIDVALIDITY=10)` is distinct from UID=123 after UIDVALIDITY=11. Rebuild that folder's mapping, retain visible warning while reconciling.

**READ-02**: HTML with `<script>`, event attributes, `javascript:` URLs, remote `<img>`, CSS external fetch and embedded iframes cannot execute active behavior or remote loads by default.

**ACC-03**: removing locally configured account must not issue remote IMAP `DELETE` folders or messages. Separate local purge decision from remote deletion.

**NAV-01**: no cross-account IMAP MOVE; unified inbox is a virtual query over per-account state.
