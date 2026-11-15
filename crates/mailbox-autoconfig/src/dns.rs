//! DNS SRV (RFC 2782 / RFC 6186) wire format: build a query, parse an answer. Pure and
//! bounds-checked: every read is length-checked and name compression cannot loop.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SrvRecord {
    pub priority: u16,
    pub weight: u16,
    pub port: u16,
    pub target: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsError {
    /// The name to query is not a valid DNS name.
    BadName,
    /// Reply is not a valid answer to this query.
    BadReply,
    /// Reply was truncated (TC): not retried over TCP.
    Truncated,
    /// Server answered with an error code (NXDOMAIN counts as "no records", not an error).
    ServerFailure,
}

const TYPE_SRV: u16 = 33;
const CLASS_IN: u16 = 1;

pub fn build_query(id: u16, name: &str) -> Result<Vec<u8>, DnsError> {
    let name = name.strip_suffix('.').unwrap_or(name);
    if name.is_empty() || name.len() > 253 || !name.is_ascii() {
        return Err(DnsError::BadName);
    }
    let mut q = Vec::with_capacity(18 + name.len());
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00]); // RD
    q.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]); // 1 question
    for label in name.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(DnsError::BadName);
        }
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&TYPE_SRV.to_be_bytes());
    q.extend_from_slice(&CLASS_IN.to_be_bytes());
    Ok(q)
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, DnsError> {
    b.get(at..at + 2)
        .map(|s| u16::from_be_bytes([s[0], s[1]]))
        .ok_or(DnsError::BadReply)
}

/// Read a (possibly compressed) name starting at `at`. Returns the dotted name (lowercase) and
/// the offset just after the name in the original position.
fn read_name(b: &[u8], at: usize) -> Result<(String, usize), DnsError> {
    let mut name = String::new();
    let mut pos = at;
    let mut after = None;
    let mut jumps = 0;
    loop {
        let len = *b.get(pos).ok_or(DnsError::BadReply)? as usize;
        if len == 0 {
            pos += 1;
            break;
        }
        if len & 0xc0 == 0xc0 {
            let low = *b.get(pos + 1).ok_or(DnsError::BadReply)? as usize;
            if after.is_none() {
                after = Some(pos + 2);
            }
            jumps += 1;
            if jumps > 16 {
                return Err(DnsError::BadReply);
            }
            pos = ((len & 0x3f) << 8) | low;
            continue;
        }
        if len > 63 {
            return Err(DnsError::BadReply);
        }
        let label = b.get(pos + 1..pos + 1 + len).ok_or(DnsError::BadReply)?;
        if !name.is_empty() {
            name.push('.');
        }
        if name.len() + len > 253 {
            return Err(DnsError::BadReply);
        }
        name.push_str(&String::from_utf8_lossy(label).to_ascii_lowercase());
        pos += 1 + len;
    }
    Ok((name, after.unwrap_or(pos)))
}

/// SRV records of a reply to the query `id`/`qname`. NXDOMAIN or no answers give an empty list.
pub fn parse_reply(reply: &[u8], id: u16) -> Result<Vec<SrvRecord>, DnsError> {
    if reply.len() < 12 || u16_at(reply, 0)? != id {
        return Err(DnsError::BadReply);
    }
    let flags = u16_at(reply, 2)?;
    if flags & 0x8000 == 0 {
        return Err(DnsError::BadReply); // not a response
    }
    if flags & 0x0200 != 0 {
        return Err(DnsError::Truncated);
    }
    match flags & 0x000f {
        0 => {}
        3 => return Ok(Vec::new()), // NXDOMAIN
        _ => return Err(DnsError::ServerFailure),
    }
    let (qd, an) = (u16_at(reply, 4)? as usize, u16_at(reply, 6)? as usize);
    if qd != 1 || an > 64 {
        return Err(DnsError::BadReply);
    }
    let mut pos = 12;
    for _ in 0..qd {
        let (_, next) = read_name(reply, pos)?;
        pos = next + 4;
    }
    let mut out = Vec::new();
    for _ in 0..an {
        let (_, next) = read_name(reply, pos)?;
        let rtype = u16_at(reply, next)?;
        let rdlen = u16_at(reply, next + 8)? as usize;
        let data = next + 10;
        if data + rdlen > reply.len() {
            return Err(DnsError::BadReply);
        }
        if rtype == TYPE_SRV && rdlen >= 7 {
            let (target, _) = read_name(reply, data + 6)?;
            out.push(SrvRecord {
                priority: u16_at(reply, data)?,
                weight: u16_at(reply, data + 2)?,
                port: u16_at(reply, data + 4)?,
                target,
            });
        }
        pos = data + rdlen;
    }
    Ok(out)
}

/// Order records for use: lowest priority first; inside one priority the heavier weight first
/// (a deterministic stand-in for RFC 2782's weighted random pick, so results are repeatable),
/// then by target and port. A target of "." (empty) means "service not available" and is dropped.
pub fn order(mut records: Vec<SrvRecord>) -> Vec<SrvRecord> {
    records.retain(|r| !r.target.is_empty());
    records.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then(b.weight.cmp(&a.weight))
            .then(a.target.cmp(&b.target))
            .then(a.port.cmp(&b.port))
    });
    records
}

