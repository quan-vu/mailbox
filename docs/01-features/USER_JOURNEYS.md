# User journeys and edge-case catalog

## J1 — Connect a bought hosted mailbox

1. From Inbox click **Thêm tài khoản**.
2. Enter `name@domain.tld` and choose cPanel/Hosting or Custom.
3. Try audited autoconfig rules; user sees source of suggested hosts and may override.
4. Require valid TLS and test IMAP auth separately from SMTP auth; avoid leaking whether a mailbox exists through public endpoints.
5. On success show provider capability summary and initial sync progress (counts may be incomplete).
6. User sees account in sidebar and connected/last-sync status; never show green 'connected' based on form validation alone.

**Failures**: incorrect app password, disabled IMAP, self-signed cert, TLS downgrade, rate limited, mailbox exceeds quota, DNS resolves private IP, SMTP send not permitted but IMAP works. Present recovery with actionable non-sensitive details and independent IMAP/SMTP status.

## J2 — Read across accounts

1. Select Unified Inbox; merge newest known messages by received timestamp, with stable tie-breaker.
2. Change account filter; update list without losing current email context.
3. Select email; show sender account tag and attachments metadata.
4. Preview safe HTML; remote image permission per sender/message is explicit.
5. Mark read/archive; update UI optimistically only with rollback and offline state, reconcile IMAP flags.

**Failures**: stale account, UIDVALIDITY reset, message expunged while being previewed, MIME parse failure, invalid charset, oversized attachment, alias mismatch.

## J3 — Reply from correct account

1. Open a received message and click Reply.
2. Default From = account that received this message; no silent switch to global default.
3. Prepopulate To with Reply-To if valid; show all recipients for Reply All.
4. Validate From/To/CC/BCC, attachment sizes and actual sending account.
5. Submit with explicit sending progress. On SMTP `250` acceptance mark submitted; reconcile Sent folder policy.

**Failure**: SMTP disconnect after DATA acknowledgment unknown => status `outcome_unknown`, not automatically retried.

## J4 — Recover connection

1. Account shows `Needs attention`, last successful sync timestamp and brief cause.
2. Reconnect/reauth uses secure flow and does not expose old secret.
3. Retry with bounded backoff and jitter, reconcile flags and folder UID epoch.
4. UI transitions through `connecting` → `syncing` → `connected`; errors remain actionable.

## J5 — Desktop setup

1. Install signed bundle; app stores metadata in OS app-data directory.
2. Credentials stored in OS credential manager; avoid port-bound local server accessible by other apps.
3. Open app with default fixture screen only in dev/demo; release requires login/account connect.
4. Send/notification privileges require explicit user action and least-privilege configuration.

## Edge-state checklist for each screen

`loading`, `empty`, `error`, `no-network`, `offline-cache`, `syncing`, `expired-auth`, `read-only-provider`, `quota-warning`, `access-denied`, `partial-provider-outage`, `success`, `sent-outcome-unknown`. Every user-visible state needs copy, accessible announcement where applicable, and test evidence.
