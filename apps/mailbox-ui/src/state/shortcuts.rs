//! Keyboard shortcuts (MB-210): one registry for the bindings, the help overlay and the settings
//! list. `resolve` is pure: it decides from the key, the modifiers, where the focus is and the
//! on/off setting. Nothing fires while the user types in an input, textarea or editable area.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Compose,
    Reply,
    ReplyAll,
    Forward,
    Archive,
    Delete,
    Star,
    MarkUnread,
    Next,
    Previous,
    Open,
    Search,
    Send,
    Close,
    Help,
}

#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub action: Action,
    /// Single keys as `KeyboardEvent.key` reports them (no modifier).
    pub keys: &'static [&'static str],
    /// Key combination with Ctrl (Windows, Linux) or Cmd (macOS).
    pub with_modifier: Option<&'static str>,
    pub group: &'static str,
    pub label: &'static str,
    /// What the help overlay shows, e.g. `Ctrl/Cmd + K`.
    pub shown: &'static str,
}

pub const BINDINGS: &[Binding] = &[
    b(
        Action::Compose,
        &["c"],
        None,
        "Soạn thư",
        "Soạn thư mới",
        "C",
    ),
    b(Action::Reply, &["r"], None, "Thư đang chọn", "Trả lời", "R"),
    b(
        Action::ReplyAll,
        &["a"],
        None,
        "Thư đang chọn",
        "Trả lời tất cả",
        "A",
    ),
    b(
        Action::Forward,
        &["f"],
        None,
        "Thư đang chọn",
        "Chuyển tiếp",
        "F",
    ),
    b(
        Action::Archive,
        &["e"],
        None,
        "Thư đang chọn",
        "Lưu trữ",
        "E",
    ),
    b(
        Action::Delete,
        &["#", "Delete"],
        None,
        "Thư đang chọn",
        "Xóa (chuyển vào Thùng rác)",
        "# hoặc Delete",
    ),
    b(
        Action::Star,
        &["s"],
        None,
        "Thư đang chọn",
        "Gắn / bỏ sao",
        "S",
    ),
    b(
        Action::MarkUnread,
        &["u"],
        None,
        "Thư đang chọn",
        "Đánh dấu chưa đọc",
        "U",
    ),
    b(Action::Next, &["j"], None, "Danh sách", "Thư kế tiếp", "J"),
    b(
        Action::Previous,
        &["k"],
        None,
        "Danh sách",
        "Thư trước",
        "K",
    ),
    b(
        Action::Open,
        &["Enter"],
        None,
        "Danh sách",
        "Mở hội thoại",
        "Enter",
    ),
    b(
        Action::Search,
        &["/"],
        Some("k"),
        "Chung",
        "Tìm kiếm",
        "/ hoặc Ctrl/Cmd + K",
    ),
    b(
        Action::Send,
        &[],
        Some("Enter"),
        "Soạn thư",
        "Gửi thư",
        "Ctrl/Cmd + Enter",
    ),
    b(
        Action::Close,
        &["Escape"],
        None,
        "Chung",
        "Đóng hộp thoại",
        "Esc",
    ),
    b(
        Action::Help,
        &["?"],
        None,
        "Chung",
        "Hiện bảng phím tắt",
        "?",
    ),
];

const fn b(
    action: Action,
    keys: &'static [&'static str],
    with_modifier: Option<&'static str>,
    group: &'static str,
    label: &'static str,
    shown: &'static str,
) -> Binding {
    Binding {
        action,
        keys,
        with_modifier,
        group,
        label,
        shown,
    }
}

/// One key press as the page reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInput {
    pub key: String,
    /// Ctrl or Cmd held.
    pub modifier: bool,
    pub alt: bool,
    /// Focus is in an input, textarea, select or editable area.
    pub typing: bool,
    /// Focus is on something Enter already activates (button, link, switch...).
    pub interactive: bool,
}

