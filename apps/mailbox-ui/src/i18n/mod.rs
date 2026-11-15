//! Interface language (MB-105, MB-221): Vietnamese (default) and English.
//!
//! The Vietnamese text in the source is the lookup key: `tr("Soạn thư")` returns it unchanged in
//! Vietnamese and the entry of the English catalog in English. So the Vietnamese output cannot
//! drift from what the code says, and a missing English entry falls back to Vietnamese (the unit
//! tests fail on that). Strings with values use [`bi`] with two `format!`s, so word order and
//! plurals can differ per language.
//!
//! The current language is the global signal [`LANG`]: components that call `tr` re-render when
//! it changes, with no restart. Outside a Dioxus runtime (plain threads) `lang()` is Vietnamese.
use dioxus::prelude::*;
use std::collections::HashMap;
use std::sync::OnceLock;

pub use mailbox_core::prefs::Lang;

mod en_compose;
mod en_connect;
mod en_core;
mod en_links;
mod en_settings;
mod en_wave3;

pub static LANG: GlobalSignal<Lang> = Signal::global(|| Lang::Vi);

/// Current language. Reading it inside a component subscribes the component to changes.
pub fn lang() -> Lang {
    if dioxus::core::Runtime::try_current().is_some() {
        *LANG.read()
    } else {
        Lang::Vi
    }
}

/// Switch the running UI. Storing the choice is done by `state::prefs`.
pub fn set_lang(lang: Lang) {
    if dioxus::core::Runtime::try_current().is_some() && *LANG.peek() != lang {
        *LANG.write() = lang;
    }
}

/// `"vi"` / `"en"`, for the `lang` attribute of the document.
pub fn code(lang: Lang) -> &'static str {
    match lang {
        Lang::Vi => "vi",
        Lang::En => "en",
    }
}

/// Choose between two ready-made values (use for strings that carry numbers or names).
pub fn bi<T>(vi: T, en: T) -> T {
    match lang() {
        Lang::Vi => vi,
        Lang::En => en,
    }
}

/// A time label as the data gives it, written for the current language. Vietnamese is returned
/// unchanged. English turns `Hôm qua`/`Hôm nay` and Vietnamese weekday names into English words
/// and `dd/mm/yyyy` or `dd/mm` into `12 Nov 2026` / `12 Nov` (day first, month spelled out, so it
/// cannot be misread as month/day). Anything else, such as `03:04` or `UTC`, passes through.
pub fn when(label: &str) -> String {
    if lang() == Lang::Vi {
        return label.to_string();
    }
    let mut text = label
        .replace("Hôm qua", "Yesterday")
        .replace("Hôm nay", "Today");
    for (vi, en) in [
        ("Thứ Hai", "Mon"),
        ("Thứ Ba", "Tue"),
        ("Thứ Tư", "Wed"),
        ("Thứ Năm", "Thu"),
        ("Thứ Sáu", "Fri"),
        ("Thứ Bảy", "Sat"),
        ("Chủ Nhật", "Sun"),
    ] {
        text = text.replace(vi, en);
    }
    text.split(' ')
        .map(english_date_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn english_date_token(token: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (core, tail) = match token.strip_suffix(',') {
        Some(c) => (c, ","),
        None => (token, ""),
    };
    let parts: Vec<&str> = core.split('/').collect();
    let number = |s: &str| {
        s.parse::<u32>()
            .ok()
            .filter(|_| s.chars().all(|c| c.is_ascii_digit()))
    };
    let month = |s: &str| {
        number(s)
            .filter(|m| (1..=12).contains(m))
            .map(|m| MONTHS[m as usize - 1])
    };
    match parts.as_slice() {
        [d, m] => match (number(d), month(m)) {
            (Some(d), Some(m)) => format!("{d} {m}{tail}"),
            _ => token.to_string(),
        },
        [d, m, y] if y.len() == 4 => match (number(d), month(m), number(y)) {
            (Some(d), Some(m), Some(y)) => format!("{d} {m} {y}{tail}"),
            _ => token.to_string(),
        },
        _ => token.to_string(),
    }
}

/// English plural suffix for `n` ("" for one, "s" otherwise).
pub fn s(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Translate a Vietnamese catalog string. Unknown strings stay Vietnamese.
pub fn tr(vi: &'static str) -> &'static str {
    match lang() {
        Lang::Vi => vi,
        Lang::En => index().get(vi).copied().unwrap_or(vi),
    }
}

/// Every `(Vietnamese, English)` pair of the catalog.
pub fn catalog() -> impl Iterator<Item = (&'static str, &'static str)> {
    en_core::ENTRIES
        .iter()
        .chain(en_settings::ENTRIES)
        .chain(en_connect::ENTRIES)
        .chain(en_compose::ENTRIES)
        .chain(en_wave3::ENTRIES)
        .chain(en_links::ENTRIES)
        .copied()
}

fn index() -> &'static HashMap<&'static str, &'static str> {
    static INDEX: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    INDEX.get_or_init(|| catalog().collect())
}

#[cfg(test)]
mod tests;
