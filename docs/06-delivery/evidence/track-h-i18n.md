# Track H evidence — Vietnamese and English interface language (MB-105, MB-221)

Branch `claude/mb-i18n-6b39ea`, 2026-11-15, macOS aarch64, rustc 1.97.0. Desktop first; Web stays demo-only.

## What was built

| Part | Where | Notes |
|---|---|---|
| i18n layer | `apps/mailbox-ui/src/i18n/{mod.rs,en_core.rs,en_settings.rs,en_connect.rs,en_compose.rs,tests.rs}` | `Lang::{Vi, En}` (type lives in `mailbox_core::prefs`, re-exported). `LANG` is a global signal (Vietnamese default); components that call `tr` re-render when it changes, so no restart. `tr("<Vietnamese>")` returns the Vietnamese text unchanged in Vietnamese and the catalog entry in English (key = the Vietnamese string, so Vietnamese output is the source text byte for byte). `bi(vi, en)` picks between two ready-made values for strings with names or numbers; `s(n)` is the English plural suffix; `when(label)` writes time labels for the language. No new dependency, no JS. |
| Catalog | 591 hand-written Vietnamese→English pairs in four files by area (shell/inbox/reader/thread/folders/search/live/shortcuts in `en_core`, settings and prefs in `en_settings`, connect wizard/autoconfig/OAuth mock/adapters in `en_connect`, compose/outbox in `en_compose`) | Typed by key text, not by id; a missing English entry falls back to Vietnamese and fails a test. |
| Preference | `mailbox_core::prefs::Preferences.lang` (serde default `vi`, so old stored records stay Vietnamese; JSON record, no migration), `adapters/prefs.rs::{load_language, save_language}`, `state/prefs.rs::choose_language` | Chosen in Settings → Giao diện (`components/lang_picker.rs`, both demo and real panel). Applied at once, stored by reading the stored record, changing only `lang` and writing it back (works in demo and real mode, never saves unsaved edits of the working copy). Read at app start; loaded with the other settings. "Restore defaults" keeps the language. |
| Notifications | `adapters/notify.rs::localize_notice` | The policy in `mailbox_core::notify` writes Vietnamese. The adapter turns only its fixed phrases ("Có N thư mới", "Mới nhất từ …") into English from the **stored** language, so it is right on any thread. Sender and subject are never changed. |
| Dates and counts | `i18n::when`, plural via `bi`/`s` | English: `Hôm qua` → `Yesterday`, Vietnamese weekdays → `Mon…Sun`, `12/11/2026` → `12 Nov 2026`, `01/11` → `1 Nov`; times and `UTC` pass through. Counts and aria-labels use English word order and plurals ("5 unread", "2 messages in the conversation"). |

## Vietnamese stays byte-identical

The key is the Vietnamese source string; `tr` in Vietnamese returns it unchanged, `when` returns its input. Every UI test and every other workspace test that existed before this track passes without edits. The only Vietnamese UI change is that the stub row "Ngôn ngữ / English có ở MB-105" in demo Appearance became the real language control ("Chọn ngôn ngữ giao diện"). `lang="vi"` is added to `app-shell`.

## Tests (written first; RED then GREEN)

RED before the catalog existed: `every_key_has_both_languages…` and `every_literal_given_to_tr_has_an_english_entry` failed with the missing strings listed; after filling the catalog the first SSR run failed on `Hôm qua` (dates), which drove `i18n::when`. Added:

