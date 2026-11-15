//! Pure parsers for the IMAP responses used by sync: LIST, EXAMINE status, FETCH items,
//! message headers (RFC 5322 unfolding, RFC 2047 encoded words) and dates. No I/O.

/// One server response line with its literals: `text {n}` + n bytes + more text ...
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Logical {
    pub texts: Vec<String>,
    pub literals: Vec<Vec<u8>>,
}

impl Logical {
    pub fn joined(&self) -> String {
        self.texts.join(" ")
    }
}

/// If `line` (without CRLF) ends with a literal marker `{n}` or `{n+}`, return n.
pub fn literal_len(line: &str) -> Option<usize> {
    let rest = line.strip_suffix('}')?;
    let open = rest.rfind('{')?;
    rest[open + 1..].trim_end_matches('+').parse().ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEntry {
    /// Name exactly as the server sent it (modified UTF-7), used in commands.
    pub name: String,
    pub delimiter: Option<String>,
    pub flags: Vec<String>,
}

impl ListEntry {
    pub fn selectable(&self) -> bool {
        !self.flags.iter().any(|f| {
            f.eq_ignore_ascii_case("\\Noselect") || f.eq_ignore_ascii_case("\\NonExistent")
        })
    }

    /// SPECIAL-USE role (RFC 6154), with INBOX recognised by name.
    pub fn special_use(&self) -> Option<&'static str> {
        if self.name.eq_ignore_ascii_case("INBOX") {
            return Some("inbox");
        }
        const ROLES: [(&str, &str); 6] = [
            ("\\Sent", "sent"),
            ("\\Drafts", "drafts"),
            ("\\Trash", "trash"),
            ("\\Junk", "junk"),
            ("\\Archive", "archive"),
            ("\\All", "all"),
        ];
        ROLES
            .iter()
            .find(|(flag, _)| self.flags.iter().any(|f| f.eq_ignore_ascii_case(flag)))
            .map(|(_, role)| *role)
    }
}

/// Read a quoted string starting at the opening quote; returns (value, bytes consumed).
fn quoted(s: &str) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut chars = s.char_indices();
    if chars.next()?.1 != '"' {
        return None;
    }
    let mut escaped = false;
    for (i, c) in chars {
        match (escaped, c) {
            (true, _) => {
                out.push(c);
                escaped = false;
            }
            (false, '\\') => escaped = true,
            (false, '"') => return Some((out, i + 1)),
            _ => out.push(c),
        }
    }
    None
}

/// Parse `* LIST (flags) "delim" name`, where name may be an atom, a quoted string or a literal.
pub fn parse_list(l: &Logical) -> Option<ListEntry> {
    let text = l.texts.first()?;
    let rest = text
        .strip_prefix("* LIST ")
        .or_else(|| text.strip_prefix("* LSUB "))?;
    let rest = rest.strip_prefix('(')?;
    let close = rest.find(')')?;
    let flags = rest[..close]
        .split_ascii_whitespace()
        .map(str::to_string)
        .collect();
    let rest = rest[close + 1..].trim_start();
    let (delimiter, rest) = if let Some(r) = rest.strip_prefix("NIL") {
        (None, r.trim_start())
    } else {
        let (d, used) = quoted(rest)?;
        (Some(d), rest[used..].trim_start())
    };
    let name = if literal_len(rest).is_some() {
        String::from_utf8_lossy(l.literals.first()?).to_string()
    } else if rest.starts_with('"') {
        quoted(rest)?.0
    } else {
        rest.trim().to_string()
    };
    if name.is_empty()
        || name.len() > 1024
        || name.chars().any(|c| c == '\r' || c == '\n' || c == '\0')
    {
        return None;
    }
    Some(ListEntry {
        name,
        delimiter,
        flags,
    })
}

