//! Links inside received mail (AUDIT S-09).
//!
//! A link in a message must never open by itself: the visible text can say one site while the
//! address points to another. Two steps make that impossible:
//!
//! 1. [`guard_links`] rewrites every `href` of the (already sanitised) mail HTML to the inert
//!    `mailbox-link:` scheme before it is shown. No browser, WebView or operating system opens
//!    that scheme, so a click cannot leave MailBox on any platform.
//! 2. The Desktop shell sees the click as a blocked navigation, [`unguard`]s the address and
//!    [`inspect`]s it. The user is then asked, with the real host and the full address on
//!    screen, before anything is opened.
//!
//! The address that is shown is the address that is opened: both are the parsed, ASCII-only
//! form, so right-to-left overrides and look-alike Unicode cannot hide in it.

/// Scheme of a guarded link. The rest of the address is the original, hex encoded.
pub const GUARD_SCHEME: &str = "mailbox-link:";
/// Longest address accepted, in bytes.
pub const MAX_URL: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// `http` or `https`: opens in the web browser.
    Web,
    /// `mailto`: opens the default mail program.
    Mail,
}

/// A link that may be offered to the user. `url` is what is displayed and what is opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkTarget {
    pub kind: LinkKind,
    /// Parsed, normalised, ASCII-only address.
    pub url: String,
    /// Web: the real host (with a non-default port). Mail: the recipient part.
    pub host: String,
    /// `false` for plain `http`.
    pub secure: bool,
    /// The host has an internationalised (`xn--`) label, which can imitate a familiar name.
    pub punycode: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkRefusal {
    /// Not a usable address (or not a guarded link at all).
    Malformed,
    /// A scheme other than `http`, `https`, `mailto`.
    Scheme,
    /// `https://name@host/`: a user name before the host, a classic way to fake the site name.
    Credentials,
    TooLong,
}

