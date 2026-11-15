//! Interface language choice (MB-105, MB-221), shared by the demo and the real Appearance tabs.
//! The names are written in their own language on purpose: a user who cannot read the current
//! language can still find theirs.
use crate::i18n::{Lang, lang, tr};
use dioxus::prelude::*;

#[component]
pub fn LanguagePicker() -> Element {
    let current = lang();
    rsx! {
        div { class: "setting-col", "data-testid": "settings-language",
            strong { {tr("Ngôn ngữ")} }
            small { class: "muted", {tr("Chọn ngôn ngữ giao diện")} }
            div { class: "segmented", role: "radiogroup", "aria-label": tr("Ngôn ngữ"),
                for (l, key, label) in [(Lang::Vi, "vi", "Tiếng Việt"), (Lang::En, "en", "English")] {
                    button { key: "{key}", class: if current == l { "seg on" } else { "seg" }, r#type: "button", role: "radio",
                        lang: "{key}", "aria-checked": if current == l { "true" } else { "false" },
                        "data-testid": "settings-lang-{key}",
                        onclick: move |_| crate::state::prefs::choose_language(l),
                        "{label}"
                    }
                }
            }
        }
    }
}
