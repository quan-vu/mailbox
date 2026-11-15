//! Builder tests: golden messages, header injection, line length, non-ASCII, Bcc, attachments.
use crate::address::{AddressError, Mailbox, is_valid_address, parse_mailbox, split_list};
use crate::message::{
    Attachment, BuildError, Limits, Message, Transfer, content_type_for, new_message_id,
};
use crate::reply::{forward_subject, quote, reply_subject};

fn mb(s: &str) -> Mailbox {
    parse_mailbox(s).unwrap()
}

fn base() -> Message {
    Message {
        from: mb("Minh <minh@company.vn>"),
        to: vec![mb("linh@company.vn")],
        cc: vec![],
        bcc: vec![],
        subject: "Hello".into(),
        text: "Hi Linh,\nSee you.\n".into(),
        html: None,
        message_id: "<d1.abc@company.vn>".into(),
        in_reply_to: None,
        references: vec![],
        date_epoch: 1_794_614_400,
        attachments: vec![],
    }
}

fn build(m: &Message, t: Transfer) -> String {
    String::from_utf8(m.build(t, &Limits::default()).unwrap().bytes).unwrap()
}

// --- small decoders, test-only (parsing proper belongs to the reader track) ---

fn b64_decode(s: &str) -> Vec<u8> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for b in s.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'=') {
        acc = (acc << 6) | T.iter().position(|t| *t == b).expect("base64 alphabet") as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

fn qp_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < b.len() {
        if b[i] == b'=' {
            if b[i + 1..].starts_with(b"\r\n") {
                i += 3;
            } else {
                out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
                i += 3;
            }
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// Unfold a header and decode its RFC 2047 words.
fn header(msg: &str, name: &str) -> String {
    let head = msg.split("\r\n\r\n").next().unwrap();
    let unfolded = head.replace("\r\n ", " ");
    let line = unfolded
        .split("\r\n")
        .find(|l| {
            l.to_ascii_lowercase()
                .starts_with(&format!("{}:", name.to_ascii_lowercase()))
        })
        .unwrap_or_else(|| panic!("no {name} header"));
    let value = line[name.len() + 1..].trim();
    let mut out = String::new();
    let mut prev_encoded = false;
    for (i, word) in value.split(' ').enumerate() {
        let decoded = word
            .strip_prefix("=?UTF-8?B?")
            .and_then(|w| w.strip_suffix("?="))
            .map(|w| String::from_utf8(b64_decode(w)).unwrap());
        // Whitespace between two encoded words is not part of the text.
        if i > 0 && !(prev_encoded && decoded.is_some()) {
            out.push(' ');
        }
        prev_encoded = decoded.is_some();
        out.push_str(decoded.as_deref().unwrap_or(word));
    }
    out
}

fn body(msg: &str) -> &str {
    msg.split_once("\r\n\r\n").unwrap().1
}

fn assert_wire_safe(msg: &str) {
    for line in msg.split("\r\n") {
        assert!(line.len() <= 998, "line of {} bytes", line.len());
        assert!(!line.contains('\r') && !line.contains('\n'), "bare CR/LF");
    }
    assert!(msg.ends_with("\r\n"));
}

// --- addresses ---

#[test]
fn address_validation_is_structural() {
    for ok in [
        "linh.tran@company.vn",
        "a+tag@sub.example.org",
        "x_y-z@a-b.io",
        "người@ví-dụ.vn",
    ] {
        assert!(is_valid_address(ok), "{ok}");
    }
    for bad in [
        "",
        "ketoan@company",
        "no-at-sign.vn",
        "a b@company.vn",
        "@company.vn",
        "a@.vn",
        "a@b..vn",
        ".a@b.vn",
        "a.@b.vn",
        "a..b@c.vn",
        "a@b.vn.",
        "a@-b.vn",
        "a@@b.vn",
        "a@b@c.vn",
        "a<b@c.vn",
        "a@b.vn>",
        "a,b@c.vn",
        "\"q\"@c.vn",
        "a@b.vn\r\nBcc: x@y.z",
        "a\0@b.vn",
    ] {
        assert!(!is_valid_address(bad), "{bad:?}");
    }
    assert!(!is_valid_address(&format!("{}@b.vn", "a".repeat(65))));
    assert!(!is_valid_address(&format!("a@{}.vn", "b".repeat(64))));
}

#[test]
fn mailboxes_parse_with_and_without_names() {
    let m = mb("Trần Linh <linh.tran@company.vn>");
    assert_eq!(
        (m.name.as_str(), m.address.as_str()),
        ("Trần Linh", "linh.tran@company.vn")
    );
    let m = mb("\"Tran, Linh\" <linh@company.vn>");
    assert_eq!(m.name, "Tran, Linh");
    let m = mb("  bare@company.vn ");
    assert_eq!(
        (m.name.as_str(), m.address.as_str()),
        ("", "bare@company.vn")
    );
    assert!(m.is_ascii() && !mb("người@ví-dụ.vn").is_ascii());
    assert_eq!(parse_mailbox("   "), Err(AddressError::Empty));
    assert_eq!(
        parse_mailbox("Linh <linh@company>"),
        Err(AddressError::Syntax)
    );
    assert_eq!(
        parse_mailbox("Linh <a@b.vn> trailing"),
        Err(AddressError::Syntax)
    );
    assert_eq!(
        parse_mailbox("Evil\r\nBcc: v@x.io <a@b.vn>"),
        Err(AddressError::ControlCharacter)
    );
}

#[test]
fn lists_split_outside_quotes_and_brackets() {
    assert_eq!(
        split_list("a@x.vn, \"Tran, Linh\" <b@x.vn>; c@x.vn,, "),
        vec!["a@x.vn", "\"Tran, Linh\" <b@x.vn>", "c@x.vn"]
    );
}

// --- golden ---

#[test]
fn golden_plain_ascii_message() {
    assert_eq!(
        build(&base(), Transfer::SevenBit),
        "Date: Sat, 14 Nov 2026 00:00:00 +0000\r\n\
         From: Minh <minh@company.vn>\r\n\
         To: linh@company.vn\r\n\
         Subject: Hello\r\n\
         Message-ID: <d1.abc@company.vn>\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Transfer-Encoding: 7bit\r\n\
         \r\n\
         Hi Linh,\r\n\
         See you.\r\n"
    );
}

#[test]
fn golden_reply_with_html_alternative() {
    let mut m = base();
    m.html = Some("<p>Hi <b>Linh</b></p>".into());
    m.in_reply_to = Some("<orig@x.org>".into());
    m.references = vec!["<root@x.org>".into(), "<orig@x.org>".into()];
    m.subject = "Re: Hello".into();
    let out = build(&m, Transfer::SevenBit);
    let boundary = out
        .split("boundary=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap()
        .to_string();
    assert_eq!(
        out,
        format!(
            "Date: Sat, 14 Nov 2026 00:00:00 +0000\r\n\
             From: Minh <minh@company.vn>\r\n\
             To: linh@company.vn\r\n\
             Subject: Re: Hello\r\n\
             Message-ID: <d1.abc@company.vn>\r\n\
             In-Reply-To: <orig@x.org>\r\n\
             References: <root@x.org> <orig@x.org>\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: multipart/alternative; boundary=\"{boundary}\"\r\n\
             \r\n\
             --{boundary}\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             Content-Transfer-Encoding: 7bit\r\n\
             \r\n\
             Hi Linh,\r\n\
             See you.\r\n\
             --{boundary}\r\n\
             Content-Type: text/html; charset=utf-8\r\n\
             Content-Transfer-Encoding: 7bit\r\n\
             \r\n\
             <p>Hi <b>Linh</b></p>\r\n\
             --{boundary}--\r\n"
        )
    );
}

#[test]
fn rendering_is_deterministic_per_draft() {
    let mut m = base();
    m.html = Some("<p>x</p>".into());
    assert_eq!(build(&m, Transfer::EightBit), build(&m, Transfer::EightBit));
    let mut other = m.clone();
    other.message_id = "<d2.abc@company.vn>".into();
    assert_ne!(
        build(&m, Transfer::SevenBit),
        build(&other, Transfer::SevenBit)
    );
}

// --- non-ASCII ---

#[test]
fn vietnamese_subject_and_names_round_trip() {
    let mut m = base();
    m.subject = "Kế hoạch ra mắt phiên bản 2.0 — họp lúc 9 giờ sáng thứ Hai tuần tới tại phòng họp lớn tầng ba".into();
    m.from = mb("Vũ Huy Quân <minh@company.vn>");
    m.to = vec![
        mb("Trần Thị Linh <linh@company.vn>"),
        mb("\"Nguyễn, An\" <an@company.vn>"),
    ];
    let out = build(&m, Transfer::SevenBit);
    assert!(out.is_ascii(), "7-bit rendering has no raw UTF-8");
    assert_wire_safe(&out);
    for line in out.split("\r\n\r\n").next().unwrap().split("\r\n") {
        assert!(line.len() <= 78, "header line too long: {}", line.len());
    }
    assert_eq!(header(&out, "Subject"), m.subject);
    assert_eq!(header(&out, "From"), "Vũ Huy Quân <minh@company.vn>");
    assert_eq!(
        header(&out, "To"),
        "Trần Thị Linh <linh@company.vn>, Nguyễn, An <an@company.vn>"
    );
}

#[test]
fn vietnamese_body_is_8bit_when_allowed_and_quoted_printable_otherwise() {
    let mut m = base();
    m.text = "Xin chào,\nĐây là thư thử = dấu bằng.\n".into();
    let eight = m.build(Transfer::EightBit, &Limits::default()).unwrap();
    let text = String::from_utf8(eight.bytes).unwrap();
    assert!(eight.has_8bit && text.contains("Content-Transfer-Encoding: 8bit\r\n"));
    assert!(text.ends_with("Xin chào,\r\nĐây là thư thử = dấu bằng.\r\n"));

    let seven = m.build(Transfer::SevenBit, &Limits::default()).unwrap();
    assert!(!seven.has_8bit);
    let text = String::from_utf8(seven.bytes).unwrap();
    assert!(text.is_ascii() && text.contains("Content-Transfer-Encoding: quoted-printable\r\n"));
    assert!(body(&text).contains("=3D"), "= is escaped");
    assert_eq!(
        String::from_utf8(qp_decode(body(&text))).unwrap(),
        "Xin chào,\r\nĐây là thư thử = dấu bằng.\r\n"
    );
}

#[test]
fn ascii_names_with_specials_are_quoted_not_injected() {
    let mut m = base();
    m.to = vec![mb("\"Linh \\\"Boss\\\" <ceo@evil.io>\" <linh@company.vn>")];
    let out = build(&m, Transfer::SevenBit);
    assert!(out.contains("To: \"Linh \\\"Boss\\\" <ceo@evil.io>\" <linh@company.vn>\r\n"));
}

#[test]
fn utf8_addresses_require_smtputf8() {
    let mut m = base();
    m.to = vec![mb("người@ví-dụ.vn")];
    let built = m.build(Transfer::SevenBit, &Limits::default()).unwrap();
    assert!(built.needs_smtputf8 && built.has_8bit);
    assert!(
        !base()
            .build(Transfer::SevenBit, &Limits::default())
            .unwrap()
            .needs_smtputf8
    );
}

// --- injection ---

#[test]
fn header_injection_attempts_are_rejected_not_cleaned() {
    let inject = |f: &dyn Fn(&mut Message)| {
        let mut m = base();
        f(&mut m);
        m.build(Transfer::SevenBit, &Limits::default()).unwrap_err()
    };
    assert_eq!(
        inject(&|m| m.subject = "Hi\r\nBcc: victim@x.io".into()),
        BuildError::HeaderInjection("subject")
    );
    assert_eq!(
        inject(&|m| m.subject = "Hi\nX: y".into()),
        BuildError::HeaderInjection("subject")
    );
    assert_eq!(
        inject(&|m| m.subject = "Hi\0".into()),
        BuildError::HeaderInjection("subject")
    );
    assert_eq!(
        inject(&|m| m.from.name = "A\r\nBcc: v@x.io".into()),
        BuildError::HeaderInjection("name")
    );
    assert_eq!(
        inject(&|m| m.to[0].address = "a@b.vn>\r\nRCPT TO:<c@d.vn".into()),
        BuildError::InvalidAddress
    );
    assert_eq!(
        inject(&|m| m.bcc.push(Mailbox {
            name: String::new(),
            address: "a@b.vn\nx".into()
        })),
        BuildError::InvalidAddress
    );
    assert_eq!(
        inject(&|m| m.message_id = "<a@b>\r\nBcc: v@x.io".into()),
        BuildError::InvalidMessageId
    );
    assert_eq!(
        inject(&|m| m.message_id = "a@b".into()),
        BuildError::InvalidMessageId
    );
    assert_eq!(
        inject(&|m| m.in_reply_to = Some("<a@b> <c@d>\r\nX: y".into())),
        BuildError::InvalidMessageId
    );
    assert_eq!(
        inject(&|m| m.references = vec!["<ok@x>".into(), "bad".into()]),
        BuildError::InvalidMessageId
    );
    assert_eq!(
        inject(&|m| m.attachments.push(Attachment {
            filename: "a.pdf\"\r\nContent-Type: text/html".into(),
            content_type: None,
            data: vec![1],
        })),
        BuildError::HeaderInjection("filename")
    );
    assert_eq!(
        inject(&|m| m.attachments.push(Attachment {
            filename: "a.pdf".into(),
            content_type: Some("text/html\r\nX: y".into()),
            data: vec![1],
        })),
        BuildError::HeaderInjection("content-type")
    );
    assert_eq!(inject(&|m| m.to.clear()), BuildError::NoRecipients);
}

#[test]
fn body_cannot_smuggle_headers_or_end_the_message() {
    let mut m = base();
    m.text = "line\r\n.\r\nMAIL FROM:<x@y.z>\rbare-cr\n\nBcc: v@x.io\n".into();
    let out = build(&m, Transfer::SevenBit);
    assert_wire_safe(&out);
    let (head, text) = out.split_once("\r\n\r\n").unwrap();
    assert!(!head.contains("Bcc") && !head.contains("MAIL FROM"));
    // Line endings are normalised; the lone "." stays a body line (the SMTP layer dot-stuffs it).
    assert_eq!(
        text,
        "line\r\n.\r\nMAIL FROM:<x@y.z>\r\nbare-cr\r\n\r\nBcc: v@x.io\r\n"
    );
}

// --- Bcc ---

#[test]
fn bcc_is_in_the_envelope_and_never_in_the_message() {
    let mut m = base();
    m.cc = vec![mb("An <an@company.vn>")];
    m.bcc = vec![mb("Secret Boss <boss@hidden.io>"), mb("LINH@company.vn")];
    let built = m.build(Transfer::SevenBit, &Limits::default()).unwrap();
    assert_eq!(built.envelope_from, "minh@company.vn");
    assert_eq!(
        built.recipients,
        vec!["linh@company.vn", "an@company.vn", "boss@hidden.io"],
        "to + cc + bcc, duplicates removed"
    );
    let text = String::from_utf8(built.bytes).unwrap();
    assert!(text.contains("Cc: An <an@company.vn>\r\n"));
    for leak in ["boss@hidden.io", "Secret Boss", "Bcc", "bcc"] {
        assert!(!text.contains(leak), "{leak} leaked");
    }
}

// --- long lines ---

#[test]
fn long_lines_are_encoded_to_stay_within_limits() {
    let mut m = base();
    m.text = format!("{}\n{}\n", "a".repeat(2500), "ờ".repeat(900));
    m.subject = "x".repeat(1200);
    m.references = (0..40)
        .map(|i| format!("<ref-{i}-aaaaaaaaaaaaaaaaaaaa@example.org>"))
        .collect();
    m.to = (0..30)
        .map(|i| mb(&format!("Người nhận {i} <user{i}@company.vn>")))
        .collect();
    for transfer in [Transfer::SevenBit, Transfer::EightBit] {
        let out = build(&m, transfer);
        assert_wire_safe(&out);
        let (head, text) = out.split_once("\r\n\r\n").unwrap();
        for line in head.split("\r\n") {
            assert!(line.len() <= 78, "header line of {}", line.len());
        }
        assert!(head.contains("Content-Transfer-Encoding: quoted-printable"));
        for line in text.split("\r\n") {
            assert!(line.len() <= 76, "body line of {}", line.len());
        }
        assert_eq!(
            String::from_utf8(qp_decode(text)).unwrap(),
            format!("{}\r\n{}\r\n", "a".repeat(2500), "ờ".repeat(900))
        );
        assert_eq!(header(&out, "Subject"), m.subject);
    }
}

#[test]
fn trailing_whitespace_and_control_bytes_survive_transport() {
    let mut m = base();
    m.text = "tab at end\t\nspace at end \nbell\u{7}\n".into();
    let out = build(&m, Transfer::SevenBit);
    assert!(out.contains("quoted-printable"));
    assert_eq!(
        String::from_utf8(qp_decode(body(&out))).unwrap(),
        "tab at end\t\r\nspace at end \r\nbell\u{7}\r\n"
    );
}

// --- attachments ---

#[test]
fn attachments_are_base64_in_multipart_mixed_with_rfc2231_names() {
    let mut m = base();
    let data: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
    m.attachments = vec![
        Attachment {
            filename: "/Users/minh/Tài liệu/Báo cáo quý 3 — bản cuối cùng đã duyệt (ký tên).pdf"
                .into(),
            content_type: None,
            data: data.clone(),
        },
        Attachment {
            filename: "C:\\tmp\\notes.txt".into(),
            content_type: None,
            data: b"hi\n".to_vec(),
        },
    ];
    let out = build(&m, Transfer::SevenBit);
    assert_wire_safe(&out);
    assert!(out.is_ascii());
    let boundary = out
        .split("boundary=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    assert!(out.contains(&format!(
        "Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n"
    )));
    let parts: Vec<&str> = out.split(&format!("--{boundary}")).collect();
    assert_eq!(parts.len(), 5, "preamble, text, 2 attachments, closing");
    assert!(parts[4].starts_with("--\r\n"));
    assert!(parts[1].contains("Content-Type: text/plain; charset=utf-8"));

    let (head, payload) = parts[2]
        .trim_start_matches("\r\n")
        .split_once("\r\n\r\n")
        .unwrap();
    assert!(head.contains("Content-Type: application/pdf\r\n"));
    assert!(head.contains("Content-Transfer-Encoding: base64"));
    assert!(head.contains("Content-Disposition: attachment;"));
    assert!(
        !head.contains("Users") && !head.contains("minh"),
        "directories are dropped"
    );
    for line in head.split("\r\n") {
        assert!(line.len() <= 78, "attachment header line of {}", line.len());
    }
    // RFC 2231: percent-encoded UTF-8 in numbered continuations.
    let unfolded = head.replace("\r\n ", " ");
    let mut encoded = String::new();
    for i in 0.. {
        let key = format!("filename*{i}*=");
        let Some(pos) = unfolded.find(&key) else {
            break;
        };
        let value = unfolded[pos + key.len()..]
            .split(';')
            .next()
            .unwrap()
            .trim();
        encoded.push_str(value);
    }
    let encoded = encoded
        .strip_prefix("UTF-8''")
        .expect("charset prefix on the first section");
    let mut bytes = Vec::new();
    let raw = encoded.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' {
            bytes.push(u8::from_str_radix(&encoded[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            bytes.push(raw[i]);
            i += 1;
        }
    }
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        "Báo cáo quý 3 — bản cuối cùng đã duyệt (ký tên).pdf"
    );
    for line in payload.trim_end().split("\r\n") {
        assert!(line.len() <= 76);
    }
    assert_eq!(b64_decode(payload), data);

    let (head, payload) = parts[3]
        .trim_start_matches("\r\n")
        .split_once("\r\n\r\n")
        .unwrap();
    assert!(head.contains("Content-Type: text/plain\r\n"));
    assert!(head.contains("Content-Disposition: attachment; filename=\"notes.txt\""));
    assert_eq!(b64_decode(payload), b"hi\n");
}

#[test]
fn html_plus_attachment_nests_alternative_inside_mixed() {
    let mut m = base();
    m.html = Some("<p>x</p>".into());
    m.attachments = vec![Attachment {
        filename: "a.png".into(),
        content_type: None,
        data: vec![0; 10],
    }];
    let out = build(&m, Transfer::SevenBit);
    let mixed = out.find("multipart/mixed").unwrap();
    let alt = out.find("multipart/alternative").unwrap();
    assert!(mixed < alt && out.contains("Content-Type: image/png\r\n"));
    let boundaries: Vec<&str> = out
        .split("boundary=\"")
        .skip(1)
        .map(|s| s.split('"').next().unwrap())
        .collect();
    assert_eq!(boundaries.len(), 2);
    assert_ne!(boundaries[0], boundaries[1]);
}

#[test]
fn attachment_size_cap_gives_a_clear_error() {
    let mut m = base();
    m.attachments = vec![
        Attachment {
            filename: "a.bin".into(),
            content_type: None,
            data: vec![0; 600],
        },
        Attachment {
            filename: "b.bin".into(),
            content_type: None,
            data: vec![0; 600],
        },
    ];
    let limits = Limits {
        max_attachment_bytes: 1000,
        ..Limits::default()
    };
    assert_eq!(
        m.build(Transfer::SevenBit, &limits).unwrap_err(),
        BuildError::AttachmentsTooLarge {
            limit: 1000,
            actual: 1200
        }
    );
    let limits = Limits {
        max_attachment_bytes: 5000,
        max_message_bytes: 1500,
    };
    assert!(matches!(
        m.build(Transfer::SevenBit, &limits).unwrap_err(),
        BuildError::MessageTooLarge { limit: 1500, .. }
    ));
    m.attachments[0].filename = "dir/".into();
    assert_eq!(
        m.build(Transfer::SevenBit, &Limits::default()).unwrap_err(),
        BuildError::InvalidFilename
    );
}

#[test]
fn content_types_follow_the_extension() {
    for (name, ty) in [
        ("a.PDF", "application/pdf"),
        ("b.jpeg", "image/jpeg"),
        (
            "c.docx",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        ),
        ("d.csv", "text/csv"),
        ("e.zip", "application/zip"),
        ("noext", "application/octet-stream"),
        ("f.weird", "application/octet-stream"),
    ] {
        assert_eq!(content_type_for(name), ty, "{name}");
    }
}

// --- ids and reply helpers ---

#[test]
fn message_ids_are_well_formed_and_scoped_to_the_sender_domain() {
    assert_eq!(
        new_message_id("18f.2a", "Minh@Company.VN"),
        Some("<18f.2a@company.vn>".into())
    );
    assert_eq!(
        new_message_id("a b", "minh@company.vn"),
        None,
        "token must be id-safe"
    );
    assert_eq!(new_message_id("x", "not-an-address"), None);
    // Non-ASCII domains do not go into the id (it must survive 7-bit transport).
    assert_eq!(
        new_message_id("x", "a@ví-dụ.vn"),
        Some("<x@mailbox.invalid>".into())
    );
}

#[test]
fn reply_and_forward_prefixes_do_not_stack() {
    assert_eq!(reply_subject("Hello"), "Re: Hello");
    assert_eq!(reply_subject("RE: re: Fwd: Hello"), "Re: Hello");
    assert_eq!(forward_subject("Re: Hello"), "Fwd: Hello");
    assert_eq!(forward_subject(""), "Fwd: ");
    assert_eq!(
        quote("Linh đã viết:", "dòng 1\n> cũ\n\ndòng 3"),
        "\n\nLinh đã viết:\n> dòng 1\n>> cũ\n>\n> dòng 3\n"
    );
}
