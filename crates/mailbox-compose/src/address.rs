//! Email address validation and parsing. One implementation, used by the message builder and by
//! the composer's recipient chips, so the UI never accepts what the builder would refuse.
//!
//! The check is structural (`local@domain.tld`): deliverability is the SMTP server's job.
//! Quoted local parts and comments are deliberately not supported.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressError {
    Empty,
    Syntax,
    /// CR, LF, NUL or another control character: rejected, never stripped.
    ControlCharacter,
    TooLong,
}

/// A display name (may be empty) and an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mailbox {
    pub name: String,
    pub address: String,
}

impl Mailbox {
    /// False when the address needs SMTPUTF8 (non-ASCII local part or domain).
    pub fn is_ascii(&self) -> bool {
        self.address.is_ascii()
    }

    /// What to show for this recipient: the name, or the address when there is none.
    pub fn display(&self) -> &str {
        if self.name.is_empty() {
            &self.address
        } else {
            &self.name
        }
    }
}

const ATEXT: &str = "!#$%&'*+-/=?^_`{|}~";

fn local_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || ATEXT.contains(c)
        || c == '.'
        || (!c.is_ascii() && !c.is_whitespace())
}

fn domain_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || (!c.is_ascii() && !c.is_whitespace())
}

/// Validate a bare address (`local@domain`), without display name or angle brackets.
pub fn validate_address(address: &str) -> Result<(), AddressError> {
    if address.is_empty() {
        return Err(AddressError::Empty);
    }
    if address.chars().any(char::is_control) {
        return Err(AddressError::ControlCharacter);
    }
    if address.len() > 254 {
        return Err(AddressError::TooLong);
    }
    let (local, domain) = address.split_once('@').ok_or(AddressError::Syntax)?;
    let local_ok = !local.is_empty()
        && local.len() <= 64
        && local.chars().all(local_char)
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..");
    let labels: Vec<&str> = domain.split('.').collect();
    let domain_ok = labels.len() >= 2
        && labels.iter().all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && l.chars().all(domain_char)
                && !l.starts_with('-')
                && !l.ends_with('-')
        });
    if local_ok && domain_ok {
        Ok(())
    } else {
        Err(AddressError::Syntax)
    }
}

pub fn is_valid_address(address: &str) -> bool {
    validate_address(address).is_ok()
}

/// Parse `addr`, `Name <addr>` or `"Quoted, Name" <addr>`.
pub fn parse_mailbox(input: &str) -> Result<Mailbox, AddressError> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(AddressError::Empty);
    }
    if raw.chars().any(char::is_control) {
        return Err(AddressError::ControlCharacter);
    }
    let (name, rest) = if let Some(quoted) = raw.strip_prefix('"') {
        let mut name = String::new();
        let mut end = None;
        let mut escaped = false;
        for (i, c) in quoted.char_indices() {
            match (escaped, c) {
                (true, _) => {
                    name.push(c);
                    escaped = false;
                }
                (false, '\\') => escaped = true,
                (false, '"') => {
                    end = Some(i + 1);
                    break;
                }
                _ => name.push(c),
            }
        }
        let end = end.ok_or(AddressError::Syntax)?;
        (name, quoted[end..].trim())
    } else if let Some(open) = raw.find('<') {
        (raw[..open].trim().to_string(), &raw[open..])
    } else {
        (String::new(), raw)
    };
    let address = if name.is_empty() && !rest.starts_with('<') {
        rest
    } else {
        rest.strip_prefix('<')
            .and_then(|r| r.strip_suffix('>'))
            .ok_or(AddressError::Syntax)?
            .trim()
    };
    if name.len() > 256 {
        return Err(AddressError::TooLong);
    }
    validate_address(address).map_err(|e| match e {
        AddressError::Empty => AddressError::Syntax,
        other => other,
    })?;
    Ok(Mailbox {
        name,
        address: address.to_string(),
    })
}

/// Split a typed or pasted recipient list on `,`, `;` and line breaks, ignoring separators
/// inside quotes or angle brackets. Entries are trimmed; empty ones are dropped.
pub fn split_list(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let (mut quoted, mut escaped, mut angle) = (false, false, false);
    for c in input.chars() {
        let separator = !quoted && !angle && matches!(c, ',' | ';' | '\n' | '\r');
        if separator {
            out.push(std::mem::take(&mut current));
            continue;
        }
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            _ => {}
        }
        current.push(c);
    }
    out.push(current);
    out.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
