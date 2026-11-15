//! Transfer and header encodings: base64, quoted-printable, RFC 2047 words, header folding.

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
/// Soft limit for generated lines (RFC 5322 recommends 78, RFC 2045 requires 76 for encodings).
pub const HEADER_LINE: usize = 78;
const ENCODED_LINE: usize = 76;
/// UTF-8 bytes per RFC 2047 word: 39 bytes -> 52 base64 chars -> 64-char word, well under 75.
const WORD_BYTES: usize = 39;

pub fn base64(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        out.push(B64[(b[0] >> 2) as usize] as char);
        out.push(B64[(((b[0] & 3) << 4) | (b[1] >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            B64[(((b[1] & 15) << 2) | (b[2] >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[(b[2] & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Base64 in 76-column lines, each ending with CRLF.
pub fn base64_lines(input: &[u8]) -> String {
    let encoded = base64(input);
    let mut out = String::with_capacity(encoded.len() + encoded.len() / ENCODED_LINE * 2 + 2);
    for chunk in encoded.as_bytes().chunks(ENCODED_LINE) {
        // Base64 output is ASCII, so any byte chunk is valid UTF-8.
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        out.push_str("\r\n");
    }
    out
}

/// Quoted-printable for text whose line endings are already CRLF. Lines stay within 76 columns.
pub fn quoted_printable(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    let mut lines = text.split("\r\n").peekable();
    while let Some(line) = lines.next() {
        let bytes = line.as_bytes();
        let mut column = 0;
        for (i, &b) in bytes.iter().enumerate() {
            let last = i + 1 == bytes.len();
            let literal =
                (b == b' ' || b == b'\t') && !last || (33..=126).contains(&b) && b != b'=';
            let width = if literal { 1 } else { 3 };
            // Keep one column for the soft line break "=".
            if column + width > ENCODED_LINE - 1 {
                out.push_str("=\r\n");
                column = 0;
            }
            if literal {
                out.push(b as char);
            } else {
                out.push_str(&format!("={b:02X}"));
            }
            column += width;
        }
        if lines.peek().is_some() {
            out.push_str("\r\n");
        }
    }
    out
}

/// RFC 2047 `=?UTF-8?B?...?=` words covering `text`, split on character boundaries.
pub fn encoded_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + WORD_BYTES).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        words.push(format!(
            "=?UTF-8?B?{}?=",
            base64(&text.as_bytes()[start..end])
        ));
        start = end;
    }
    words
}

/// Tokens for an unstructured header value (Subject): plain words when that is safe and exact,
/// encoded words otherwise.
pub fn unstructured(text: &str) -> Vec<String> {
    let plain = text.is_ascii()
        && !text.contains("=?")
        && !text.contains("  ")
        && !text.contains('\t')
        && text.trim() == text
        && text.split(' ').all(|w| w.len() <= 60);
    if text.is_empty() {
        Vec::new()
    } else if plain {
        text.split(' ').map(str::to_string).collect()
    } else {
        encoded_words(text)
    }
}

/// Tokens for a display name: atoms, a quoted string, or encoded words.
pub fn phrase(name: &str) -> Vec<String> {
    const ATEXT: &str = "!#$%&'*+-/=?^_`{|}~";
    let atoms = name.is_ascii()
        && !name.contains("=?")
        && name.trim() == name
        && !name.contains("  ")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ' || ATEXT.contains(c));
    if name.is_empty() {
        Vec::new()
    } else if atoms {
        name.split(' ').map(str::to_string).collect()
    } else if name.is_ascii() && name.len() <= 60 && !name.contains("=?") {
        vec![format!(
            "\"{}\"",
            name.replace('\\', "\\\\").replace('"', "\\\"")
        )]
    } else {
        encoded_words(name)
    }
}

/// `Name: token token ...` folded so lines stay within 78 columns where tokens allow it.
pub fn fold(name: &str, tokens: &[String]) -> String {
    let mut out = format!("{name}:");
    let mut column = out.len();
    for (i, token) in tokens.iter().enumerate() {
        if i > 0 && column + 1 + token.len() > HEADER_LINE {
            out.push_str("\r\n ");
            column = 1;
        } else {
            out.push(' ');
            column += 1;
        }
        out.push_str(token);
        column += token.len();
    }
    out.push_str("\r\n");
    out
}

/// RFC 2231 percent-encoding of a parameter value (UTF-8).
pub fn percent_encode(value: &str) -> Vec<String> {
    const ATTR: &str = "!#$&+-.^_`|~";
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || ATTR.contains(b as char) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (i, o) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(i.as_bytes()), o);
        }
        assert_eq!(
            base64_lines(&[0u8; 58])
                .lines()
                .map(str::len)
                .collect::<Vec<_>>(),
            vec![76, 4]
        );
    }

    #[test]
    fn quoted_printable_basics() {
        assert_eq!(quoted_printable("a=b\r\n"), "a=3Db\r\n");
        assert_eq!(quoted_printable("end \r\nx"), "end=20\r\nx");
        assert_eq!(quoted_printable("é"), "=C3=A9");
        let long = quoted_printable(&"a".repeat(200));
        assert!(long.split("\r\n").all(|l| l.len() <= 76));
        assert_eq!(long.replace("=\r\n", ""), "a".repeat(200));
    }

    #[test]
    fn folding_keeps_the_first_token_on_the_header_line() {
        let tokens: Vec<String> = (0..4).map(|_| "x".repeat(30)).collect();
        let folded = fold("Subject", &tokens);
        assert!(folded.starts_with("Subject: xxx"));
        assert_eq!(folded.matches("\r\n ").count(), 1);
        assert_eq!(fold("Subject", &[]), "Subject:\r\n");
    }
}
