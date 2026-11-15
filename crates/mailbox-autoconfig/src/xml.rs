//! Thunderbird autoconfig (`config-v1.1.xml`) reader, written for untrusted input.
//!
//! No XML library and no entity handling at all: a document that declares a DTD or an entity is
//! refused outright, so external entities, entity expansion and XInclude-style tricks have nothing
//! to work with. Only text inside a handful of known elements is read, and every value is
//! validated by the caller's policy before use.
use crate::types::{Security, Service};

pub const MAX_XML_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlError {
    TooLarge,
    NotText,
    /// DOCTYPE, ENTITY, processing instruction other than `<?xml`, or any `<!` construct.
    ForbiddenConstruct,
    Malformed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub service: Service,
    pub host: String,
    pub port: u16,
    /// `None` = plain text (never accepted later) or unknown.
    pub security: Option<Security>,
}

#[derive(Default)]
struct Draft {
    host: String,
    port: String,
    socket: String,
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = tag;
    while let Some(i) = rest.find(name) {
        let before_ok = rest[..i].chars().last().is_none_or(|c| c.is_whitespace());
        let after = &rest[i + name.len()..];
        let after_trim = after.trim_start();
        if before_ok && let Some(v) = after_trim.strip_prefix('=') {
            let v = v.trim_start();
            let quote = v.chars().next()?;
            if quote == '"' || quote == '\'' {
                let body = &v[1..];
                return body.find(quote).map(|end| &body[..end]);
            }
        }
        rest = after;
    }
    None
}

/// Decode only the five predefined entities; any other `&` makes the value unusable.
fn text_value(raw: &str) -> Option<String> {
    let t = raw.trim();
    if !t.contains('&') {
        return Some(t.to_string());
    }
    let mut out = String::new();
    let mut rest = t;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let (rep, len) = if tail.starts_with("&amp;") {
            ('&', 5)
        } else if tail.starts_with("&lt;") {
            ('<', 4)
        } else if tail.starts_with("&gt;") {
            ('>', 4)
        } else if tail.starts_with("&quot;") {
            ('"', 6)
        } else if tail.starts_with("&apos;") {
            ('\'', 6)
        } else {
            return None;
        };
        out.push(rep);
        rest = &tail[len..];
    }
    out.push_str(rest);
    Some(out)
}

