//! Subject prefixes and quoting for reply and forward.

fn strip_prefixes(subject: &str) -> &str {
    let mut s = subject.trim();
    loop {
        let lower = s.to_ascii_lowercase();
        let cut = ["re:", "fwd:", "fw:"]
            .iter()
            .find(|p| lower.starts_with(**p))
            .map(|p| p.len());
        match cut {
            Some(n) => s = s[n..].trim_start(),
            None => return s,
        }
    }
}

/// `Re: <subject>` without stacking prefixes.
pub fn reply_subject(subject: &str) -> String {
    format!("Re: {}", strip_prefixes(subject))
}

/// `Fwd: <subject>` without stacking prefixes.
pub fn forward_subject(subject: &str) -> String {
    format!("Fwd: {}", strip_prefixes(subject))
}

/// Quote `body` below an attribution line, leaving room above for the reply.
pub fn quote(attribution: &str, body: &str) -> String {
    let mut out = format!("\n\n{attribution}\n");
    for line in body.lines() {
        if line.is_empty() {
            out.push('>');
        } else if line.starts_with('>') {
            out.push('>');
            out.push_str(line);
        } else {
            out.push_str("> ");
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}
