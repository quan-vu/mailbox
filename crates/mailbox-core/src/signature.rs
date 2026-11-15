//! Signature placement in a draft body (MB-210).
//!
//! The signature is a block `\n\n-- \n<text>` (RFC 3676 separator) placed after what the user
//! types and before any quoted text. It is replaced in place when the identity changes and is
//! never added twice.

const SEPARATOR: &str = "-- \n";

/// The exact text that is inserted for a signature.
pub fn block(signature: &str) -> String {
    format!("\n\n{SEPARATOR}{}", signature.trim_end())
}

/// Byte offset where quoted text begins: the first `>` line, or the attribution line just above
/// it. `None` when the body has no quote.
fn quote_start(body: &str) -> Option<usize> {
    let mut offset = 0;
    let mut prev: Option<(usize, &str)> = None;
    for line in body.split_inclusive('\n') {
        if line.starts_with('>') {
            return Some(match prev {
                Some((at, p)) if p.trim_end().ends_with(':') => at,
                _ => offset,
            });
        }
        if !line.trim().is_empty() {
            prev = Some((offset, line));
        }
        offset += line.len();
    }
    None
}

/// Swap the signature of `body`. `old` is the signature text that was inserted before (if any),
/// `new` the one that should be there now (`None`/blank = no signature).
///
/// * `old` present in the body: replaced (or removed) where it is, so edits around it survive.
/// * otherwise `new` is inserted above the quoted text, or at the end when there is none.
/// * `new` already present: nothing changes.
pub fn apply(body: &str, old: Option<&str>, new: Option<&str>) -> String {
    let new_block = new.filter(|s| !s.trim().is_empty()).map(block);
    let old_block = old.filter(|s| !s.trim().is_empty()).map(block);
    if let (Some(n), true) = (&new_block, old_block.as_ref() != new_block.as_ref())
        && body.contains(n.as_str())
        && old_block
            .as_ref()
            .is_none_or(|o| !body.contains(o.as_str()))
    {
        return body.to_string();
    }
    if let Some(o) = &old_block
        && let Some(at) = body.find(o.as_str())
    {
        let mut out = String::with_capacity(body.len());
        out.push_str(&body[..at]);
        if let Some(n) = &new_block {
            out.push_str(n);
        }
        out.push_str(&body[at + o.len()..]);
        return out;
    }
    let Some(n) = new_block else {
        return body.to_string();
    };
    if body.contains(&n) {
        return body.to_string();
    }
    match quote_start(body) {
        Some(at) => {
            let head = body[..at].trim_end_matches('\n');
            format!("{head}{n}{}", &body[at..])
        }
        None => format!("{}{n}", body.trim_end_matches('\n')),
    }
}

/// True when the body holds nothing but whitespace or exactly one of the `known` signature
/// blocks, so a draft that only has the automatic signature still counts as empty.
///
/// The comparison is exact: any character the user typed before, after or instead of the
/// signature makes the body non-empty. With no known signatures only whitespace is empty, so an
/// unknown case errs on the side of keeping the draft.
pub fn is_only_signature_of(body: &str, known: &[&str]) -> bool {
    let t = body.trim_start().trim_end();
    if t.is_empty() {
        return true;
    }
    known
        .iter()
        .filter(|s| !s.trim().is_empty())
        .any(|s| t == format!("{SEPARATOR}{}", s.trim_end()).trim_end())
}

