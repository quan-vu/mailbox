# Web API contract (versioned)

**Implemented today** (local demo only, no authentication; bound to 127.0.0.1 by default):

```http
GET /healthz
200 { "status": "ok", "mode": "demo" }

GET /api/v1/demo/bootstrap
200 BootstrapResponse { "mode": "demo", "accounts": [...], "messages": [...] }
```

`mailbox-contracts/src/lib.rs` is the source for current DTOs. Demo API must never accept credentials or be publicly exposed.

**Proposed production contract — not implemented:**

| Method | Path | Purpose | Auth |
|---|---|---|---|
| POST | `/api/v1/session` | Start authenticated session; CSRF policy | Login flow |
| DELETE | `/api/v1/session` | End session/revoke token | Required |
| GET | `/api/v1/accounts` | List own connected accounts, safe fields | Required |
| POST | `/api/v1/accounts/discovery` | Suggested provider settings (SSRF-safe) | Required |
| POST | `/api/v1/accounts` | Connect account; ephemeral secret over HTTPS | Required |
| PATCH | `/api/v1/accounts/{id}` | Update settings/disable; ownership scoped | Required |
| DELETE | `/api/v1/accounts/{id}` | Detach and purge locally; remote mailbox intact | Required |
| POST | `/api/v1/accounts/{id}/test` | Test IMAP and SMTP separately | Required |
| GET | `/api/v1/accounts/{id}/folders` | List provider folder mapping | Required |
| GET | `/api/v1/messages` | Cursor pagination, account/folder/filter | Required |
| GET | `/api/v1/messages/{id}` | Fetch body and attachment metadata | Required |
| PATCH | `/api/v1/messages/{id}/flags` | Mark read/star; optimistic conflict handling | Required |
| POST | `/api/v1/messages/{id}/move` | Move same-account folder if supported | Required |
| GET | `/api/v1/messages/{id}/attachments/{part}` | Authorized streaming download | Required |
| GET | `/api/v1/drafts` | Own local drafts | Required |
| POST | `/api/v1/drafts` | Create versioned draft | Required |
| PATCH | `/api/v1/drafts/{id}` | Conditional update with version | Required |
| POST | `/api/v1/drafts/{id}/submit` | Explicit send intent; idempotency/uncertainty policy | Required |
| GET | `/api/v1/send-jobs/{id}` | `queued/sending/accepted/unknown/rejected` | Required |
| GET | `/api/v1/sync-status` | Account last-sync state/health | Required |
| GET | `/api/v1/events` | Authenticated SSE mailbox change stream | Required |

## JSON response examples (future contract)

```json
{
  "data": [
    {"id":"opaque-id","account_id":"work","folder_id":"inbox","from_name":"A","subject":"Hello","unread":true}
  ],
  "next_cursor":"opaque-signed-cursor-or-null",
  "sync_state":"ready"
}
```

```json
{
  "error": {
    "code": "imap_auth_required",
    "message": "Vui lòng xác thực lại tài khoản email.",
    "retryable": false,
    "request_id": "opaque-id"
  }
}
```

## Contract conventions

- Stable opaque IDs, API prefixed `/api/v1`.
- Cursor pagination; avoid offset for mutable inbox ordering.
- Backend authorization of account ownership at every endpoint; no trusting account_id supplied by UI.
- Standard error codes: `validation_failed`, `auth_required`, `access_denied`, `imap_unavailable`, `imap_auth_required`, `tls_invalid`, `sync_in_progress`, `rate_limited`, `smtp_rejected`, `send_outcome_unknown`.
- 401 vs 403 used consistently. 429 includes `Retry-After`; 409 for version mismatch; 413 for exceeded upload limits; sanitize all user-facing provider errors.
- All mutations require CSRF protection for cookie-based sessions and/or same-origin CSRF mitigations.
- `Idempotency-Key` applies to creating a send intent; it does not magically make SMTP delivery exactly-once.
- Never include password/refresh token/secret in GET/response serialization.
- Future: OpenAPI schema code-generated from shared DTOs once production endpoints exist; do not claim spec implemented prematurely.

## Demo endpoint CORS (MB-103)

`GET /api/v1/demo/bootstrap` answers cross-origin requests only for loopback http origins (`http://127.0.0.1:<port>`, `http://localhost:<port>`). `MAILBOX_CORS_ORIGINS` (comma-separated) can change the port list; non-loopback or https origins in it are ignored. This is a development convenience for the Dioxus dev server, not an authorization model: the endpoint stays unauthenticated and local-only until MB-110.