/// Decode IMAP modified UTF-7 (RFC 3501 §5.1.3) for display. Invalid input is returned as is.
pub fn decode_mutf7(name: &str) -> String {
    let mut out = String::new();
    let mut rest = name;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('-') else {
            return name.to_string();
        };
        let chunk = &after[..end];
        if chunk.is_empty() {
            out.push('&');
        } else {
            let Some(bytes) = b64_decode(&chunk.replace(',', "/")) else {
                return name.to_string();
            };
            if bytes.len() % 2 != 0 {
                return name.to_string();
            }
            let units: Vec<u16> = bytes
                .chunks(2)
                .map(|p| u16::from_be_bytes([p[0], p[1]]))
                .collect();
            match String::from_utf16(&units) {
                Ok(s) => out.push_str(&s),
                Err(_) => return name.to_string(),
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn b64_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
    {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Find `KEY value` in a status/FETCH text; value ends at space, `)` or `]`.
fn number_after(text: &str, key: &str) -> Option<u64> {
    let upper = text.to_ascii_uppercase();
    let mut from = 0;
    while let Some(pos) = upper[from..].find(key) {
        let at = from + pos;
        let boundary = at == 0
            || !upper.as_bytes()[at - 1].is_ascii_alphanumeric()
                && upper.as_bytes()[at - 1] != b'.';
        let tail = text[at + key.len()..].trim_start();
        let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
        if boundary && !digits.is_empty() && text.as_bytes().get(at + key.len()) == Some(&b' ') {
            return digits.parse().ok();
        }
        from = at + key.len();
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MailboxStatus {
    pub exists: u32,
    pub uid_validity: Option<u64>,
    pub uid_next: Option<u64>,
}

/// Fold the untagged responses of EXAMINE into a status.
pub fn parse_examine(lines: &[String]) -> MailboxStatus {
    let mut status = MailboxStatus::default();
    for line in lines {
        let mut parts = line.split_ascii_whitespace();
        if let (Some("*"), Some(n), Some(word)) = (parts.next(), parts.next(), parts.next())
            && word.eq_ignore_ascii_case("EXISTS")
        {
            status.exists = n.parse().unwrap_or(0);
        }
        if let Some(v) = number_after(line, "UIDVALIDITY") {
            status.uid_validity = Some(v);
        }
        if let Some(v) = number_after(line, "UIDNEXT") {
            status.uid_next = Some(v);
        }
    }
    status
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Envelope {
    pub uid: u32,
    pub seen: bool,
    pub flagged: bool,
    /// Seconds since the Unix epoch (from INTERNALDATE), 0 when unparsable.
    pub internal_date: i64,
    pub size_bytes: u64,
    pub subject: String,
    pub from_display: String,
    pub from_address: String,
    pub message_id: Option<String>,
    /// Raw `In-Reply-To` / `References` values (single line, bounded); parsed into ids by the core.
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    /// Decoded `To` / `Cc` display text (single line, bounded).
    pub to: String,
    pub cc: String,
}

/// Parse one `* n FETCH (...)` response whose header block arrived as a literal.
pub fn parse_fetch(l: &Logical) -> Option<Envelope> {
    let text = l.joined();
    if !text.to_ascii_uppercase().contains(" FETCH (") {
        return None;
    }
    let uid = u32::try_from(number_after(&text, "UID")?).ok()?;
    let flags = text
        .find("FLAGS (")
        .and_then(|p| {
            text[p + 7..]
                .find(')')
                .map(|e| text[p + 7..p + 7 + e].to_string())
        })
        .unwrap_or_default();
    let has = |f: &str| {
        flags
            .split_ascii_whitespace()
            .any(|x| x.eq_ignore_ascii_case(f))
    };
    let internal_date = text
        .find("INTERNALDATE \"")
        .and_then(|p| text[p + 14..].find('"').map(|e| &text[p + 14..p + 14 + e]))
        .and_then(parse_internal_date)
        .unwrap_or(0);
    let headers = l
        .literals
        .first()
        .map(|h| parse_headers(h))
        .unwrap_or_default();
    let get = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    };
    let (from_display, from_address) = parse_from(&decode_words(&get("From").unwrap_or_default()));
    Some(Envelope {
        uid,
        seen: has("\\Seen"),
        flagged: has("\\Flagged"),
        internal_date,
        size_bytes: number_after(&text, "RFC822.SIZE").unwrap_or(0),
        subject: clean(&decode_words(&get("Subject").unwrap_or_default()), 500),
        from_display: clean(&from_display, 200),
        from_address: clean(&from_address, 320),
        message_id: get("Message-ID")
            .map(|v| clean(&v, 500))
            .filter(|v| !v.is_empty()),
        in_reply_to: get("In-Reply-To")
            .map(|v| clean(&v, 1_000))
            .filter(|v| !v.is_empty()),
        references: get("References")
            .map(|v| clean(&v, 16_000))
            .filter(|v| !v.is_empty()),
        to: clean(&decode_words(&get("To").unwrap_or_default()), 2_000),
        cc: clean(&decode_words(&get("Cc").unwrap_or_default()), 2_000),
    })
}

/// Single-line, bounded text safe to store and show.
fn clean(s: &str, max: usize) -> String {
    let flat: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    flat.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}

/// Unfold and split a header block into (name, value) pairs.
pub fn parse_headers(raw: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(raw);
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.starts_with([' ', '\t']) {
            if let Some(last) = out.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim_start());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            out.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    out
}

fn decode_bytes(charset: &str, bytes: &[u8]) -> Option<String> {
    match charset.to_ascii_lowercase().as_str() {
        "utf-8" | "utf8" | "us-ascii" | "ascii" => Some(String::from_utf8_lossy(bytes).to_string()),
        // Latin-1 maps bytes 1:1 to code points; windows-1252 differs only in 0x80-0x9F.
        "iso-8859-1" | "latin1" | "windows-1252" | "cp1252" => {
            Some(bytes.iter().map(|&b| b as char).collect())
        }
        _ => None,
    }
}

/// Decode RFC 2047 encoded words (`=?charset?B|Q?text?=`). Unknown charsets are left as is.
pub fn decode_words(input: &str) -> String {
    let mut out = String::new();
    let mut rest = input;
    let mut last_was_word = false;
    while let Some(start) = rest.find("=?") {
        let before = &rest[..start];
        let decoded = (|| {
            let body = &rest[start + 2..];
            let (charset, body) = body.split_once('?')?;
            let (encoding, body) = body.split_once('?')?;
            let end = body.find("?=")?;
            let payload = &body[..end];
            let bytes = match encoding {
                "B" | "b" => b64_decode(payload)?,
                "Q" | "q" => {
                    let mut v = Vec::new();
                    let p = payload.as_bytes();
                    let mut i = 0;
                    while i < p.len() {
                        match p[i] {
                            b'_' => v.push(b' '),
                            b'=' => {
                                let hex = std::str::from_utf8(p.get(i + 1..i + 3)?).ok()?;
                                v.push(u8::from_str_radix(hex, 16).ok()?);
                                i += 2;
                            }
                            b => v.push(b),
                        }
                        i += 1;
                    }
                    v
                }
                _ => return None,
            };
            let consumed = start + 2 + charset.len() + 1 + encoding.len() + 1 + end + 2;
            Some((decode_bytes(charset, &bytes)?, consumed))
        })();
        match decoded {
            Some((text, consumed)) => {
                // Whitespace between two encoded words is not part of the text.
                if !(last_was_word && before.trim().is_empty()) {
                    out.push_str(before);
                }
                out.push_str(&text);
                rest = &rest[consumed..];
                last_was_word = true;
            }
            None => {
                out.push_str(&rest[..start + 2]);
                rest = &rest[start + 2..];
                last_was_word = false;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Split a From value into (display name, address).
pub fn parse_from(value: &str) -> (String, String) {
    let v = value.trim();
    if let (Some(open), Some(close)) = (v.rfind('<'), v.rfind('>'))
        && close > open
    {
        let address = v[open + 1..close].trim().to_string();
        let name = v[..open].trim().trim_matches('"').trim().to_string();
        return (
            if name.is_empty() {
                address.clone()
            } else {
                name
            },
            address,
        );
    }
    (v.to_string(), v.to_string())
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// `14-Nov-2026 10:05:00 +0700` → Unix seconds (UTC).
pub fn parse_internal_date(s: &str) -> Option<i64> {
    let mut parts = s.split_ascii_whitespace();
    let (date, time, zone) = (
        parts.next()?,
        parts.next()?,
        parts.next().unwrap_or("+0000"),
    );
    let mut d = date.split('-');
    let day: i64 = d.next()?.trim().parse().ok()?;
    let month = 1 + [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ]
    .iter()
    .position(|m| d.clone().next().is_some_and(|x| x.eq_ignore_ascii_case(m)))?
        as i64;
    let year: i64 = d.nth(1)?.parse().ok()?;
    let mut t = time.split(':');
    let (h, mi, sec): (i64, i64, i64) = (
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
    );
    // Server-supplied: keep every field in range so nothing can overflow (MB-304).
    if !(1..=31).contains(&day)
        || !(1000..=9999).contains(&year)
        || !(0..=23).contains(&h)
        || !(0..=59).contains(&mi)
        || !(0..=60).contains(&sec)
    {
        return None;
    }
    let sign = if zone.starts_with('-') { -1 } else { 1 };
    let digits = zone.trim_start_matches(['+', '-']);
    let offset = if digits.len() == 4 && digits.bytes().all(|b| b.is_ascii_digit()) {
        let (zh, zm) = (
            digits[..2].parse::<i64>().ok()?,
            digits[2..].parse::<i64>().ok()?,
        );
        if zh > 23 || zm > 59 {
            return None;
        }
        zh * 3600 + zm * 60
    } else if digits.len() == 4 {
        return None;
    } else {
        0
    };
    Some(days_from_civil(year, month, day) * 86_400 + h * 3600 + mi * 60 + sec - sign * offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn logical(texts: &[&str], literals: &[&str]) -> Logical {
        Logical {
            texts: texts.iter().map(|s| s.to_string()).collect(),
            literals: literals.iter().map(|s| s.as_bytes().to_vec()).collect(),
        }
    }

    #[test]
    fn literal_markers() {
        assert_eq!(literal_len("* 1 FETCH (BODY[] {42}"), Some(42));
        assert_eq!(literal_len("a LOGIN {5+}"), Some(5));
        assert_eq!(literal_len("* OK done"), None);
    }

    #[test]
    fn list_variants_and_special_use() {
        let e = parse_list(&logical(
            &[r#"* LIST (\HasNoChildren \Sent) "/" "Sent Items""#],
            &[],
        ))
        .unwrap();
        assert_eq!(
            (e.name.as_str(), e.delimiter.as_deref(), e.special_use()),
            ("Sent Items", Some("/"), Some("sent"))
        );
        let e = parse_list(&logical(&[r#"* LIST (\HasNoChildren) "." INBOX"#], &[])).unwrap();
        assert_eq!(
            (e.name.as_str(), e.special_use(), e.selectable()),
            ("INBOX", Some("inbox"), true)
        );
        let e = parse_list(&logical(
            &[r#"* LIST (\Noselect \HasChildren) NIL "[Gmail]""#],
            &[],
        ))
        .unwrap();
        assert!(!e.selectable() && e.delimiter.is_none());
        let e = parse_list(&logical(&[r#"* LIST () "/" {11}"#, ""], &["Quote\"d box"])).unwrap();
        assert_eq!(e.name, "Quote\"d box");
        let e = parse_list(&logical(&[r#"* LIST () "/" "a \"b\" c""#], &[])).unwrap();
        assert_eq!(e.name, "a \"b\" c");
        assert!(parse_list(&logical(&["* 3 EXISTS"], &[])).is_none());
    }

    #[test]
    fn modified_utf7_names() {
        assert_eq!(decode_mutf7("INBOX"), "INBOX");
        assert_eq!(decode_mutf7("Th&AbA- &AREA4w- g&Hu0-i"), "Thư đã gửi");
        assert_eq!(decode_mutf7("A&-B"), "A&B");
        assert_eq!(decode_mutf7("broken&abc"), "broken&abc");
    }

    #[test]
    fn examine_status() {
        let lines: Vec<String> = [
            "* 42 EXISTS",
            "* 0 RECENT",
            "* OK [UIDVALIDITY 1699999999] UIDs valid",
            "* OK [UIDNEXT 120] Predicted next UID",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            parse_examine(&lines),
            MailboxStatus {
                exists: 42,
                uid_validity: Some(1_699_999_999),
                uid_next: Some(120)
            }
        );
    }

    #[test]
    fn fetch_with_header_literal() {
        let headers = "From: =?UTF-8?B?VHLhuqduIExpbmg=?= <linh.tran@company.vn>\r\nSubject: =?UTF-8?Q?K=E1=BA=BF_ho=E1=BA=A1ch?=\r\n =?UTF-8?Q?_ra_m=E1=BA=AFt?=\r\nMessage-ID: <abc@company.vn>\r\n\r\n";
        let l = logical(
            &[
                r#"* 7 FETCH (UID 118 FLAGS (\Seen \Flagged) INTERNALDATE "14-Nov-2026 10:05:00 +0700" RFC822.SIZE 20480 BODY[HEADER.FIELDS (FROM SUBJECT MESSAGE-ID)] {200}"#,
                ")",
            ],
            &[headers],
        );
        let e = parse_fetch(&l).unwrap();
        assert_eq!(e.uid, 118);
        assert!(e.seen && e.flagged);
        assert_eq!(e.size_bytes, 20_480);
        assert_eq!(e.subject, "Kế hoạch ra mắt");
        assert_eq!(
            (e.from_display.as_str(), e.from_address.as_str()),
            ("Trần Linh", "linh.tran@company.vn")
        );
        assert_eq!(e.message_id.as_deref(), Some("<abc@company.vn>"));
        assert_eq!(e.internal_date, 1_794_625_500);
    }

    #[test]
    fn fetch_item_order_and_missing_headers_do_not_matter() {
        let l = logical(
            &[
                "* 1 FETCH (RFC822.SIZE 10 BODY[HEADER.FIELDS (FROM)] {0}",
                " UID 5 FLAGS ())",
            ],
            &[""],
        );
        let e = parse_fetch(&l).unwrap();
        assert_eq!((e.uid, e.seen, e.size_bytes), (5, false, 10));
        assert!(e.subject.is_empty() && e.message_id.is_none());
        assert!(parse_fetch(&logical(&["* 3 EXISTS"], &[])).is_none());
    }

    #[test]
    fn encoded_words_and_hostile_headers() {
        assert_eq!(decode_words("=?iso-8859-1?Q?Caf=E9?= menu"), "Café menu");
        assert_eq!(decode_words("=?UTF-8?B?4pyT?= =?UTF-8?B?IG9r?="), "✓ ok");
        assert_eq!(decode_words("=?x-unknown?B?AAAA?="), "=?x-unknown?B?AAAA?=");
        assert_eq!(decode_words("plain =? not a word"), "plain =? not a word");
        // Control characters and line breaks never survive into stored text.
        let l = logical(
            &["* 1 FETCH (UID 1 BODY[HEADER.FIELDS (SUBJECT)] {30}", ")"],
            &["Subject: a\u{0007}b\r\n\tc\u{0000}d\r\n\r\n"],
        );
        assert_eq!(parse_fetch(&l).unwrap().subject, "a b c d");
    }

    #[test]
    fn from_forms_and_dates() {
        assert_eq!(
            parse_from("\"Kế toán\" <ketoan@company.vn>"),
            ("Kế toán".into(), "ketoan@company.vn".into())
        );
        assert_eq!(parse_from("<a@b.vn>"), ("a@b.vn".into(), "a@b.vn".into()));
        assert_eq!(parse_from("a@b.vn"), ("a@b.vn".into(), "a@b.vn".into()));
        assert_eq!(parse_internal_date("01-Jan-1970 00:00:00 +0000"), Some(0));
        assert_eq!(
            parse_internal_date("29-Feb-2000 23:59:59 -0500"),
            Some(951_886_799)
        );
        assert_eq!(parse_internal_date("garbage"), None);
    }

    /// MB-304: INTERNALDATE comes from the server. A multi-byte zone or out-of-range numbers
    /// used to panic (string slice off a char boundary, integer overflow) and stop the sync.
    #[test]
    fn hostile_internal_dates_are_refused_without_panicking() {
        for bad in [
            "14-Nov-2026 10:05:00 +1\u{e9}1",
            "14-Nov-2026 10:05:00 +\u{e9}\u{e9}",
            "14-Nov-9223372036854775807 10:05:00 +0000",
            "14-Nov-2026 9223372036854775807:00:00 +0000",
            "9223372036854775807-Nov-2026 10:05:00 +0000",
            "14-Nov-2026 10:05:-9223372036854775808 +0000",
            "14-Nov-2026 25:00:00 +0000",
            "32-Nov-2026 10:00:00 +0000",
            "14-Nov-2026 10:05:00 +9960",
        ] {
            assert_eq!(parse_internal_date(bad), None, "{bad:?}");
        }
        assert_eq!(
            parse_internal_date("14-Nov-2026 10:05:00 +0700"),
            Some(1_794_625_500)
        );
    }
}
