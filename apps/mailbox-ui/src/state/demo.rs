//! Illustrative, UI-only demo data (contacts, aliases, quotas, devices).
//! Moves behind the data-source adapter in MB-103; never shown as verified provider data.

pub struct Contact {
    pub name: &'static str,
    pub address: &'static str,
}

pub const CONTACTS: [Contact; 3] = [
    Contact {
        name: "Nguyễn Văn Đức",
        address: "duc.nguyen@company.vn",
    },
    Contact {
        name: "Kế toán",
        address: "ketoan@company.vn",
    },
    Contact {
        name: "Mai Hương",
        address: "huong.mai@company.vn",
    },
];

/// (account id, used GB, total GB). `None` total = server has no QUOTA capability.
pub const QUOTAS: [(&str, f32, Option<f32>); 3] = [
    ("work", 6.8, Some(10.0)),
    ("personal", 4.1, Some(10.0)),
    ("support", 1.5, None),
];

pub const ALIASES: [&str; 2] = ["lienhe@company.vn", "sales@company.vn"];

pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || c == '(' || c == '-')
        .filter(|w| !w.is_empty() && w.chars().next().is_some_and(char::is_alphabetic))
        .collect();
    let pick: Vec<&str> = match words.len() {
        0 => return "?".into(),
        1 => vec![words[0]],
        _ => vec![words[0], words[1]],
    };
    pick.iter()
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .collect()
}

/// Stable avatar palette index from a name (no randomness, screenshot-safe).
pub fn avatar_tone(name: &str) -> usize {
    name.bytes()
        .fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize))
        % 6
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_skip_parentheses_and_handle_unicode() {
        assert_eq!(initials("Trần Linh (Product)"), "TL");
        assert_eq!(initials("Nguyễn Văn Đức"), "NV");
        assert_eq!(initials("Kế toán"), "KT");
        assert_eq!(initials("Khách hàng - GreenTech"), "KH");
        assert_eq!(initials(""), "?");
    }
}