/// Replace `%EMAILDOMAIN%`. A host that wants the local part or the full address is refused:
/// those must never be placed into a name that is looked up or connected to.
fn expand_host(host: &str, domain: &str) -> Option<String> {
    let upper = host.to_ascii_uppercase();
    if upper.contains("%EMAILLOCALPART%") || upper.contains("%EMAILADDRESS%") {
        return None;
    }
    let mut out = String::new();
    let mut rest = host;
    loop {
        let up = rest.to_ascii_uppercase();
        match up.find("%EMAILDOMAIN%") {
            Some(i) => {
                out.push_str(&rest[..i]);
                out.push_str(domain);
                rest = &rest[i + "%EMAILDOMAIN%".len()..];
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    (!out.contains('%')).then_some(out.to_ascii_lowercase())
}

/// Candidates in document order. `domain` replaces `%EMAILDOMAIN%`.
pub fn parse(bytes: &[u8], domain: &str) -> Result<Vec<Candidate>, XmlError> {
    if bytes.len() > MAX_XML_BYTES {
        return Err(XmlError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| XmlError::NotText)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Vec::new();
    let mut current: Option<(Service, Draft)> = None;
    let mut field: Option<&'static str> = None;
    let mut i = 0;
    while let Some(lt) = text[i..].find('<') {
        // Text before this tag belongs to the open field.
        if let (Some(f), Some((_, d))) = (field, current.as_mut()) {
            let t = &text[i..i + lt];
            let slot = match f {
                "hostname" => &mut d.host,
                "port" => &mut d.port,
                _ => &mut d.socket,
            };
            slot.push_str(t);
        }
        let start = i + lt;
        let rest = &text[start..];
        if rest.starts_with("<!--") {
            let end = rest.find("-->").ok_or(XmlError::Malformed)?;
            i = start + end + 3;
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            return Err(XmlError::ForbiddenConstruct);
        }
        if rest.starts_with("<!") {
            return Err(XmlError::ForbiddenConstruct);
        }
        if rest.starts_with("<?") {
            // Only the XML declaration is tolerated.
            let end = rest.find("?>").ok_or(XmlError::Malformed)?;
            let decl = rest[..end].to_ascii_lowercase();
            let is_decl = decl
                .strip_prefix("<?xml")
                .is_some_and(|after| after.is_empty() || after.starts_with(char::is_whitespace));
            if !is_decl {
                return Err(XmlError::ForbiddenConstruct);
            }
            i = start + end + 2;
            continue;
        }
        let end = rest.find('>').ok_or(XmlError::Malformed)?;
        let tag = &rest[1..end];
        i = start + end + 1;
        let (closing, body) = match tag.strip_prefix('/') {
            Some(b) => (true, b),
            None => (false, tag),
        };
        let self_closing = body.ends_with('/');
        let name = body
            .trim_end_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if closing {
            match name.as_str() {
                "hostname" | "port" | "sockettype" => field = None,
                "incomingserver" | "outgoingserver" => {
                    if let Some((service, d)) = current.take()
                        && let Some(host) =
                            text_value(&d.host).and_then(|h| expand_host(&h, domain))
                        && let Ok(port) = d.port.trim().parse::<u16>()
                    {
                        let security = match d.socket.trim().to_ascii_uppercase().as_str() {
                            "SSL" | "TLS" => Some(Security::ImplicitTls),
                            "STARTTLS" => Some(Security::StartTls),
                            _ => None,
                        };
                        out.push(Candidate {
                            service,
                            host,
                            port,
                            security,
                        });
                    }
                }
                _ => {}
            }
            continue;
        }
        match name.as_str() {
            "incomingserver" if !self_closing => {
                current = match attr(body, "type") {
                    Some(t) if t.eq_ignore_ascii_case("imap") => {
                        Some((Service::Imap, Draft::default()))
                    }
                    _ => None,
                };
            }
            "outgoingserver" if !self_closing => {
                current = match attr(body, "type") {
                    Some(t) if t.eq_ignore_ascii_case("smtp") => {
                        Some((Service::Smtp, Draft::default()))
                    }
                    _ => None,
                };
            }
            "hostname" if current.is_some() && !self_closing => field = Some("hostname"),
            "port" if current.is_some() && !self_closing => field = Some("port"),
            "sockettype" if current.is_some() && !self_closing => field = Some("sockettype"),
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<clientConfig version="1.1">
  <emailProvider id="example.com">
    <!-- comment <hostname>evil.example</hostname> -->
    <incomingServer type="pop3"><hostname>pop.example.com</hostname><port>995</port><socketType>SSL</socketType></incomingServer>
    <incomingServer type="imap">
      <hostname>imap.%EMAILDOMAIN%</hostname><port>993</port><socketType>SSL</socketType>
      <username>%EMAILADDRESS%</username><authentication>password-cleartext</authentication>
    </incomingServer>
    <outgoingServer type="smtp">
      <hostname>smtp.example.com</hostname><port>587</port><socketType>STARTTLS</socketType>
    </outgoingServer>
  </emailProvider>
</clientConfig>"#;

    #[test]
    fn reads_imap_and_smtp_and_skips_pop3_and_comments() {
        let c = parse(GOOD.as_bytes(), "example.com").unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(
            c[0],
            Candidate {
                service: Service::Imap,
                host: "imap.example.com".into(),
                port: 993,
                security: Some(Security::ImplicitTls)
            }
        );
        assert_eq!(c[1].service, Service::Smtp);
        assert_eq!(c[1].security, Some(Security::StartTls));
    }

    #[test]
    fn documents_with_a_dtd_or_entities_are_refused() {
        let xxe = r#"<?xml version="1.0"?><!DOCTYPE x [<!ENTITY e SYSTEM "file:///etc/passwd">]><clientConfig><emailProvider><incomingServer type="imap"><hostname>&e;</hostname><port>993</port><socketType>SSL</socketType></incomingServer></emailProvider></clientConfig>"#;
        assert_eq!(
            parse(xxe.as_bytes(), "x.com"),
            Err(XmlError::ForbiddenConstruct)
        );
        let bomb = r#"<!DOCTYPE lolz [<!ENTITY a "aaaa"><!ENTITY b "&a;&a;&a;">]><x/>"#;
        assert_eq!(
            parse(bomb.as_bytes(), "x.com"),
            Err(XmlError::ForbiddenConstruct)
        );
        let lower = "<!doctype x><x/>";
        assert_eq!(
            parse(lower.as_bytes(), "x.com"),
            Err(XmlError::ForbiddenConstruct)
        );
        let pi = r#"<?xml-stylesheet href="x"?><x/>"#;
        assert_eq!(
            parse(pi.as_bytes(), "x.com"),
            Err(XmlError::ForbiddenConstruct)
        );
        let cdata = "<x><![CDATA[<hostname>a.b.c</hostname>]]></x>";
        assert_eq!(
            parse(cdata.as_bytes(), "x.com"),
            Err(XmlError::ForbiddenConstruct)
        );
    }

    #[test]
    fn a_stray_entity_reference_makes_the_value_unusable_without_expanding_it() {
        let x = r#"<c><incomingServer type="imap"><hostname>imap.&e;.com</hostname><port>993</port><socketType>SSL</socketType></incomingServer></c>"#;
        assert!(parse(x.as_bytes(), "x.com").unwrap().is_empty());
        let ok = r#"<c><incomingServer type="imap"><hostname>imap.a&amp;b.com</hostname><port>993</port><socketType>SSL</socketType></incomingServer></c>"#;
        assert_eq!(
            parse(ok.as_bytes(), "x.com").unwrap()[0].host,
            "imap.a&b.com"
        );
    }

    #[test]
    fn local_part_and_address_never_enter_a_host_name() {
        for h in [
            "%EMAILLOCALPART%.example.com",
            "imap.%EMAILADDRESS%",
            "%emaillocalpart%.x.com",
        ] {
            let x = format!(
                r#"<c><incomingServer type="imap"><hostname>{h}</hostname><port>993</port><socketType>SSL</socketType></incomingServer></c>"#
            );
            assert!(parse(x.as_bytes(), "x.com").unwrap().is_empty(), "{h}");
        }
        let x = r#"<c><incomingServer type="imap"><hostname>%UNKNOWN%.x.com</hostname><port>993</port><socketType>SSL</socketType></incomingServer></c>"#;
        assert!(parse(x.as_bytes(), "x.com").unwrap().is_empty());
    }

    #[test]
    fn size_and_encoding_limits() {
        assert_eq!(
            parse(&vec![b' '; MAX_XML_BYTES + 1], "x.com"),
            Err(XmlError::TooLarge)
        );
        assert_eq!(parse(&[0xff, 0xfe, 0x00], "x.com"), Err(XmlError::NotText));
        assert_eq!(parse(b"<a", "x.com"), Err(XmlError::Malformed));
        assert!(parse(b"", "x.com").unwrap().is_empty());
    }

    #[test]
    fn plain_text_and_bad_ports_come_through_as_data_for_the_policy_to_judge() {
        let x = r#"<c><incomingServer type="imap"><hostname>imap.x.com</hostname><port>143</port><socketType>plain</socketType></incomingServer><incomingServer type="imap"><hostname>imap.x.com</hostname><port>99999</port><socketType>SSL</socketType></incomingServer></c>"#;
        let c = parse(x.as_bytes(), "x.com").unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].security, None);
    }
}
