# MailBox UI Design System v0.1

**Visual source of truth**: `references/01-unified-inbox.png` to `05-settings.png`. These are generated design references approved as the direction, not exact pixel coordinates or evidence of working functionality. Source code CSS variables: `apps/mailbox-ui/assets/main.css`.

## 1. Design intent

- Gmail-level clarity, but own identity: bright blue mark and calm neutral cards.
- Desktop-first three-column workflows, graceful narrow desktop/phone fallbacks.
- Email content receives largest reading area; accounts never become invisible when changing sender.
- Use subtle borders, soft corners, medium density, generous white space; no oversaturated dashboard decoration.
- Interface languages: Vietnamese default, English planned; plain terms such as "Hộp thư hợp nhất", "Soạn thư", "Đồng bộ".

## 2. Design tokens

| Token | Light | Dark target | Application |
|---|---|---|---|
| `--blue` | `#2672F6` | `#4E91FA` | Primary CTA, selected filters |
| `--bg` | `#F8FAFF` | `#101827` | App canvas |
| `--surface` | `#FFFFFF` | `#182335` | Sidebar, email cards |
| `--ink` | `#172137` | `#ECF2FD` | Text |
| `--muted` | `#67758B` | `#AAB7CE` | Secondary text |
| `--line` | `#E6EBF3` | `#2C3B52` | Dividers and controls |
| `--soft` | `#E8F0FF` | `#223B61` | Selected state surfaces |
| success | `#26B55C` | test contrast | Actually verified connected/healthy |
| warning | `#906318` | test contrast | Partial sync/auth attention |
| danger | accessible red | test contrast | Failed/error/deletion confirmation |

**Semantic token rule:** green "Connected" only after real protocol validation; in demo, use neutral or orange `Demo` label regardless of mock fixtures.

## 3. Typography & iconography

- Type stack: `Inter, ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif`; remote font downloads prohibited by default. No fonts bundled with starter.
- Desktop app name 24px bold; primary screen heading 22–24px; sectional heading 17–18px; email sender/subject 13–14px; secondary metadata 11–12px.
- Body email 14px / ~1.65 line-height; user may adjust reading size.
- Use simple outline icons with uniform stroke style (production: choose a reviewed SVG icon set compatible with licenses). Current source uses Unicode placeholders to make scaffold self-contained; **MB-102 must replace them**.
- Preserve real provider logo trademarks only with permission; avoid misleading imitation in shipped assets.

## 4. Layout grids

- Top bar: 72px, logo left, global search center, account/status/settings right.
- Full left navigation: ~264px; collapsible to ~66px on narrower screens.
- Inbox: list 42%, reader 58% after sidebar; list min ~345px, reader min ~430px, fallback at smaller viewports.
- Compose screen: editor fills remaining area with optional ~300px templates panel.
- Account form: explanatory panel ~310px + configuration panel, with a maximum content width on large displays.
- Settings: second navigation rail ~195px + main content cards.
- Spacing unit: 4px; usual padding 8/12/16/24/32px.
- Corner radii: pill `999px`, inputs `9–11px`, cards `12–15px`, primary buttons `12px`.
- Shadows: minimal and controlled, never substitute for accessibility contrast.

## 5. Core components

| Component | Behavior / variants |
|---|---|
| `AppShell` | Topbar + collapsible sidebar + current route/outlet |
| `PrimaryButton` | Enabled, hover, focus, disabled, loading; no fake success |
| `MessageRow` | Unread/read, selected, has attachments, starred, account badge |
| `SenderAvatar` | Initials fallback, safe optional image loading |
| `FilterChip` | Toggle active state, keyboard accessible, clear all |
| `AccountSwitcher` | Account name + address + status; wrong-account prevention |
| `StatusBadge` | Demo, connecting, syncing, healthy, degraded, reauth, offline |
| `EmailPreview` | Plain/HTML switching, sanitized render, remote-content control |
| `AttachmentCard` | Filename, size, type, preview or safe download; no auto-execution |
| `Composer` | To/CC/BCC chips, subject, editor, draft state, attachments, send |
| `SettingsRow` | Option, helper text, current value, accessible action |
| `InlineAlert` | Error/retry, warning, success; screenreader announcements |
| `EmptyState` | Helpful first step for no accounts, empty inbox, zero search results |
| `Skeleton` | Loading states with no misleading fake content |

## 6. Surface interaction details

- Selecting an email updates reading panel without opening a new browser tab.
- Reply defaults sender account to the recipient account of the original message.
- Use Undo only for reversible operations; SMTP submission cannot be un-sent after acceptance.
- Failed account sync must never be masked by another account's success.
- Destructive actions show confirmation when appropriate, with remote vs local scope explicit.
- Date/time localized, represent absolute timestamp in details; avoid misleading "Hôm nay" when test fixtures are old.

## 7. UI state taxonomy

| State | Example copy | Required affordance |
|---|---|---|
| `demo` | “Dữ liệu minh họa” | No real send/connect |
| `loading` | “Đang tải thư…” | Skeleton + escape/focus behavior |
| `syncing` | “Đang đồng bộ (42/280)” | Current progress; not a false done status |
| `connected` | “Đã đồng bộ lúc 14:26” | Accurate per-account timestamp |
| `reauth_required` | “Cần đăng nhập lại” | Reconnect action |
| `partial_failure` | “2/3 tài khoản được đồng bộ” | Error account and retry |
| `offline` | “Đang xem dữ liệu đã lưu” | Last cache time |
| `send_unknown` | “Chưa xác định được thư đã gửi hay chưa” | Review Sent, no auto retry |

## 8. Code design guidance

- Dioxus components should be extracted from `main.rs` as MB-102; create `components/`, `screens/`, `state/`, `adapters/` directories.
- Design tokens in a single CSS layer; do not add custom inline colors outside token contract.
- Breakpoint rules are CSS-first. Avoid leaking native platform APIs into shared UI.
- Replace demo copy with localized string keys in MB-221.
- Refer to `SCREENS.md` for component inventory, routes, empty/error states, and acceptance tests.