#[cfg(test)]
pub(crate) fn encode_name(name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for l in name.split('.').filter(|l| !l.is_empty()) {
        out.push(l.len() as u8);
        out.extend_from_slice(l.as_bytes());
    }
    out.push(0);
    out
}

#[cfg(test)]
pub(crate) fn fake_reply(
    id: u16,
    qname: &str,
    rcode: u16,
    records: &[(u16, u16, u16, &str)],
) -> Vec<u8> {
    let mut r = Vec::new();
    r.extend_from_slice(&id.to_be_bytes());
    r.extend_from_slice(&(0x8180u16 | rcode).to_be_bytes());
    r.extend_from_slice(&[0, 1]);
    r.extend_from_slice(&(records.len() as u16).to_be_bytes());
    r.extend_from_slice(&[0, 0, 0, 0]);
    r.extend_from_slice(&encode_name(qname));
    r.extend_from_slice(&[0, 33, 0, 1]);
    for (prio, weight, port, target) in records {
        r.extend_from_slice(&[0xc0, 12]); // pointer to the question name
        r.extend_from_slice(&[0, 33, 0, 1, 0, 0, 0, 60]);
        let t = encode_name(target);
        r.extend_from_slice(&((6 + t.len()) as u16).to_be_bytes());
        for v in [prio, weight, port] {
            r.extend_from_slice(&v.to_be_bytes());
        }
        r.extend_from_slice(&t);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_has_the_expected_wire_form() {
        let q = build_query(0xabcd, "_imaps._tcp.example.com").unwrap();
        assert_eq!(&q[..4], &[0xab, 0xcd, 1, 0]);
        assert_eq!(&q[4..6], &[0, 1]);
        assert!(q.ends_with(&[0, 0, 33, 0, 1]));
        assert_eq!(q[12], 6); // "_imaps"
        assert!(build_query(1, "").is_err());
        assert!(build_query(1, "a..b").is_err());
        assert!(build_query(1, &format!("{}.com", "a".repeat(64))).is_err());
    }

    #[test]
    fn parses_srv_answers_with_compressed_names() {
        let name = "_imaps._tcp.example.com";
        let reply = fake_reply(
            7,
            name,
            0,
            &[
                (10, 60, 993, "imap.example.com"),
                (20, 0, 993, "backup.example.net"),
            ],
        );
        let recs = parse_reply(&reply, 7).unwrap();
        assert_eq!(recs.len(), 2);
        assert_eq!(
            recs[0],
            SrvRecord {
                priority: 10,
                weight: 60,
                port: 993,
                target: "imap.example.com".into()
            }
        );
    }

    #[test]
    fn nxdomain_is_empty_and_other_errors_are_errors() {
        assert_eq!(
            parse_reply(&fake_reply(1, "a.example.com", 3, &[]), 1).unwrap(),
            vec![]
        );
        assert_eq!(
            parse_reply(&fake_reply(1, "a.example.com", 2, &[]), 1),
            Err(DnsError::ServerFailure)
        );
    }

    #[test]
    fn wrong_id_truncation_and_garbage_are_refused() {
        let r = fake_reply(1, "a.example.com", 0, &[(1, 1, 993, "x.example.com")]);
        assert_eq!(parse_reply(&r, 2), Err(DnsError::BadReply));
        let mut tc = r.clone();
        tc[2] |= 0x02;
        assert_eq!(parse_reply(&tc, 1), Err(DnsError::Truncated));
        assert_eq!(parse_reply(&r[..10], 1), Err(DnsError::BadReply));
        assert!(parse_reply(&r[..r.len() - 3], 1).is_err());
        let mut query_not_response = r.clone();
        query_not_response[2] &= 0x7f;
        assert_eq!(parse_reply(&query_not_response, 1), Err(DnsError::BadReply));
    }

    #[test]
    fn compression_loops_do_not_hang() {
        let mut r = Vec::new();
        r.extend_from_slice(&[0, 1, 0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0]);
        r.extend_from_slice(&[0xc0, 12]); // question name points at itself
        r.extend_from_slice(&[0, 33, 0, 1]);
        assert_eq!(parse_reply(&r, 1), Err(DnsError::BadReply));
    }

    fn rec(p: u16, w: u16, t: &str) -> SrvRecord {
        SrvRecord {
            priority: p,
            weight: w,
            port: 993,
            target: t.into(),
        }
    }

    #[test]
    fn ordering_is_by_priority_then_weight_and_drops_the_null_target() {
        let out = order(vec![
            rec(20, 100, "c.x.com"),
            rec(10, 0, "b.x.com"),
            rec(10, 50, "a.x.com"),
            rec(5, 1, ""),
            rec(10, 50, "0.x.com"),
        ]);
        let targets: Vec<_> = out.iter().map(|r| r.target.as_str()).collect();
        assert_eq!(targets, ["0.x.com", "a.x.com", "b.x.com", "c.x.com"]);
    }
}
