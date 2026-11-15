//! Attachment file names are attacker-controlled (threat T06). The name a message suggests is
//! only ever used as a display label and as the last component of a path chosen by the app.

const MAX_CHARS: usize = 120;
const FALLBACK: &str = "attachment";
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Invisible characters that can disguise an extension (e.g. right-to-left override).
fn is_disguise(c: char) -> bool {
    matches!(c,
        '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}')
}

/// Reduce a suggested file name to a single, visible, portable path component.
/// Never returns an empty string, a path separator, a leading dot or a device name.
pub fn safe_filename(name: &str) -> String {
    // Only the last component counts, whichever separator the sender used.
    let last = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control() && !is_disguise(*c))
        .map(|c| match c {
            ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned
        .trim_start_matches(|c: char| c == '.' || c == '~' || c.is_whitespace())
        .trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        return FALLBACK.to_string();
    }
    let mut out = trimmed.to_string();
    if out.chars().count() > MAX_CHARS {
        // Keep a short extension so the file type stays recognisable.
        let ext: String = match out.rsplit_once('.') {
            Some((_, e)) if (1..=10).contains(&e.chars().count()) => format!(".{e}"),
            _ => String::new(),
        };
        let keep = MAX_CHARS - ext.chars().count();
        out = out.chars().take(keep).collect::<String>() + &ext;
    }
    let stem = out.split('.').next().unwrap_or("");
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        out.insert(0, '_');
    }
    out
}