- `crates/mailbox-core/src/prefs.rs`: `language_defaults_to_vietnamese_for_old_records`, `language_roundtrips_and_survives_restore_defaults`.
- `i18n/tests.rs` (7): every catalog key has non-empty English that differs from the Vietnamese; same `{`/`}` count and trailing space; **every `tr("…")` literal in every source file has an English entry** (scans the sources, rustfmt-wrapped calls too); const tables (shortcuts, built-in templates, core template errors) covered; **no unused catalog entries**; Vietnamese is the default and `tr`/`when` are identity there.
- `adapters/prefs.rs`: `language_is_remembered_and_leaves_other_settings_alone` (real SQLite file, reopened).
- `adapters/notify.rs`: `notices_keep_vietnamese_and_translate_only_the_fixed_phrases_in_english`.
- `main.rs` `ui_tests` (appended, 8): default is Vietnamese with `lang="vi"`; English demo shell and inbox contain the English labels, keep the demo badge, contain none of a checked list of 11 Vietnamese phrases **and** no Vietnamese letter at all once the demo fixture text (mail, accounts, contacts, `Tiếng Việt`, `Minh Nguyễn`) is removed; the same strict check for 13 demo routes (thread, compose, connect, every settings tab, not-found) and 10 real-mode routes; language control present in demo and real, current one `aria-checked`; counts/labels follow the language; dates follow the language.
- `e2e/i18n-real.spec.ts` (3 specs): default Vietnamese; switch to English without restart and back; on Web the choice is not stored, a reload shows the default.

## Commands and results (this branch, final tree)

```
cargo fmt --all -- --check                                                         FMT-OK
cargo clippy --workspace --all-targets -- -D warnings                              Finished, 0 warnings
cargo clippy -p mailbox-ui --no-default-features --features desktop -- -D warnings Finished, 0 warnings
cargo clippy -p mailbox-ui --no-default-features --features web --target wasm32-unknown-unknown -- -D warnings   Finished, 0 warnings
cargo test --workspace --no-fail-fast                                              56 test binaries: 461 passed, 0 failed, 23 ignored (opt-in real-provider tests)
  mailbox-ui: 143 passed (all earlier tests unchanged, plus the new ones listed above)
```

Playwright: **not run** (TEAM_RULES forbids workers from running Playwright and `dx serve`); the spec is written against the same test ids and must be run by the coordinator with the existing config (`pnpm test -- i18n-real.spec.ts` in `e2e/`).

## Strings left in Vietnamese only

- Demo fixture content: message subjects, previews, bodies, senders, labels, attachment names, account names and the demo contacts (`state/demo.rs` CONTACTS). Email content is out of scope by the brief. Their time labels do follow the language.
- Reply/forward attribution line `--- name <addr> đã viết (date):` is now `… wrote (date):` in English, but `date` is copied from the message as stored, so a demo message shows its Vietnamese weekday in it.
- Native window title is the fixed "MailBox".
- Text typed or stored by the user (signatures, templates, the demo signature sample) is never translated.
- Mail-server error text is never shown; the app's own messages for each failure are translated.

## Shared / other-track files touched (smallest possible diff; only string literals wrapped, nothing moved)

`state/compose.rs` (reply attribution, built-in template table left as plain consts), `state/outbox.rs` (status/error strings), `components/oauth_mock.rs`, `components/autoconfig.rs`, `adapters/connect.rs`, `state/connect.rs`. `adapters/send.rs` and `adapters/worker.rs` needed no change (no UI string in them). Shared: `app.rs` (language read at start, `lang` attribute), `components/mod.rs` (one `pub mod`), `main.rs` (`mod i18n;`, appended tests), `state/prefs.rs`, `adapters/prefs.rs`, `adapters/notify.rs`, `crates/mailbox-core/src/prefs.rs` (additive `Lang`).

## Proposed TRACKING rows (TEAM_RULES says workers do not edit TRACKING.md; the coordinator applies these)

| ID | Pri | Task | Dep | Status | Acceptance / evidence |
|---|---|---|---|---|---|
| MB-105 | P0 | Add localization infrastructure VI/EN | MB-102 | DONE (Desktop; Web session-only) | Runtime switch without restart, stored in the preferences record, catalog + source-scan tests, SSR tests in both languages. Evidence: `evidence/track-h-i18n.md` |
| MB-221 | P3 | Full i18n coverage | MB-105 | DONE for the Desktop surface | All 591 catalog strings extracted and translated; strict no-Vietnamese-left SSR tests on 23 routes; fixtures and user text excluded by design. Playwright spec written, not run by this worker |

## Risks

- Catalog quality is one translator's work: English wording should get an owner review.
- A new `tr("…")` without a catalog entry shows Vietnamese in English until the unit test is run (the test fails, so CI catches it).
- Toasts and dialogs keep the language they were created in until closed.
- At start the UI is Vietnamese for a moment until the stored language is read.
