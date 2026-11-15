use super::*;

/// Source files whose `t("...")` calls must all have an English entry. Test code is cut off at the
/// first `#[cfg(test)]`, as it keeps Vietnamese on purpose.
const SOURCES: &[(&str, &str)] = &[
    ("app.rs", include_str!("../app.rs")),
    ("routes.rs", include_str!("../routes.rs")),
    (
        "components/schedule.rs",
        include_str!("../components/schedule.rs"),
    ),
    (
        "screens/settings_sync.rs",
        include_str!("../screens/settings_sync.rs"),
    ),
    ("state/schedule.rs", include_str!("../state/schedule.rs")),
    (
        "components/autoconfig.rs",
        include_str!("../components/autoconfig.rs"),
    ),
    ("components/body.rs", include_str!("../components/body.rs")),
    (
        "components/common.rs",
        include_str!("../components/common.rs"),
    ),
    (
        "components/folders.rs",
        include_str!("../components/folders.rs"),
    ),
    (
        "components/link_confirm.rs",
        include_str!("../components/link_confirm.rs"),
    ),
    (
        "components/lang_picker.rs",
        include_str!("../components/lang_picker.rs"),
    ),
    ("components/live.rs", include_str!("../components/live.rs")),
    (
        "components/notify_view.rs",
        include_str!("../components/notify_view.rs"),
    ),
    (
        "components/oauth_mock.rs",
        include_str!("../components/oauth_mock.rs"),
    ),
    (
        "components/search.rs",
        include_str!("../components/search.rs"),
    ),
    (
        "components/shell.rs",
        include_str!("../components/shell.rs"),
    ),
    (
        "components/shortcuts.rs",
        include_str!("../components/shortcuts.rs"),
    ),
    ("screens/compose.rs", include_str!("../screens/compose.rs")),
    ("screens/connect.rs", include_str!("../screens/connect.rs")),
    ("screens/inbox.rs", include_str!("../screens/inbox.rs")),
    ("screens/outbox.rs", include_str!("../screens/outbox.rs")),
    (
        "screens/settings.rs",
        include_str!("../screens/settings.rs"),
    ),
    (
        "screens/settings_real.rs",
        include_str!("../screens/settings_real.rs"),
    ),
    ("screens/thread.rs", include_str!("../screens/thread.rs")),
    ("state/compose.rs", include_str!("../state/compose.rs")),
    ("state/connect.rs", include_str!("../state/connect.rs")),
    ("state/folders.rs", include_str!("../state/folders.rs")),
    ("state/live.rs", include_str!("../state/live.rs")),
    ("state/mail.rs", include_str!("../state/mail.rs")),
    ("state/mod.rs", include_str!("../state/mod.rs")),
    ("state/outbox.rs", include_str!("../state/outbox.rs")),
    ("state/prefs.rs", include_str!("../state/prefs.rs")),
    ("state/search.rs", include_str!("../state/search.rs")),
    ("state/shortcuts.rs", include_str!("../state/shortcuts.rs")),
    (
        "adapters/account.rs",
        include_str!("../adapters/account.rs"),
    ),
    (
        "adapters/actions.rs",
        include_str!("../adapters/actions.rs"),
    ),
    ("adapters/body.rs", include_str!("../adapters/body.rs")),
    (
        "adapters/connect.rs",
        include_str!("../adapters/connect.rs"),
    ),
    (
        "adapters/history.rs",
        include_str!("../adapters/history.rs"),
    ),
    ("adapters/mod.rs", include_str!("../adapters/mod.rs")),
    ("adapters/send.rs", include_str!("../adapters/send.rs")),
    ("adapters/worker.rs", include_str!("../adapters/worker.rs")),
];

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('\n') => {
                // line continuation: skip the next line's indentation
                let rest: String = it.clone().collect();
                let trimmed = rest.trim_start();
                for _ in 0..rest.chars().count() - trimmed.chars().count() {
                    it.next();
                }
            }
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => {}
        }
    }
    out
}

/// Every string literal passed to `tr(` in the non-test part of a source (rustfmt may put the
/// literal on the next line, so whitespace after the parenthesis is skipped).
fn tr_calls(src: &str) -> Vec<String> {
    let end = src.find("#[cfg(test)]").unwrap_or(src.len());
    let src = &src[..end];
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(pos) = src[from..].find("tr(") {
        let at = from + pos;
        from = at + 3;
        let prev = src[..at].chars().next_back().unwrap_or(' ');
        if prev.is_alphanumeric() || prev == '_' || prev == '.' {
            continue;
        }
        let rest = src[at + 3..].trim_start();
        let Some(rest) = rest.strip_prefix('"') else {
            continue;
        };
        let bytes = rest.as_bytes();
        let mut j = 0;
        while bytes[j] != b'"' {
            j += if bytes[j] == b'\\' { 2 } else { 1 };
        }
        found.push(unescape(&rest[..j]));
    }
    found
}

