//! Parser behaviour on ordinary and hostile messages. All fixtures are synthetic.
use mailbox_mime::{Limits, MimeError, attachment_bytes, parse};

fn crlf(s: &str) -> Vec<u8> {
    s.replace("\r\n", "\n").replace('\n', "\r\n").into_bytes()
}

const ALTERNATIVE: &str = "From: =?UTF-8?B?VHLhuqduIExpbmg=?= <linh@example.vn>
To: Minh <minh@example.vn>, team@example.vn
Cc: \"Lan, Pham\" <lan@example.vn>
Subject: =?UTF-8?Q?B=C3=A1o_c=C3=A1o_qu=C3=BD?=
Date: Sat, 14 Nov 2026 10:05:00 +0700
Message-ID: <abc@example.vn>
MIME-Version: 1.0
Content-Type: multipart/alternative; boundary=\"alt\"

--alt
Content-Type: text/plain; charset=utf-8
Content-Transfer-Encoding: quoted-printable

Xin ch=C3=A0o,=20
d=C3=B2ng hai.
--alt
Content-Type: text/html; charset=utf-8
Content-Transfer-Encoding: base64

PHA+WGluIDxiPmNow6BvPC9iPjwvcD4=
--alt--
";

#[test]
fn alternative_prefers_plain_and_keeps_html() {
    let m = parse(&crlf(ALTERNATIVE), &Limits::default()).unwrap();
    assert_eq!(m.headers.subject, "Báo cáo quý");
    assert_eq!(m.headers.from[0].name, "Trần Linh");
    assert_eq!(m.headers.from[0].address, "linh@example.vn");
    assert_eq!(m.headers.to.len(), 2);
    assert_eq!(m.headers.cc[0].name, "Lan, Pham");
    assert_eq!(m.headers.date, Some(1_794_625_500));
    assert_eq!(m.headers.message_id.as_deref(), Some("abc@example.vn"));
    assert!(m.text_is_plain);
    assert!(m.text.contains("Xin chào,") && m.text.contains("dòng hai."));
    assert_eq!(m.html.as_deref(), Some("<p>Xin <b>chào</b></p>"));
    assert!(!m.remote_blocked && !m.truncated);
    assert!(m.attachments.is_empty());
}

#[test]
fn html_only_message_gets_derived_text() {
    let raw = crlf(
        "Subject: x\nContent-Type: text/html; charset=utf-8\n\n<html><body><p>Hello <b>world</b></p><script>alert(1)</script></body></html>\n",
    );
    let m = parse(&raw, &Limits::default()).unwrap();
    assert!(!m.text_is_plain);
    assert!(m.text.contains("Hello") && m.text.contains("world"));
    let html = m.html.unwrap();
    assert!(html.contains("<b>world</b>") && !html.contains("script") && !html.contains("alert"));
}

#[test]
fn common_charsets_decode() {
    // 0xE9 = é in ISO-8859-1; 0x93/0x94 = curly quotes in windows-1252; windows-1258: 0xF0 = đ.
    for (charset, bytes, expected) in [
        ("iso-8859-1", &b"caf\xE9"[..], "café"),
        ("windows-1252", &b"\x93hi\x94"[..], "\u{201c}hi\u{201d}"),
        ("windows-1258", &b"\xF0i"[..], "đi"),
        ("utf-8", "Tiếng Việt".as_bytes(), "Tiếng Việt"),
    ] {
        let mut raw = format!("Subject: s\r\nContent-Type: text/plain; charset={charset}\r\n\r\n")
            .into_bytes();
        raw.extend_from_slice(bytes);
        let m = parse(&raw, &Limits::default()).unwrap();
        assert_eq!(m.text.trim(), expected, "{charset}");
    }
}

#[test]
fn unknown_charset_and_broken_encodings_never_panic() {
    let raw = b"Subject: =?x-nope?B?!!!!?= =?UTF-8?B?4pyT\r\nContent-Type: text/plain; charset=x-klingon\r\nContent-Transfer-Encoding: base64\r\n\r\n@@@not base64@@@ \xff\xfe SGVsbG8=\r\n";
    let m = parse(raw, &Limits::default()).unwrap();
    assert!(!m.text.contains('\0'));
    // Malformed base64 attachment: still listed, bytes are whatever decodes.
    let raw = crlf(
        "Subject: s\nContent-Type: multipart/mixed; boundary=b\n\n--b\nContent-Type: text/plain\n\nhi\n--b\nContent-Type: application/pdf; name=a.pdf\nContent-Transfer-Encoding: base64\n\n%%%%JVBERi0=====%%%\n--b--\n",
    );
    let m = parse(&raw, &Limits::default()).unwrap();
    assert_eq!(m.attachments.len(), 1);
    assert!(attachment_bytes(&raw, 0, &Limits::default()).is_ok());
}