/// Make `body` carry exactly `desired` (the signature of the identity now selected) and no other
/// known signature. `known` lists every signature text the user has, so one inserted for another
/// identity earlier (even in a draft reloaded from disk) is replaced, not kept next to the new one.
pub fn sync(body: &str, desired: Option<&str>, known: &[&str]) -> String {
    let desired = desired.filter(|s| !s.trim().is_empty());
    let mut out = body.to_string();
    let mut placed = false;
    for other in known {
        if other.trim().is_empty() || Some(*other) == desired {
            continue;
        }
        if out.contains(&block(other)) {
            // The first foreign signature found becomes the new one; further ones are removed.
            let new = if placed { None } else { desired };
            out = apply(&out, Some(other), new);
            placed = placed || new.is_some();
        }
    }
    if placed {
        out
    } else {
        apply(&out, None, desired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appended_once_and_not_duplicated() {
        let once = apply("Chào anh", None, Some("Linh"));
        assert_eq!(once, "Chào anh\n\n-- \nLinh");
        assert_eq!(apply(&once, None, Some("Linh")), once);
        assert_eq!(apply(&once, Some("Linh"), Some("Linh")), once);
    }

    #[test]
    fn empty_body_gets_a_cursor_block_above() {
        assert_eq!(apply("", None, Some("Linh")), "\n\n-- \nLinh");
    }

    #[test]
    fn replaced_in_place_when_identity_changes() {
        let body = apply("Xin chào", None, Some("Work Linh"));
        let swapped = apply(&body, Some("Work Linh"), Some("Home Linh"));
        assert_eq!(swapped, "Xin chào\n\n-- \nHome Linh");
        assert!(!swapped.contains("Work"));
        let removed = apply(&swapped, Some("Home Linh"), None);
        assert_eq!(removed, "Xin chào");
    }

    #[test]
    fn edits_around_the_signature_survive_a_swap() {
        let body = "Mở đầu\n\n-- \nA\nthêm dòng sau";
        assert_eq!(
            apply(body, Some("A"), Some("B")),
            "Mở đầu\n\n-- \nB\nthêm dòng sau"
        );
    }

    #[test]
    fn goes_above_the_quote_not_inside() {
        let reply = "\n\n--- Linh <l@x.vn> đã viết (1/1/2026 UTC):\n> câu hỏi\n> thêm\n";
        let out = apply(reply, None, Some("Minh"));
        let sig = out.find("-- \nMinh").unwrap();
        let quote = out.find("--- Linh").unwrap();
        assert!(sig < quote, "{out:?}");
        // Nothing of the signature inside the quoted lines, and the quote is byte-identical.
        assert!(out.ends_with("--- Linh <l@x.vn> đã viết (1/1/2026 UTC):\n> câu hỏi\n> thêm\n"));
        // A second application changes nothing, a swap stays above the quote.
        assert_eq!(apply(&out, None, Some("Minh")), out);
        let swapped = apply(&out, Some("Minh"), Some("Khác"));
        assert!(swapped.find("Khác").unwrap() < swapped.find("--- Linh").unwrap());
        assert!(!swapped.contains("Minh"));
    }

    #[test]
    fn blank_signature_is_no_signature() {
        assert_eq!(apply("Hi", None, Some("  \n")), "Hi");
    }

    #[test]
    fn sync_replaces_a_foreign_signature_found_in_a_reloaded_draft() {
        let known = ["Work Linh", "Home Linh"];
        let body = apply("Chào", None, Some("Work Linh"));
        let out = sync(&body, Some("Home Linh"), &known);
        assert_eq!(out, "Chào\n\n-- \nHome Linh");
        assert_eq!(sync(&out, Some("Home Linh"), &known), out);
        assert_eq!(sync(&out, None, &known), "Chào");
    }

    #[test]
    fn only_a_known_signature_counts_as_empty() {
        let known = ["Linh", "Minh\nCông ty A"];
        assert!(is_only_signature_of("", &known));
        assert!(is_only_signature_of("  \n ", &[]));
        assert!(is_only_signature_of("\n\n-- \nLinh", &known));
        assert!(is_only_signature_of("\n\n-- \nLinh\n", &known));
        assert!(is_only_signature_of(&block("Minh\nCông ty A"), &known));
        assert!(!is_only_signature_of("Xin chào\n\n-- \nLinh", &known));
        assert!(!is_only_signature_of(
            "\n\n-- \nLinh\n\nA đã viết:\n> hi\n",
            &known
        ));
    }

    /// MB-304: the old check counted any body starting with `-- ` as empty, so text the user
    /// typed below the signature, or a note that starts with `-- `, was never saved.
    #[test]
    fn text_after_or_instead_of_the_signature_is_never_empty() {
        let known = ["Linh"];
        assert!(!is_only_signature_of(
            "\n\n-- \nLinh\n\nNội dung thật bên dưới",
            &known
        ));
        assert!(!is_only_signature_of("\n\n-- \nLinh ghi thêm", &known));
        assert!(!is_only_signature_of("-- \nghi chú riêng của tôi", &known));
        // Without knowing the signature, nothing but whitespace is empty.
        assert!(!is_only_signature_of("\n\n-- \nLinh", &[]));
        assert!(!is_only_signature_of("-- \n", &known));
    }
}