/// Rewrite every `href` in sanitised mail HTML to the inert guard scheme.
///
/// The input comes from the sanitiser, which writes well-formed tags with double-quoted values.
/// The scanner still accepts single-quoted and unquoted values and any element name, so an
/// unexpected shape loses its link instead of keeping a live one. Text between tags is copied
/// unchanged.
pub fn guard_links(html: &str) -> String {
    let b = html.as_bytes();
    let space = |c: u8| c.is_ascii_whitespace();
    let mut out = String::with_capacity(html.len() + 64);
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' || !b.get(i + 1).is_some_and(u8::is_ascii_alphabetic) {
            // Text, a closing tag or a stray `<`: copy up to the next `<`.
            // Searched as bytes: `i + 1` may be inside a multi-byte character.
            let next = b[i + 1..]
                .iter()
                .position(|&c| c == b'<')
                .map_or(b.len(), |n| i + 1 + n);
            out.push_str(&html[i..next]);
            i = next;
            continue;
        }
        let name_end = (i + 1..b.len())
            .find(|&j| space(b[j]) || b[j] == b'>' || b[j] == b'/')
            .unwrap_or(b.len());
        out.push_str(&html[i..name_end]);
        i = name_end;
        // Attributes, up to and including the closing `>`.
        while i < b.len() {
            if space(b[i]) || b[i] == b'/' || b[i] == b'=' {
                out.push(b[i] as char);
                i += 1;
                continue;
            }
            if b[i] == b'>' {
                out.push('>');
                i += 1;
                break;
            }
            let start = i;
            while i < b.len() && !space(b[i]) && !matches!(b[i], b'=' | b'>' | b'/') {
                i += 1;
            }
            let name = &html[start..i];
            out.push_str(name);
            let mut k = i;
            while k < b.len() && space(b[k]) {
                k += 1;
            }
            if b.get(k) != Some(&b'=') {
                continue;
            }
            k += 1;
            while k < b.len() && space(b[k]) {
                k += 1;
            }
            let (value, end) = match b.get(k) {
                Some(&q) if q == b'"' || q == b'\'' => {
                    let close = html[k + 1..].find(q as char).map_or(b.len(), |n| k + 1 + n);
                    (&html[k + 1..close], (close + 1).min(b.len()))
                }
                _ => {
                    let close = (k..b.len())
                        .find(|&j| space(b[j]) || b[j] == b'>')
                        .unwrap_or(b.len());
                    (&html[k..close], close)
                }
            };
            if name.eq_ignore_ascii_case("href") {
                out.push_str("=\"");
                out.push_str(GUARD_SCHEME);
                for byte in unescape(value).bytes() {
                    out.push(HEX[usize::from(byte >> 4)] as char);
                    out.push(HEX[usize::from(byte & 15)] as char);
                }
                out.push('"');
            } else {
                out.push_str(&html[i..end]);
            }
            i = end;
        }
    }
    out
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// Decode the character references an HTML serialiser writes inside attribute values.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let decoded = rest[1..]
            .find(';')
            .filter(|&n| n <= 8)
            .and_then(|n| Some((n, entity(&rest[1..1 + n])?)));
        match decoded {
            Some((n, c)) => {
                out.push(c);
                rest = &rest[n + 2..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "nbsp" => Some('\u{a0}'),
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

/// The original address of a guarded link, or `None` when `navigation` is not one.
pub fn unguard(navigation: &str) -> Option<String> {
    let hex = navigation.strip_prefix(GUARD_SCHEME)?.as_bytes();
    if hex.is_empty() || hex.len() % 2 != 0 || hex.len() > 2 * MAX_URL {
        return None;
    }
    let digit = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let bytes = hex
        .chunks(2)
        .map(|pair| Some(digit(pair[0])? << 4 | digit(pair[1])?))
        .collect::<Option<Vec<u8>>>()?;
    String::from_utf8(bytes).ok()
}

/// Decide whether an address from a message may be offered to the user.
pub fn inspect(url: &str) -> Result<LinkTarget, LinkRefusal> {
    if url.len() > MAX_URL {
        return Err(LinkRefusal::TooLong);
    }
    // Checked before parsing: a URL parser silently drops tabs and line breaks.
    if url.is_empty() || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(LinkRefusal::Malformed);
    }
    let scheme = url.split_once(':').map(|(s, _)| s).unwrap_or("");
    let well_formed = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !well_formed {
        return Err(LinkRefusal::Malformed);
    }
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https" | "mailto") {
        return Err(LinkRefusal::Scheme);
    }
    let parsed = url::Url::parse(url).map_err(|_| LinkRefusal::Malformed)?;
    if scheme == "mailto" {
        let to = parsed.path();
        if to.is_empty() {
            return Err(LinkRefusal::Malformed);
        }
        return Ok(LinkTarget {
            kind: LinkKind::Mail,
            url: parsed.as_str().to_string(),
            host: to.to_string(),
            secure: true,
            punycode: false,
        });
    }
    let host = parsed
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or(LinkRefusal::Malformed)?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(LinkRefusal::Credentials);
    }
    Ok(LinkTarget {
        kind: LinkKind::Web,
        url: parsed.as_str().to_string(),
        host: match parsed.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        },
        secure: scheme == "https",
        punycode: host.split('.').any(|label| label.starts_with("xn--")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `href` value in `html`, as written.
    fn hrefs(html: &str) -> Vec<String> {
        html.split("href=\"")
            .skip(1)
            .map(|rest| rest[..rest.find('"').unwrap()].to_string())
            .collect()
    }

    #[test]
    fn text_with_multi_byte_characters_is_copied_unchanged() {
        let html = "Đây là thư <b>có dấu</b> ✓ <a href=\"https://example.org/đường\">Mở liên kết</a> hết — 終";
        let out = guard_links(html);
        assert!(out.starts_with("Đây là thư <b>có dấu</b> ✓ <a href=\"mailbox-link:"));
        assert!(out.ends_with("\">Mở liên kết</a> hết — 終"));
        assert!(!out.contains("https://"));
        assert_eq!(guard_links("é"), "é");
        assert_eq!(guard_links("<é"), "<é");
        assert_eq!(guard_links("a<"), "a<");
    }

    #[test]
    fn every_link_is_rewritten_to_the_inert_scheme_and_decodes_to_the_original() {
        let html = "<p>Xin chào <a href=\"https://example.com/a?x=1&amp;y=2\" rel=\"noopener noreferrer\">bấm vào đây</a> và <a href=\"mailto:linh@company.vn\">gửi thư</a>.</p>";
        let guarded = guard_links(html);
        let links = hrefs(&guarded);
        assert_eq!(links.len(), 2);
        assert!(
            links.iter().all(|l| l.starts_with(GUARD_SCHEME)),
            "{guarded}"
        );
        assert_eq!(
            unguard(&links[0]).as_deref(),
            Some("https://example.com/a?x=1&y=2"),
            "entities are decoded before encoding"
        );
        assert_eq!(
            unguard(&links[1]).as_deref(),
            Some("mailto:linh@company.vn")
        );
        for leftover in ["https://", "http://", "mailto:", "example.com"] {
            assert!(!guarded.contains(leftover), "{leftover} left in {guarded}");
        }
        assert!(guarded.contains("rel=\"noopener noreferrer\">bấm vào đây</a>"));
    }

    #[test]
    fn odd_markup_cannot_keep_a_live_link() {
        // Single quotes, no quotes, upper case, a `>` inside a value, href on another element.
        let html = "<a href='https://one.example/'>1</a><a href=https://two.example/>2</a><A HREF=\"https://three.example/\">3</A><a name=\"a>b\" href=\"https://four.example/?q=>\">4</a><area href=\"https://five.example/\">";
        let guarded = guard_links(html);
        assert!(!guarded.contains(".example"), "{guarded}");
        assert_eq!(guarded.matches(GUARD_SCHEME).count(), 5, "{guarded}");
        assert!(guarded.contains(">1</a>") && guarded.contains(">4</a>"));
    }

    #[test]
    fn text_that_looks_like_markup_is_left_alone() {
        let html = "<p>Viết href=\"https://example.com\" trong mã, và &lt;a href=\"x\"&gt;.</p>";
        assert_eq!(guard_links(html), html);
        assert_eq!(guard_links(""), "");
        assert_eq!(guard_links("a < b, c > d"), "a < b, c > d");
    }

    #[test]
    fn only_a_well_formed_guarded_link_decodes() {
        let guarded = guard_links("<a href=\"https://example.com/\">x</a>");
        let link = &hrefs(&guarded)[0];
        assert_eq!(unguard(link).as_deref(), Some("https://example.com/"));
        for bad in [
            "https://example.com/",
            "mailbox-link:",
            "mailbox-link:zz",
            "mailbox-link:616",
            "mailbox-link:ff",
            "about:srcdoc",
        ] {
            assert_eq!(unguard(bad), None, "{bad}");
        }
        let long = format!("{GUARD_SCHEME}{}", "61".repeat(MAX_URL + 1));
        assert_eq!(unguard(&long), None);
    }

    #[test]
    fn a_web_link_shows_its_real_host() {
        let t = inspect("https://Example.COM/path?q=1#top").unwrap();
        assert_eq!((t.kind, t.host.as_str()), (LinkKind::Web, "example.com"));
        assert!(t.secure && !t.punycode);
        assert_eq!(t.url, "https://example.com/path?q=1#top");

        let plain = inspect("http://example.com:8080/").unwrap();
        assert_eq!(plain.host, "example.com:8080");
        assert!(!plain.secure);

        let mail = inspect("mailto:linh@company.vn?subject=Hi").unwrap();
        assert_eq!(
            (mail.kind, mail.host.as_str()),
            (LinkKind::Mail, "linh@company.vn")
        );
    }

    #[test]
    fn look_alike_and_hidden_characters_cannot_survive_in_what_is_shown() {
        // Cyrillic "а" in "аpple.com".
        let idn = inspect("https://\u{430}pple.com/").unwrap();
        assert!(idn.punycode && idn.host.starts_with("xn--"), "{}", idn.host);
        // A right-to-left override in the path.
        let rtl = inspect("https://example.com/\u{202e}gpj.exe").unwrap();
        for t in [&idn, &rtl] {
            assert!(t.url.is_ascii() && t.host.is_ascii(), "{}", t.url);
            assert!(!t.url.bytes().any(|b| b.is_ascii_control() || b == b' '));
        }
    }

    #[test]
    fn deceptive_and_unsupported_addresses_are_refused() {
        for (url, why) in [
            (
                "https://bank.example@evil.example/",
                LinkRefusal::Credentials,
            ),
            ("https://user:pw@evil.example/", LinkRefusal::Credentials),
            ("javascript:alert(1)", LinkRefusal::Scheme),
            ("file:///etc/passwd", LinkRefusal::Scheme),
            ("data:text/html,x", LinkRefusal::Scheme),
            ("mailbox-link:61", LinkRefusal::Scheme),
            ("tel:+84123", LinkRefusal::Scheme),
            ("", LinkRefusal::Malformed),
            ("example.com/no-scheme", LinkRefusal::Malformed),
            ("https://", LinkRefusal::Malformed),
            ("https://exa mple.com/", LinkRefusal::Malformed),
            ("https://example.com/\r\nx", LinkRefusal::Malformed),
            ("mailto:", LinkRefusal::Malformed),
        ] {
            assert_eq!(inspect(url), Err(why), "{url:?}");
        }
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL));
        assert_eq!(inspect(&long), Err(LinkRefusal::TooLong));
    }

    #[test]
    fn a_guarded_link_round_trips_to_the_same_decision() {
        let guarded =
            guard_links("<a href=\"https://bank.example@evil.example/login\">Ngân hàng</a>");
        let original = unguard(&hrefs(&guarded)[0]).unwrap();
        assert_eq!(inspect(&original), Err(LinkRefusal::Credentials));
    }
}