fn mixed() -> Vec<u8> {
    crlf(
        "Subject: files
Content-Type: multipart/mixed; boundary=mix

--mix
Content-Type: multipart/related; boundary=rel

--rel
Content-Type: text/html; charset=utf-8

<p>Logo: <img src=\"cid:logo@x\" alt=\"logo\"> <img src=\"cid:missing\"></p>
--rel
Content-Type: image/png
Content-ID: <logo@x>
Content-Disposition: inline; filename=logo.png
Content-Transfer-Encoding: base64

iVBORw0KGgo=
--rel--
--mix
Content-Type: application/pdf
Content-Disposition: attachment; filename*0*=UTF-8''B%C3%A1o%20c; filename*1*=%C3%A1o.pdf
Content-Transfer-Encoding: base64

JVBERi0xLjQK
--mix
Content-Type: application/octet-stream
Content-Disposition: attachment; filename=\"../../etc/passwd\"

root
--mix
Content-Type: application/x-sh; name=\"=?UTF-8?B?4oCuZmRwLnNo?=\"

echo
--mix
Content-Type: message/rfc822

Subject: inner
Content-Type: text/plain

inner body
--mix--
",
    )
}

#[test]
fn attachments_are_listed_with_safe_names_and_inline_images_embedded() {
    let raw = mixed();
    let m = parse(&raw, &Limits::default()).unwrap();
    let names: Vec<_> = m.attachments.iter().map(|a| a.filename.as_str()).collect();
    assert_eq!(names[0], "logo.png");
    assert_eq!(names[1], "Báo cáo.pdf", "RFC 2231 continuation + charset");
    assert_eq!(names[2], "passwd", "no path survives");
    assert!(!names[3].contains('\u{202e}'), "bidi override removed");
    assert_eq!(m.attachments.len(), 5);
    let logo = &m.attachments[0];
    assert!(logo.inline && logo.content_id.as_deref() == Some("logo@x"));
    assert_eq!(
        (logo.content_type.as_str(), logo.size_bytes),
        ("image/png", 8)
    );
    assert_eq!(m.attachments[1].content_type, "application/pdf");
    assert!(!m.attachments[1].inline);
    assert_eq!(m.attachments[4].content_type, "message/rfc822");
    for (i, a) in m.attachments.iter().enumerate() {
        assert_eq!(a.part as usize, i);
        assert!(
            !a.filename.contains('/') && !a.filename.contains('\\') && !a.filename.starts_with('.')
        );
    }
    // cid image resolved to a data: URI; the unknown cid is dropped; nothing counts as remote.
    let html = m.html.unwrap();
    assert!(html.contains("src=\"data:image/png;base64,iVBORw0KGgo=\""));
    assert!(!html.contains("cid:"));
    assert!(!m.remote_blocked);

    let (name, bytes) = attachment_bytes(&raw, 1, &Limits::default()).unwrap();
    assert_eq!(
        (name.as_str(), bytes.as_slice()),
        ("Báo cáo.pdf", &b"%PDF-1.4\n"[..])
    );
    let (name, bytes) = attachment_bytes(&raw, 4, &Limits::default()).unwrap();
    assert!(name.ends_with(".eml") && bytes.starts_with(b"Subject: inner"));
    assert_eq!(
        attachment_bytes(&raw, 9, &Limits::default()),
        Err(MimeError::NoSuchAttachment)
    );
}

#[test]
fn into_record_carries_everything_the_store_needs() {
    let r = parse(&mixed(), &Limits::default())
        .unwrap()
        .into_record("m1");
    assert_eq!(r.message_id, "m1");
    assert_eq!(r.attachments.len(), 5);
    assert_eq!(r.attachments[1].filename, "Báo cáo.pdf");
    assert_eq!(r.attachments[1].media_type, "application/pdf");
    assert!(r.html.is_some() && !r.text.is_empty());
}

