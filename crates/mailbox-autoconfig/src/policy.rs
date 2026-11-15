//! The SSRF rules, in one place and pure: which addresses, ports and names may be contacted.
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Ports a mail endpoint may use. Everything else is refused before any connection.
pub const MAIL_PORTS: [u16; 4] = [993, 143, 465, 587];
pub const HTTPS_PORT: u16 = 443;

pub fn allowed_mail_port(port: u16) -> bool {
    MAIL_PORTS.contains(&port)
}

fn v4_public(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0                                  // "this" network, unspecified
        || a == 10                            // private
        || (a == 100 && (64..=127).contains(&b)) // CGNAT 100.64/10
        || a == 127                           // loopback
        || (a == 169 && b == 254)             // link-local, cloud metadata
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 0 && c == 0)     // IETF protocol assignments
        || (a == 192 && b == 0 && c == 2)     // TEST-NET-1
        || (a == 192 && b == 88 && c == 99)   // 6to4 relay (deprecated)
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19)) // benchmarking
        || (a == 198 && b == 51 && c == 100)  // TEST-NET-2
        || (a == 203 && b == 0 && c == 113)   // TEST-NET-3
        || a >= 224) // multicast, reserved, broadcast
}

fn v6_public(ip: Ipv6Addr) -> bool {
    // Addresses that embed an IPv4 address are judged by that address.
    if let Some(v4) = ip.to_ipv4_mapped() {
        return v4_public(v4);
    }
    let s = ip.segments();
    if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
        return false;
    }
    // ::/96 (deprecated IPv4-compatible)
    if s[..6].iter().all(|x| *x == 0) {
        return false;
    }
    // 64:ff9b::/96 NAT64 and 64:ff9b:1::/48
    if s[0] == 0x64 && s[1] == 0xff9b {
        if s[2..6].iter().all(|x| *x == 0) {
            let v4 = Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8);
            return v4_public(v4);
        }
        return false;
    }
    // 2002::/16 6to4 embeds an IPv4 address in bits 16..48
    if s[0] == 0x2002 {
        let v4 = Ipv4Addr::new((s[1] >> 8) as u8, s[1] as u8, (s[2] >> 8) as u8, s[2] as u8);
        return v4_public(v4);
    }
    !((s[0] & 0xfe00) == 0xfc00                // fc00::/7 unique local
        || (s[0] & 0xffc0) == 0xfe80           // fe80::/10 link-local
        || (s[0] & 0xffc0) == 0xfec0           // fec0::/10 site-local (deprecated)
        || (s[0] == 0x2001 && s[1] == 0)       // 2001::/32 Teredo
        || (s[0] == 0x2001 && s[1] == 0x0db8)  // documentation
        || (s[0] == 0x2001 && (s[1] & 0xfff0) == 0x0010) // ORCHID
        || (s[0] == 0x0100 && s[1] == 0 && s[2] == 0 && s[3] == 0)) // discard-only
}

/// True only for addresses that are routable on the public internet.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v4_public(v),
        IpAddr::V6(v) => v6_public(v),
    }
}

