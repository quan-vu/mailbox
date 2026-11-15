//! Allow-list HTML sanitizer for received mail (threats T01, T08, T11).
//!
//! Built on `ammonia` (html5ever parser, so the result is what a browser would parse, not what
//! a regex would guess). On top of its allow-list this module decides every URL itself:
//! - `href` survives only on `<a>` and only for `http`, `https`, `mailto`.
//! - `src` survives only on `<img>`: a `cid:` reference that resolves to an image carried by the
//!   message, a small raster `data:` image, or — only in the explicit opt-in variant — `http(s)`.
//! - `srcset`, `background`, `poster` and any `style` declaration that could fetch something
//!   (`url(...)`, `image-set`, escapes) are dropped.
//! - No `<style>`, `<link>`, `<base>`, `<meta>`, forms, frames, objects, SVG or MathML.
//!
//! The caller still shows the result in a sandboxed frame with a restrictive CSP; this module is
//! the first of two layers, not the only one.
use ammonia::{Builder, UrlRelative};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Inline images of the message: content id (without `<>`) → `data:` URI.
pub type InlineImages = HashMap<String, String>;

/// Largest `data:` image URI kept from the HTML itself.
const MAX_DATA_URI: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sanitized {
    /// Safe HTML with every remote reference removed.
    pub html: String,
    /// Same, but remote `<img src>` kept. `None` when there is no remote image.
    pub html_with_remote_images: Option<String>,
    /// At least one remote reference was removed from `html`.
    pub remote_blocked: bool,
}

const TAGS: [&str; 61] = [
    "a",
    "abbr",
    "b",
    "bdi",
    "bdo",
    "big",
    "blockquote",
    "br",
    "caption",
    "center",
    "cite",
    "code",
    "col",
    "colgroup",
    "dd",
    "del",
    "dfn",
    "div",
    "dl",
    "dt",
    "em",
    "font",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
    "img",
    "ins",
    "kbd",
    "li",
    "mark",
    "ol",
    "p",
    "pre",
    "q",
    "s",
    "samp",
    "small",
    "span",
    "strike",
    "strong",
    "sub",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "tt",
    "u",
    "ul",
    "var",
    "wbr",
    "address",
    "section",
];

/// Elements whose text content must vanish with them (not be shown as text).
const DROP_WITH_CONTENT: [&str; 16] = [
    "script", "style", "title", "template", "svg", "math", "iframe", "object", "embed", "noembed",
    "noframes", "noscript", "select", "textarea", "button", "head",
];

const GENERIC_ATTRIBUTES: [&str; 14] = [
    "style",
    "dir",
    "lang",
    "title",
    "align",
    "valign",
    "bgcolor",
    "width",
    "height",
    "border",
    "cellpadding",
    "cellspacing",
    "color",
    "background",
];

fn has_control(value: &str) -> bool {
    value.chars().any(char::is_control)
}

fn is_link(value: &str) -> bool {
    let v = value.trim().to_ascii_lowercase();
    !has_control(&v)
        && (v.starts_with("https://") || v.starts_with("http://") || v.starts_with("mailto:"))
}

fn is_remote(value: &str) -> bool {
    let v = value.trim().to_ascii_lowercase();
    !has_control(&v) && (v.starts_with("https://") || v.starts_with("http://"))
}