/// Which action a key press means, if any.
///
/// * Esc always means [`Action::Close`]: closing a dialog must keep working.
/// * With shortcuts switched off nothing else fires.
/// * Ctrl/Cmd + K and Ctrl/Cmd + Enter work while typing (they cannot be typed text).
/// * Plain keys never fire while typing; Enter also not on a focused button or link.
pub fn resolve(input: &KeyInput, enabled: bool) -> Option<Action> {
    if input.key == "Escape" {
        return Some(Action::Close);
    }
    if !enabled || input.alt {
        return None;
    }
    if input.modifier {
        let key = input.key.to_ascii_lowercase();
        return BINDINGS
            .iter()
            .find(|b| {
                b.with_modifier
                    .is_some_and(|m| m.eq_ignore_ascii_case(&key))
            })
            .map(|b| b.action);
    }
    if input.typing {
        return None;
    }
    let found = BINDINGS
        .iter()
        .find(|b| b.keys.contains(&input.key.as_str()))?;
    if found.action == Action::Open && input.interactive {
        return None;
    }
    // Escape is handled above; keys without a screen to act on are filtered by the dispatcher.
    Some(found.action)
}

/// Plain characters the page may swallow (`preventDefault`) when they are shortcuts, so `/`
/// does not start the browser's quick find. Enter and Escape are never listed.
pub fn swallowed_keys() -> Vec<&'static str> {
    BINDINGS
        .iter()
        .flat_map(|b| b.keys.iter().copied())
        .filter(|k| !matches!(*k, "Enter" | "Escape"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: &str) -> KeyInput {
        KeyInput {
            key: k.into(),
            modifier: false,
            alt: false,
            typing: false,
            interactive: false,
        }
    }

    #[test]
    fn every_listed_action_has_a_binding_that_resolves() {
        for b in BINDINGS {
            for k in b.keys {
                assert_eq!(resolve(&key(k), true), Some(b.action), "{k}");
            }
        }
        let mut cmd_k = key("k");
        cmd_k.modifier = true;
        assert_eq!(resolve(&cmd_k, true), Some(Action::Search));
        let mut send = key("Enter");
        send.modifier = true;
        assert_eq!(resolve(&send, true), Some(Action::Send));
    }

    #[test]
    fn plain_keys_do_not_fire_while_typing() {
        for k in [
            "c", "r", "a", "f", "e", "#", "Delete", "s", "u", "j", "k", "Enter", "/", "?",
        ] {
            let mut i = key(k);
            i.typing = true;
            assert_eq!(resolve(&i, true), None, "{k} fired while typing");
        }
    }

    #[test]
    fn send_and_search_combos_work_while_typing() {
        let mut send = key("Enter");
        send.modifier = true;
        send.typing = true;
        assert_eq!(resolve(&send, true), Some(Action::Send));
        let mut k = key("K");
        k.modifier = true;
        k.typing = true;
        assert_eq!(resolve(&k, true), Some(Action::Search));
    }

    #[test]
    fn disabled_setting_silences_everything_except_escape() {
        for b in BINDINGS {
            for k in b.keys {
                let expect = (*k == "Escape").then_some(Action::Close);
                assert_eq!(resolve(&key(k), false), expect, "{k}");
            }
        }
        let mut send = key("Enter");
        send.modifier = true;
        assert_eq!(resolve(&send, false), None);
    }

    #[test]
    fn escape_closes_even_while_typing() {
        let mut i = key("Escape");
        i.typing = true;
        assert_eq!(resolve(&i, true), Some(Action::Close));
    }

    #[test]
    fn enter_on_a_focused_button_is_left_to_the_button() {
        let mut i = key("Enter");
        i.interactive = true;
        assert_eq!(resolve(&i, true), None);
        assert_eq!(resolve(&key("Enter"), true), Some(Action::Open));
    }

    #[test]
    fn alt_and_unknown_combinations_do_nothing() {
        let mut i = key("c");
        i.alt = true;
        assert_eq!(resolve(&i, true), None);
        let mut m = key("c");
        m.modifier = true; // Ctrl+C is copy
        assert_eq!(resolve(&m, true), None);
        assert_eq!(resolve(&key("C"), true), None); // Shift+C is not "c"
    }

    #[test]
    fn keys_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for b in BINDINGS {
            for k in b.keys {
                assert!(seen.insert(*k), "{k} bound twice");
            }
        }
    }
}
