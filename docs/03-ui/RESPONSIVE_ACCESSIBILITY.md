# Responsive UI, accessibility & content design

## Breakpoints (proposed)

| Width | Layout | UX requirements |
|---|---|---|
| ≥ 1440px | Sidebar + list + full reader | Search, message actions always visible |
| 1024–1439px | Compact sidebar, list + reader | Collapse nonessential labels; no horizontal overflow |
| 768–1023px | Icon rail + list/reader | Reader may take full content with back navigation |
| < 768px | One primary pane at a time | Bottom/rail navigation, touch targets, safe composition |

Current starter CSS has 1200/850/600 breakpoints but mobile list/reader flow is provisional; **MB-102 must implement a true single-pane mobile workflow**, not simply vertically stacking list and reader.

## A11y baseline

- WCAG 2.2 AA target: body/interactive contrast, visible focus, logical tab order.
- Full keyboard nav for all UI actions; Esc closes dialogs and dropdowns.
- Use real `<button>`, `<input>`, `<label>` and semantics, not plain clickable divs.
- Screen reader status via `aria-live="polite"` for sync; errors via alert; sender/subject/attachment have descriptive text.
- Touch targets ≥ 44px where reasonable; no hover-only required actions.
- Avoid low-contrast unread indicators: combine blue dot with font weight/text semantics.
- Provide an explicit option to disable animations and respect reduced-motion preferences.
- Do not use color alone for account status; include text and icons.
- All locales/date formats must be normalized by region/timezone settings, not hardcoded sample dates.

## Component tests

1. Keyboard only: compose/send and account switch reachable with visible focus.
2. Zoom browser to 200% and 400%, no critical clipping.
3. VoiceOver (macOS), NVDA (Windows) smoke tests.
4. Color contrast and high-contrast OS modes.
5. Large attachment names with long Unicode filenames remain readable/ellipsized and discoverable.
6. Compose cannot lose draft silently from route change or crash.

## Content and language

Default Vietnamese copy concise and not overly technical. Prefer "Máy chủ nhận thư (IMAP)" over raw jargon during onboarding; full diagnostics optionally expandable for technical users. Errors should propose a safe next action (check app password, provider host, internet status) without inviting TLS bypass.