/// Strings that live in `const` tables or in `mailbox-core` (translated where they are shown, so
/// there is no `tr("...")` literal for the scanner to find).
fn table_strings() -> Vec<String> {
    use crate::state::compose::TEMPLATES;
    use crate::state::shortcuts::BINDINGS;
    let has_vi = |s: &str| !s.is_ascii();
    let mut out: Vec<String> = BINDINGS
        .iter()
        .flat_map(|b| [b.group, b.label, b.shown])
        .filter(|s| has_vi(s))
        .map(String::from)
        .collect();
    // Problems reported by `mailbox_core::prefs::Template::problem`, translated where shown.
    let bad = |id: &str, name: &str, subject: &str, body: &str| mailbox_core::prefs::Template {
        id: id.into(),
        name: name.into(),
        subject: subject.into(),
        body: body.into(),
    };
    let long = |n: usize| "x".repeat(n);
    for t in [
        bad("", "n", "s", "b"),
        bad("i", " ", "s", "b"),
        bad("i", &long(81), "s", "b"),
        bad("i", "n", &long(201), "b"),
        bad("i", "n", "a\nb", "b"),
        bad("i", "n", "s", &long(20_001)),
    ] {
        out.push(t.problem().expect("template should be invalid").to_string());
    }
    out.extend(
        TEMPLATES
            .iter()
            .flat_map(|t| [t.title, t.description, t.subject, t.body])
            .map(String::from),
    );
    out
}

#[test]
fn every_key_has_both_languages_and_none_is_empty() {
    let mut seen = std::collections::HashSet::new();
    for (vi, en) in catalog() {
        assert!(!vi.trim().is_empty(), "empty Vietnamese key");
        assert!(!en.trim().is_empty(), "empty English text for {vi:?}");
        assert!(seen.insert(vi), "duplicate key {vi:?}");
        assert_ne!(vi, en, "English text equals Vietnamese for {vi:?}");
    }
    assert!(seen.len() > 300, "catalog looks truncated: {}", seen.len());
}

#[test]
fn english_keeps_the_same_placeholders_and_trailing_space() {
    for (vi, en) in catalog() {
        let count = |s: &str, c: char| s.chars().filter(|x| *x == c).count();
        assert_eq!(count(vi, '{'), count(en, '{'), "braces differ for {vi:?}");
        assert_eq!(
            vi.ends_with(' '),
            en.ends_with(' '),
            "trailing space differs for {vi:?}"
        );
    }
}

#[test]
fn every_literal_given_to_tr_has_an_english_entry() {
    let keys: std::collections::HashSet<&str> = catalog().map(|(vi, _)| vi).collect();
    let mut missing = Vec::new();
    let mut total = 0;
    for (file, src) in SOURCES {
        for lit in tr_calls(src) {
            total += 1;
            if !keys.contains(lit.as_str()) {
                missing.push(format!("{file}: {lit}"));
            }
        }
    }
    for lit in table_strings() {
        if !keys.contains(lit.as_str()) {
            missing.push(format!("const table: {lit}"));
        }
    }
    assert!(
        total > 300,
        "found only {total} tr() calls; scanner broken?"
    );
    assert!(
        missing.is_empty(),
        "{} strings without English:\n{}",
        missing.len(),
        missing.join("\n")
    );
}

#[test]
fn catalog_has_no_unused_entries() {
    let mut used: std::collections::HashSet<String> =
        SOURCES.iter().flat_map(|(_, s)| tr_calls(s)).collect();
    used.extend(table_strings());
    let extra: Vec<&str> = catalog()
        .map(|(vi, _)| vi)
        .filter(|k| !used.contains(*k))
        .collect();
    // Entries reached through `pick`-free indirection (tables, `meta()` tuples) are allowed only
    // when listed here; keep this empty unless there is a reason.
    assert!(extra.is_empty(), "unused catalog entries: {extra:?}");
}

#[test]
fn time_labels_are_unchanged_in_vietnamese() {
    for s in [
        "Hôm qua",
        "Thứ Năm, 12/11/2026 16:05",
        "03:04",
        "14/11/2026 03:04 UTC",
        "01/11",
    ] {
        assert_eq!(when(s), s);
    }
}

#[test]
fn vietnamese_is_the_default_and_t_is_identity_there() {
    assert_eq!(lang(), Lang::Vi);
    assert_eq!(tr("Soạn thư"), "Soạn thư");
    assert_eq!(tr("không có trong bảng"), "không có trong bảng");
    assert_eq!(bi("một", "one"), "một");
    assert_eq!(code(Lang::Vi), "vi");
    assert_eq!(code(Lang::En), "en");
}
