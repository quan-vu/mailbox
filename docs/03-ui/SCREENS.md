# UI screen specifications — approved visual direction

**Image files** are in `references/`. Each screen should be faithfully implemented (hierarchy, spacing, UX), but without misleading example provider permissions, success states or fabricated server data. The starter renders mock layout; production transitions are TODO.

## S01 — Unified Inbox

Reference: [`01-unified-inbox.png`](references/01-unified-inbox.png)

**Route target:** `/inbox`

**Major regions:** app bar; primary nav/sidebar; accounts; unread badge; search; filters; email list; detail reading pane; attachment cards; Reply/Reply all/Forward.

**Events**: select message → reader; change filter/account → list updates; search → account-scoped search; select attachment → guarded preview/download; reply → composer from correct account; star/read/archive → server mutation with pending state.

**Responsive**: ≥ 1200px full sidebar + list + reader; 850–1199px narrower sidebar; ≤ 600px display list then reader via navigation instead of stacking both in long scroll (implemented in MB-102: list and reader swap, with a back button).

**Empty/error**: no accounts → onboarding; empty folder → helpful no mail; stale sync → warning; invalid message → retry; network offline → cached data banner.

**Accessibility**: list items keyboard selectable, visible focus; sender name and subject text alternatives; timestamp full date on hover/focus.

**Acceptance**: all account filters keep account tags visible; selection never silently changes SMTP identity.

## S02 — Add/Connect Account

Reference: [`02-connect-account.png`](references/02-connect-account.png)

**Route target:** `/accounts/connect`

**Regions**: optional left education panel; provider shortcuts; automatic discovery toggle; email address; authentication flow; IMAP host/port/TLS; SMTP host/port/TLS; test connection; 3-step progress; error recovery.

**Events**: select provider → suggestions; auto-discovery → well-known provider rules; manual override → explicit state; test IMAP + SMTP separately; verify connection → secure persistence via adapter; show last tested cert hostname only if safe and useful.

**Security**: do not collect genuine passwords until secure store/transport and policy are implemented. Hide/disable raw secret field in starter. Never show green success before round-trip protocol test. For OAuth-based providers, redirect through correct OAuth flow, no plaintext password form.

**States**: `manual`, `detecting`, `need_app_password`, `oauth_redirect`, `imap_ok_smtp_failed`, `certificate_error`, `auth_failed`, `rate_limited`, `completed`. Copy must explain next action without leaking credentials.

**Acceptance**: cPanel Dovecot case + custom IMAP configuration validated; no certificate bypass button.

## S03 — Conversation / Thread

Reference: [`03-thread.png`](references/03-thread.png)

**Route target:** `/messages/{id}/thread`

**Regions**: compact nav, list column optional, subject + account chip, message timeline, attachment preview, quotes, actions, snooze/labels in post-MVP.

**Events**: expand/collapse individual messages; show original headers; reply/reply all/forward; mark star/read; link to attachments.

**Edge cases**: missing/duplicated Message-ID, wrong thread attribution, forwarded messages, collapsed prior quoted content, cross-account conversation grouping (view only, never cross-account remote mutation).

**Acceptance**: thread order stable and each message retains its original sending/receiving account.

## S04 — Compose / Reply

Reference: [`04-compose.png`](references/04-compose.png)

**Route target:** `/compose`

**Regions**: From identity; recipient chips; CC/BCC; subject; editor; attachments; signature; schedule dropdown (post-MVP); template rail (P1); send CTA; draft state.

**Events**: from-account switch must confirm if replying across identity; validate recipients; autosave after debounce and on blur; upload files with size/type policies; send → SMTP job and status; no false success.

**UX rules**: replying should use original account and Reply-To semantics, keep BCC hidden from Reply All, warn on empty recipient, confirm destructive draft close, disable send while required data absent or connection unavailable.

**States**: `editing`, `saving`, `saved`, `offline`, `sending`, `accepted`, `rejected`, `unknown`; draft persisted securely before any provider operation.

**Acceptance**: no accidental send from default account when reading another mailbox. Ambiguous SMTP outcome shown with manual verification action.

## S05 — Accounts & Settings

Reference: [`05-settings.png`](references/05-settings.png)

**Route target:** `/settings`

**Regions**: second nav rail (Accounts, Sync, Appearance, Security, Signature, Notifications, Advanced); account cards; health; quota; aliases; default sending identity; signature editor; connected devices.

**Events**: add/edit/disable/detach/reconnect; change theme; change notifications; revoke device session; view sync logs (PII redacted).

**Data truth**: quota from provider QUOTA capability when available; `unknown` otherwise, not invented storage limits; connected badge requires valid session. Account detach must not delete hosted mailbox content.

**States**: account disconnected/requires attention/disabled; last successful sync; quota unknown; certificate problem, unverified alias; incomplete settings save.

**Acceptance**: user's account never removed from mail server without explicit independent action; credentials not displayed in form or logs.

## Shared design references and current implementation map

All screens render deterministic demo fixtures (`mailbox_contracts::fixtures`). Routes are real URLs (MB-104).

| Screen | Route | Source (`apps/mailbox-ui/src`) | State (pure, unit-tested) | Current state |
|---|---|---|---|---|
| Shell | all | `app.rs` (`Body`), `components/shell.rs` (`Topbar`, `Sidebar`, `BottomNav`) | `state/mod.rs` (`AppState`) | Demo badge, theme toggle, folder/account nav, mobile drawer + bottom nav |
| S01 | `/inbox` | `screens/inbox.rs` (`InboxScreen`, `MessageRow`, `Reader`, `AttachmentCard`) | `state/mail.rs` | Filter, search, select, star, read/unread, archive/delete + undo, one-pane mobile |
| S02 | `/accounts/connect` | `screens/connect.rs` | `state/connect.rs` | 3-step wizard, provider suggestions, validation, simulated outcomes; password field disabled, no network |
| S03 | `/messages/{id}/thread` | `screens/thread.rs` | `state/mail.rs` | Conversation of the message, expand/collapse, reply from receiving account |
| S04 | `/compose` | `screens/compose.rs` | `state/compose.rs` | Recipient chips + validation, Cc/Bcc, templates, identity warning, demo send dialog (never sends) |
| S05 | `/settings`, `/settings/{tab}` | `screens/settings.rs` (9 tabs) | `state/mod.rs` | Accounts (demo status), detach confirm, theme, toggles; in-session only |
| Not found | any other path | `routes.rs` (`Missing`) | — | Shell kept, link back to inbox |

Shared components: `components/icon.rs` (Lucide-style inline SVG, ISC), `components/common.rs` (`Avatar`, `DemoBadge`, `Switch`, `EmptyState`, `ToastHost`, `ConfirmHost`). Tokens: `assets/tokens.css`; styles: `assets/main.css`.

Deviations from the reference PNGs approved at design review (Gate G1, 2026-11-14): demo badge instead of green sync status; disabled password field; account status "Demo"; quota marked illustrative; neutral provider monograms; `--muted` `#5F6D84`.

The 5 PNGs are intentionally in Git/repository so a code agent can open the exact visual references without relying on earlier chat history.