#[test]
fn oversized_message_is_refused_before_parsing() {
    let limits = Limits {
        max_message_bytes: 1024,
        ..Limits::default()
    };
    let mut raw = b"Subject: big\r\n\r\n".to_vec();
    raw.resize(2048, b'a');
    assert_eq!(parse(&raw, &limits), Err(MimeError::TooLarge));
    assert_eq!(attachment_bytes(&raw, 0, &limits), Err(MimeError::TooLarge));
}

#[test]
fn oversized_attachment_is_listed_but_not_extracted() {
    let limits = Limits {
        max_attachment_bytes: 4,
        ..Limits::default()
    };
    let raw = mixed();
    assert_eq!(parse(&raw, &limits).unwrap().attachments.len(), 5);
    assert_eq!(
        attachment_bytes(&raw, 1, &limits),
        Err(MimeError::AttachmentTooLarge)
    );
}

#[test]
fn oversized_text_is_truncated_and_oversized_html_dropped() {
    let limits = Limits {
        max_text_bytes: 10,
        max_html_bytes: 10,
        ..Limits::default()
    };
    let m = parse(&crlf(ALTERNATIVE), &limits).unwrap();
    assert!(m.truncated && m.html.is_none());
    assert!(m.text.len() <= 10 && !m.text.is_empty());
}

fn nested_multipart(levels: usize) -> Vec<u8> {
    let mut s = String::from("Subject: bomb\r\n");
    for i in 0..levels {
        s.push_str(&format!(
            "Content-Type: multipart/mixed; boundary=b{i}\r\n\r\n--b{i}\r\n"
        ));
    }
    s.push_str("Content-Type: text/plain\r\n\r\nx\r\n");
    for i in (0..levels).rev() {
        s.push_str(&format!("--b{i}--\r\n"));
    }
    s.into_bytes()
}

#[test]
fn nesting_depth_bomb_is_refused() {
    assert!(parse(&nested_multipart(5), &Limits::default()).is_ok());
    assert_eq!(
        parse(&nested_multipart(40), &Limits::default()),
        Err(MimeError::TooDeep)
    );
    // Far beyond any stack-friendly depth: must return, not crash.
    assert!(parse(&nested_multipart(5_000), &Limits::default()).is_err());
}

#[test]
fn nested_rfc822_chain_is_bounded() {
    let mut s = String::from("Subject: outer\r\n");
    for _ in 0..200 {
        s.push_str("Content-Type: message/rfc822\r\n\r\nSubject: in\r\n");
    }
    s.push_str("Content-Type: text/plain\r\n\r\nend\r\n");
    // Either refused or parsed with a bounded structure; never a crash.
    if let Ok(m) = parse(s.as_bytes(), &Limits::default()) {
        assert!(m.attachments.len() <= Limits::default().max_parts);
    }
}

#[test]
fn part_count_bomb_is_refused() {
    let mut s = String::from("Subject: many\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n");
    for _ in 0..500 {
        s.push_str("--b\r\nContent-Type: application/octet-stream\r\n\r\nx\r\n");
    }
    s.push_str("--b--\r\n");
    assert_eq!(
        parse(s.as_bytes(), &Limits::default()),
        Err(MimeError::TooManyParts)
    );
}

#[test]
fn garbage_input_never_panics() {
    assert_eq!(parse(b"", &Limits::default()), Err(MimeError::Malformed));
    for raw in [
        &b"\0\0\0\0"[..],
        b"\r\n\r\n",
        b"Content-Type: multipart/mixed; boundary=\r\n\r\n----\r\n",
        b"Content-Type: multipart/mixed\r\n\r\n--\r\n",
        b"Content-Type: text/html; charset=\xff\xff\r\n\r\n<\xff\xfe",
        b"Content-Type: =?=?=?=?; boundary*0*=''%%%\r\nContent-Transfer-Encoding: base64\r\n\r\n=",
    ] {
        let _ = parse(raw, &Limits::default());
        let _ = attachment_bytes(raw, 0, &Limits::default());
    }
    // Deterministic pseudo-random bytes around a MIME skeleton.
    let mut x: u32 = 0x1234_5678;
    for _ in 0..300 {
        let mut raw = b"Content-Type: multipart/mixed; boundary=q\r\n\r\n--q\r\n".to_vec();
        for _ in 0..200 {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            raw.push((x >> 24) as u8);
            if x.is_multiple_of(37) {
                raw.extend_from_slice(b"\r\n--q\r\nContent-Type: text/html\r\n\r\n");
            }
        }
        let _ = parse(&raw, &Limits::default());
    }
}