/// A DNS host name as autoconfiguration may use it: ASCII letters, digits, hyphens, at least two
/// labels, and not an IP address in disguise (`127.1`, `0x7f.1`, `2130706433.com` shapes end in
/// a numeric or hex-looking label and are refused).
pub fn valid_hostname(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() || host.len() > 253 || !host.is_ascii() {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let label_ok = |l: &str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    if !labels.iter().all(|l| label_ok(l)) {
        return false;
    }
    // The top-level label of a real name contains a letter and is not a number or `0x..`.
    let tld = labels[labels.len() - 1];
    tld.chars().any(|c| c.is_ascii_alphabetic()) && !tld.starts_with("0x") && !tld.starts_with("0X")
}

/// The domain part of an address, lowercased, if it is usable. Only this part ever leaves the
/// machine: the local part and the password never reach DNS, an autoconfig server or a probe.
pub fn domain_of(email: &str) -> Option<String> {
    let (local, domain) = email.trim().rsplit_once('@')?;
    if local.is_empty() {
        return None;
    }
    let domain = domain.trim().to_ascii_lowercase();
    valid_hostname(&domain).then_some(domain)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn refuses_every_non_public_ipv4_class() {
        for s in [
            "0.0.0.0",
            "0.1.2.3",
            "10.0.0.1",
            "10.255.255.255",
            "100.64.0.1",
            "100.127.255.254",
            "127.0.0.1",
            "127.255.0.9",
            "169.254.169.254",
            "169.254.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.0.0.5",
            "192.0.2.1",
            "192.88.99.1",
            "192.168.1.1",
            "198.18.0.1",
            "198.19.255.255",
            "198.51.100.7",
            "203.0.113.9",
            "224.0.0.1",
            "239.255.255.250",
            "240.0.0.1",
            "255.255.255.255",
        ] {
            assert!(!is_public_ip(ip(s)), "{s} must be refused");
        }
    }

    #[test]
    fn accepts_ordinary_public_ipv4() {
        for s in [
            "8.8.8.8",
            "1.1.1.1",
            "142.250.80.46",
            "100.63.255.255",
            "100.128.0.1",
            "172.15.0.1",
            "172.32.0.1",
            "198.17.0.1",
            "198.20.0.1",
            "93.184.216.34",
        ] {
            assert!(is_public_ip(ip(s)), "{s} should be allowed");
        }
    }

    #[test]
    fn refuses_every_non_public_ipv6_class() {
        for s in [
            "::",
            "::1",
            "fe80::1",
            "fe80::abcd:1",
            "fc00::1",
            "fd12:3456::1",
            "ff02::1",
            "ff00::",
            "fec0::1",
            "2001:db8::1",
            "2001::1",
            "2001:10::1",
            "100::1",
            // IPv4-mapped / NAT64 / 6to4 wrappers around private and loopback IPv4
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
            "::ffff:192.168.0.1",
            "::ffff:0.0.0.0",
            "::127.0.0.1",
            "64:ff9b::7f00:1",
            "64:ff9b::a00:1",
            "64:ff9b::a9fe:a9fe",
            "64:ff9b:1::1",
            "2002:7f00:1::1",
            "2002:0a00:1::1",
            "2002:a9fe:a9fe::1",
        ] {
            assert!(!is_public_ip(ip(s)), "{s} must be refused");
        }
    }

    #[test]
    fn accepts_ordinary_public_ipv6_and_wrapped_public_ipv4() {
        for s in [
            "2606:4700:4700::1111",
            "2a00:1450:4001:81b::200e",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
            "2002:0808:0808::1",
        ] {
            assert!(is_public_ip(ip(s)), "{s} should be allowed");
        }
    }

    #[test]
    fn only_mail_ports_are_allowed() {
        for p in [993, 143, 465, 587] {
            assert!(allowed_mail_port(p));
        }
        for p in [0, 22, 25, 80, 443, 3306, 6379, 8080, 9200, 65535] {
            assert!(!allowed_mail_port(p), "{p}");
        }
    }

    #[test]
    fn hostnames_must_be_real_names_not_addresses() {
        for ok in [
            "imap.gmail.com",
            "mail.example.co.uk",
            "a-b.example.vn",
            "x.io.",
        ] {
            assert!(valid_hostname(ok), "{ok}");
        }
        for bad in [
            "",
            "localhost",
            "127.0.0.1",
            "10.0.0.1",
            "127.1",
            "0x7f.1",
            "2130706433",
            "1.2.3.4.5",
            "-a.example.com",
            "a-.example.com",
            "a..example.com",
            "exa mple.com",
            "imap.exämple.com",
            "http://imap.example.com",
            "imap.example.com/x",
            "imap.example.com:993",
            "a@b.com",
            "[::1]",
            "::1",
        ] {
            assert!(!valid_hostname(bad), "{bad:?} must be refused");
        }
        assert!(!valid_hostname(&format!("{}.com", "a".repeat(64))));
        assert!(!valid_hostname(&format!("{}.com", "a.".repeat(130))));
    }

    #[test]
    fn domain_extraction_keeps_only_the_domain() {
        assert_eq!(domain_of("Minh@Company.VN").as_deref(), Some("company.vn"));
        assert_eq!(domain_of("a@b@example.com").as_deref(), Some("example.com"));
        for bad in [
            "",
            "no-at",
            "@x.com",
            "a@",
            "a@localhost",
            "a@127.0.0.1",
            "a@b c.com",
        ] {
            assert_eq!(domain_of(bad), None, "{bad:?}");
        }
    }
}