fn is_safe_data_image(value: &str) -> bool {
    let v = value.trim();
    if v.len() > MAX_DATA_URI {
        return false;
    }
    // Byte-wise: the value is untrusted and may hold multi-byte characters anywhere (MB-304).
    let head = &v.as_bytes()[..v.len().min(32)];
    ["png", "jpeg", "jpg", "gif", "webp"].iter().any(|kind| {
        let prefix = format!("data:image/{kind};base64,");
        head.len() >= prefix.len()
            && head[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
            && v.as_bytes()[prefix.len()..]
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
    })
}

/// Keep only declarations that cannot make the renderer fetch or run anything.
/// Returns the filtered value and whether a remote-looking reference was dropped.
fn filter_style(value: &str) -> (Option<String>, bool) {
    let lower = value.to_ascii_lowercase();
    let remote = lower.contains("url") || lower.contains("image-set") || lower.contains("//");
    // Escapes (`u\72l(`), comments and markup can hide anything: refuse the whole attribute.
    if value.contains('\\') || lower.contains("/*") || value.contains('<') || has_control(value) {
        return (None, remote);
    }
    let mut dropped_remote = false;
    let kept: Vec<&str> = value
        .split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .filter(|d| {
            let l = d.to_ascii_lowercase();
            let bad = [
                "url",
                "expression",
                "behavior",
                "binding",
                "import",
                "image",
                "src",
                "@",
                "//",
                "javascript",
                "attr(",
                "var(",
                "env(",
            ]
            .iter()
            .any(|needle| l.contains(needle));
            dropped_remote |= bad && (l.contains("url") || l.contains("image") || l.contains("//"));
            !bad
        })
        .collect();
    if kept.is_empty() {
        (None, dropped_remote)
    } else {
        (Some(kept.join(";")), dropped_remote)
    }
}

#[derive(Default)]
struct Seen {
    remote_image: AtomicBool,
    other_remote: AtomicBool,
}

fn run(
    html: &str,
    inline: &Arc<InlineImages>,
    allow_remote_images: bool,
    seen: &Arc<Seen>,
) -> String {
    let (inline, seen) = (Arc::clone(inline), Arc::clone(seen));
    let mut tag_attributes: HashMap<&str, HashSet<&str>> = HashMap::new();
    tag_attributes.insert("a", HashSet::from(["href", "name"]));
    tag_attributes.insert("img", HashSet::from(["src", "srcset", "alt"]));
    tag_attributes.insert("font", HashSet::from(["face", "size"]));
    for cell in ["td", "th"] {
        tag_attributes.insert(cell, HashSet::from(["colspan", "rowspan", "nowrap"]));
    }
    tag_attributes.insert("ol", HashSet::from(["start", "type"]));
    tag_attributes.insert("col", HashSet::from(["span"]));
    tag_attributes.insert("colgroup", HashSet::from(["span"]));
    Builder::default()
        .tags(HashSet::from(TAGS))
        .clean_content_tags(HashSet::from(DROP_WITH_CONTENT))
        .generic_attributes(HashSet::from(GENERIC_ATTRIBUTES))
        .tag_attributes(tag_attributes)
        .tag_attribute_values(HashMap::new())
        .set_tag_attribute_values(HashMap::new())
        .allowed_classes(HashMap::new())
        // The scheme list lets candidates reach the filter below, which has the final word.
        .url_schemes(HashSet::from(["http", "https", "mailto", "cid", "data"]))
        .url_relative(UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer"))
        .strip_comments(true)
        .attribute_filter(move |element, attribute, value| match attribute {
            "href" => (element == "a" && is_link(value)).then_some(Cow::Borrowed(value)),
            "src" => {
                if element != "img" {
                    return None;
                }
                let v = value.trim();
                if v.len() >= 4 && v.as_bytes()[..4].eq_ignore_ascii_case(b"cid:") {
                    inline.get(&v[4..]).map(|data| Cow::Owned(data.clone()))
                } else if is_safe_data_image(v) {
                    Some(Cow::Borrowed(value))
                } else if is_remote(v) {
                    seen.remote_image.store(true, Ordering::Relaxed);
                    allow_remote_images.then_some(Cow::Borrowed(value))
                } else {
                    None
                }
            }
            // These can only ever point somewhere else; they are never restored.
            "srcset" | "background" | "poster" | "ping" => {
                if !value.trim().is_empty() {
                    seen.other_remote.store(true, Ordering::Relaxed);
                }
                None
            }
            "style" => {
                let (kept, remote) = filter_style(value);
                if remote {
                    seen.other_remote.store(true, Ordering::Relaxed);
                }
                kept.map(Cow::Owned)
            }
            _ => Some(Cow::Borrowed(value)),
        })
        .clean(html)
        .to_string()
}

/// Sanitize one HTML body. `inline` maps the message's own inline images by content id.
pub fn sanitize(html: &str, inline: &InlineImages) -> Sanitized {
    let inline = Arc::new(inline.clone());
    let seen = Arc::new(Seen::default());
    let blocked = run(html, &inline, false, &seen);
    let has_remote_image = seen.remote_image.load(Ordering::Relaxed);
    let html_with_remote_images =
        has_remote_image.then(|| run(html, &inline, true, &Arc::new(Seen::default())));
    Sanitized {
        html: blocked,
        html_with_remote_images,
        remote_blocked: has_remote_image || seen.other_remote.load(Ordering::Relaxed),
    }
}
