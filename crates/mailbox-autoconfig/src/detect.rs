//! The detection order and the guard every outgoing connection goes through.
//!
//! 1. built-in provider table (no network)
//! 2. DNS SRV `_imaps._tcp`, `_submissions._tcp`, `_submission._tcp` (RFC 6186)
//! 3. Thunderbird autoconfig XML, HTTPS only
//! 4. common host names (`imap.`, `mail.`, `smtp.`) that pass a TLS/greeting probe
//!
//! Every candidate that did not come from the built-in table must pass `Probe` before it is
//! offered, and every connection the real `Resolver`/`Https`/`Probe` make goes through
//! [`checked_addrs`]: resolve once, refuse the host if any address is not public, connect to the
//! addresses that were checked. Only the domain of the address is ever used.
use crate::dns::{self, SrvRecord};
use crate::policy::{allowed_mail_port, domain_of, valid_hostname};
use crate::providers;
use crate::types::{Detection, Endpoint, Found, Security, Service, Source};
use crate::xml;
use std::net::{IpAddr, SocketAddr};

/// At most this many probes per detection, so a hostile domain cannot make us scan.
pub const MAX_PROBES: usize = 12;
/// At most this many SRV targets are tried per service.
pub const MAX_SRV_TRIES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupError {
    NotFound,
    Failed,
    /// This platform has no way to do the lookup.
    Unsupported,
}

#[allow(async_fn_in_trait)]
pub trait Resolver {
    async fn ips(&self, host: &str) -> Result<Vec<IpAddr>, LookupError>;
    async fn srv(&self, name: &str) -> Result<Vec<SrvRecord>, LookupError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    BadName,
    /// At least one resolved address is loopback, private, link-local, CGNAT, multicast, ...
    NonPublic,
    NoAddress,
    Lookup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchError {
    Refused(Refusal),
    Timeout,
    TooLarge,
    /// Any 3xx. Redirects are never followed.
    Redirect,
    Status(u16),
    Protocol,
    Tls,
    Unreachable,
}

#[allow(async_fn_in_trait)]
pub trait Https {
    /// GET `https://{host}{path}` and return the body. Port 443 only.
    async fn get(&self, host: &str, path: &str) -> Result<Vec<u8>, FetchError>;
}

#[allow(async_fn_in_trait)]
pub trait Probe {
    /// True when `endpoint` answers like the given service (and, for implicit TLS, with a valid
    /// certificate for its host name).
    async fn probe(&self, endpoint: &Endpoint, service: Service) -> bool;
}

/// Resolve `host` exactly once and return the addresses a connection may use.
///
/// The whole host is refused when any answer is not public: an attacker-controlled name that
/// returns one public and one internal address must not get a connection to either. The caller
/// connects to the returned addresses and must not resolve the name again (no DNS rebinding).
pub async fn checked_addrs<R: Resolver>(
    resolver: &R,
    host: &str,
    port: u16,
) -> Result<Vec<SocketAddr>, Refusal> {
    if !valid_hostname(host) {
        return Err(Refusal::BadName);
    }
    let ips = resolver.ips(host).await.map_err(|_| Refusal::Lookup)?;
    if ips.is_empty() {
        return Err(Refusal::NoAddress);
    }
    if ips.iter().any(|ip| !crate::policy::is_public_ip(*ip)) {
        return Err(Refusal::NonPublic);
    }
    Ok(ips
        .into_iter()
        .take(4)
        .map(|ip| SocketAddr::new(ip, port))
        .collect())
}

struct Budget(usize);

impl Budget {
    fn take(&mut self) -> bool {
        if self.0 == 0 {
            return false;
        }
        self.0 -= 1;
        true
    }
}

async fn try_set<P: Probe>(
    slot: &mut Option<Found>,
    budget: &mut Budget,
    probe: &P,
    endpoint: Endpoint,
    service: Service,
    source: Source,
) -> bool {
    if slot.is_some() || !valid_hostname(&endpoint.host) || !allowed_mail_port(endpoint.port) {
        return false;
    }
    if !budget.take() || !probe.probe(&endpoint, service).await {
        return false;
    }
    *slot = Some(Found { endpoint, source });
    true
}

/// Valid (port, security) pairs the app can use. IMAP: implicit TLS 993 only (STARTTLS is not
/// supported by the IMAP client). SMTP: implicit TLS 465 or STARTTLS 587.
fn mode_ok(service: Service, port: u16, security: Security) -> bool {
    matches!(
        (service, port, security),
        (Service::Imap, 993, Security::ImplicitTls)
            | (Service::Smtp, 465, Security::ImplicitTls)
            | (Service::Smtp, 587, Security::StartTls)
    )
}

/// Detect the servers for `domain` (already the domain part of the address).
pub async fn detect<R: Resolver, H: Https, P: Probe>(
    domain: &str,
    resolver: &R,
    https: &H,
    probe: &P,
) -> Detection {
    let Some(domain) = domain_of(&format!("x@{domain}")) else {
        return Detection {
            notes: vec!["Tên miền email không hợp lệ."],
            ..Detection::default()
        };
    };
    if let Some(provider) = providers::lookup(&domain) {
        return providers::detection(provider);
    }
    let mut det = Detection::default();
    let mut budget = Budget(MAX_PROBES);

    // 2. DNS SRV
    let mut srv_unsupported = false;
    for (service, label, port, security) in [
        (Service::Imap, "_imaps._tcp", 993, Security::ImplicitTls),
        (
            Service::Smtp,
            "_submissions._tcp",
            465,
            Security::ImplicitTls,
        ),
        (Service::Smtp, "_submission._tcp", 587, Security::StartTls),
    ] {
        let slot_filled = match service {
            Service::Imap => det.imap.is_some(),
            Service::Smtp => det.smtp.is_some(),
        };
        if slot_filled {
            continue;
        }
        match resolver.srv(&format!("{label}.{domain}")).await {
            Ok(records) => {
                for rec in dns::order(records).into_iter().take(MAX_SRV_TRIES) {
                    // The SRV record may only name the port the label stands for.
                    if rec.port != port {
                        continue;
                    }
                    let ep = Endpoint {
                        host: rec.target,
                        port,
                        security,
                    };
                    let slot = if service == Service::Imap {
                        &mut det.imap
                    } else {
                        &mut det.smtp
                    };
                    if try_set(slot, &mut budget, probe, ep, service, Source::DnsSrv).await {
                        break;
                    }
                }
            }
            Err(LookupError::Unsupported) => srv_unsupported = true,
            Err(_) => {}
        }
    }
    if srv_unsupported {
        det.notes
            .push("Hệ thống này chưa hỗ trợ tra cứu DNS SRV; bỏ qua bước đó.");
    }

    // 3. Autoconfig XML over HTTPS
    for (host, path, source) in [
        (
            format!("autoconfig.{domain}"),
            "/mail/config-v1.1.xml",
            Source::AutoconfigSubdomain,
        ),
        (
            domain.clone(),
            "/.well-known/autoconfig/mail/config-v1.1.xml",
            Source::AutoconfigWellKnown,
        ),
    ] {
        if det.is_complete() {
            break;
        }
        let Ok(body) = https.get(&host, path).await else {
            continue;
        };
        let Ok(mut candidates) = xml::parse(&body, &domain) else {
            det.notes
                .push("Một tệp autoconfig bị từ chối vì không an toàn hoặc sai định dạng.");
            continue;
        };
        // Implicit TLS before STARTTLS, document order otherwise.
        candidates.sort_by_key(|c| c.security != Some(Security::ImplicitTls));
        for c in candidates {
            let Some(security) = c.security else { continue };
            if !mode_ok(c.service, c.port, security) {
                continue;
            }
            let ep = Endpoint {
                host: c.host,
                port: c.port,
                security,
            };
            let slot = if c.service == Service::Imap {
                &mut det.imap
            } else {
                &mut det.smtp
            };
            try_set(slot, &mut budget, probe, ep, c.service, source).await;
        }
    }

    // 4. Verified guesses
    for host in [format!("imap.{domain}"), format!("mail.{domain}")] {
        let ep = Endpoint {
            host,
            port: 993,
            security: Security::ImplicitTls,
        };
        if try_set(
            &mut det.imap,
            &mut budget,
            probe,
            ep,
            Service::Imap,
            Source::Guess,
        )
        .await
        {
            break;
        }
    }
    'smtp: for host in [format!("smtp.{domain}"), format!("mail.{domain}")] {
        for (port, security) in [(465, Security::ImplicitTls), (587, Security::StartTls)] {
            let ep = Endpoint {
                host: host.clone(),
                port,
                security,
            };
            if try_set(
                &mut det.smtp,
                &mut budget,
                probe,
                ep,
                Service::Smtp,
                Source::Guess,
            )
            .await
            {
                break 'smtp;
            }
        }
    }

    if det.imap.is_none() && det.smtp.is_none() {
        det.notes.push("Không tự phát hiện được cấu hình. Nhập máy chủ và cổng thủ công theo hướng dẫn của nhà cung cấp.");
    } else if !det.is_complete() {
        det.notes
            .push("Chỉ phát hiện được một phần cấu hình; điền phần còn lại thủ công.");
    }
    det
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct FakeResolver {
        ips: BTreeMap<&'static str, Vec<&'static str>>,
        srv: BTreeMap<String, Vec<SrvRecord>>,
        ip_calls: RefCell<Vec<String>>,
        srv_calls: RefCell<Vec<String>>,
        srv_unsupported: bool,
    }

    impl Resolver for FakeResolver {
        async fn ips(&self, host: &str) -> Result<Vec<IpAddr>, LookupError> {
            self.ip_calls.borrow_mut().push(host.to_string());
            self.ips
                .get(host)
                .map(|v| v.iter().map(|s| s.parse().unwrap()).collect())
                .ok_or(LookupError::NotFound)
        }
        async fn srv(&self, name: &str) -> Result<Vec<SrvRecord>, LookupError> {
            self.srv_calls.borrow_mut().push(name.to_string());
            if self.srv_unsupported {
                return Err(LookupError::Unsupported);
            }
            Ok(self.srv.get(name).cloned().unwrap_or_default())
        }
    }

    #[derive(Default)]
    struct FakeHttps {
        pages: BTreeMap<(String, String), Result<Vec<u8>, FetchError>>,
        calls: RefCell<Vec<(String, String)>>,
    }

    impl Https for FakeHttps {
        async fn get(&self, host: &str, path: &str) -> Result<Vec<u8>, FetchError> {
            self.calls
                .borrow_mut()
                .push((host.to_string(), path.to_string()));
            self.pages
                .get(&(host.to_string(), path.to_string()))
                .cloned()
                .unwrap_or(Err(FetchError::Unreachable))
        }
    }

    struct FakeProbe {
        ok: Vec<(&'static str, u16)>,
        calls: RefCell<Vec<(String, u16)>>,
    }

    impl FakeProbe {
        fn new(ok: &[(&'static str, u16)]) -> Self {
            Self {
                ok: ok.to_vec(),
                calls: RefCell::default(),
            }
        }
    }

    impl Probe for FakeProbe {
        async fn probe(&self, e: &Endpoint, _s: Service) -> bool {
            self.calls.borrow_mut().push((e.host.clone(), e.port));
            self.ok.iter().any(|(h, p)| *h == e.host && *p == e.port)
        }
    }

    fn run<T>(f: impl std::future::Future<Output = T>) -> T {
        use std::task::{Context, Poll, Waker};
        let mut f = std::pin::pin!(f);
        match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("fake futures never pend"),
        }
    }

    fn srv(p: u16, w: u16, port: u16, t: &str) -> SrvRecord {
        SrvRecord {
            priority: p,
            weight: w,
            port,
            target: t.into(),
        }
    }

    const XML: &str = r#"<clientConfig><emailProvider id="x">
      <incomingServer type="imap"><hostname>imap.%EMAILDOMAIN%</hostname><port>993</port><socketType>SSL</socketType></incomingServer>
      <outgoingServer type="smtp"><hostname>smtp.%EMAILDOMAIN%</hostname><port>587</port><socketType>STARTTLS</socketType></outgoingServer>
      </emailProvider></clientConfig>"#;

    // ---- guard ----

    #[test]
    fn guard_resolves_once_and_returns_only_checked_addresses() {
        let mut r = FakeResolver::default();
        r.ips.insert("imap.example.com", vec!["93.184.216.34"]);
        let addrs = run(checked_addrs(&r, "imap.example.com", 993)).unwrap();
        assert_eq!(
            addrs,
            vec!["93.184.216.34:993".parse::<SocketAddr>().unwrap()]
        );
        assert_eq!(
            r.ip_calls.borrow().len(),
            1,
            "a second lookup would allow DNS rebinding"
        );
    }

    #[test]
    fn guard_refuses_a_host_when_any_answer_is_not_public() {
        for bad in [
            "127.0.0.1",
            "10.1.2.3",
            "169.254.169.254",
            "100.64.0.9",
            "192.168.0.1",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:10.0.0.1",
            "224.0.0.1",
            "0.0.0.0",
        ] {
            let mut r = FakeResolver::default();
            r.ips.insert("evil.example.com", vec![bad]);
            assert_eq!(
                run(checked_addrs(&r, "evil.example.com", 993)),
                Err(Refusal::NonPublic),
                "{bad}"
            );
        }
        // Rebinding-style mixed answer: one public, one internal.
        let mut r = FakeResolver::default();
        r.ips
            .insert("mixed.example.com", vec!["93.184.216.34", "127.0.0.1"]);
        assert_eq!(
            run(checked_addrs(&r, "mixed.example.com", 993)),
            Err(Refusal::NonPublic)
        );
    }

    #[test]
    fn guard_refuses_bad_names_without_any_lookup() {
        let r = FakeResolver::default();
        for h in [
            "127.0.0.1",
            "localhost",
            "[::1]",
            "a b.com",
            "x.com/../y",
            "0x7f.0.0.1",
            "",
        ] {
            assert_eq!(run(checked_addrs(&r, h, 993)), Err(Refusal::BadName), "{h}");
        }
        assert!(r.ip_calls.borrow().is_empty());
    }

    #[test]
    fn guard_reports_missing_names() {
        let r = FakeResolver::default();
        assert_eq!(
            run(checked_addrs(&r, "nx.example.com", 993)),
            Err(Refusal::Lookup)
        );
        let mut r = FakeResolver::default();
        r.ips.insert("empty.example.com", vec![]);
        assert_eq!(
            run(checked_addrs(&r, "empty.example.com", 993)),
            Err(Refusal::NoAddress)
        );
    }

    // ---- orchestration ----

    #[test]
    fn built_in_provider_wins_without_any_network_call() {
        let (r, h, p) = (
            FakeResolver::default(),
            FakeHttps::default(),
            FakeProbe::new(&[]),
        );
        let d = run(detect("gmail.com", &r, &h, &p));
        assert!(d.is_complete());
        assert_eq!(d.imap.as_ref().unwrap().source, Source::BuiltIn);
        assert!(d.hint.is_some());
        assert!(
            r.srv_calls.borrow().is_empty()
                && h.calls.borrow().is_empty()
                && p.calls.borrow().is_empty()
        );
    }

    #[test]
    fn srv_is_used_in_priority_then_weight_order_and_first_probe_hit_wins() {
        let mut r = FakeResolver::default();
        r.srv.insert(
            "_imaps._tcp.corp.vn".into(),
            vec![
                srv(20, 5, 993, "backup.corp.vn"),
                srv(10, 1, 993, "light.corp.vn"),
                srv(10, 9, 993, "heavy.corp.vn"),
            ],
        );
        r.srv.insert(
            "_submissions._tcp.corp.vn".into(),
            vec![srv(0, 0, 465, "smtp.corp.vn")],
        );
        let p = FakeProbe::new(&[("light.corp.vn", 993), ("smtp.corp.vn", 465)]);
        let d = run(detect("corp.vn", &r, &FakeHttps::default(), &p));
        assert_eq!(
            d.imap.as_ref().unwrap().endpoint.host,
            "light.corp.vn",
            "heavy failed its probe, next in order wins"
        );
        assert_eq!(
            p.calls.borrow()[..2],
            [
                ("heavy.corp.vn".to_string(), 993),
                ("light.corp.vn".to_string(), 993)
            ]
        );
        assert_eq!(d.smtp.as_ref().unwrap().source, Source::DnsSrv);
        // `_submission._tcp` was not needed once `_submissions` produced a result.
        assert!(
            !r.srv_calls
                .borrow()
                .iter()
                .any(|n| n.starts_with("_submission._tcp"))
        );
    }

    #[test]
    fn srv_records_with_the_wrong_port_or_a_bad_target_are_ignored() {
        let mut r = FakeResolver::default();
        r.srv.insert(
            "_imaps._tcp.corp.vn".into(),
            vec![
                srv(0, 0, 22, "ssh.corp.vn"),
                srv(0, 0, 993, "127.0.0.1"),
                srv(0, 0, 993, "localhost"),
                srv(0, 0, 993, ""),
            ],
        );
        let p = FakeProbe::new(&[("ssh.corp.vn", 22), ("127.0.0.1", 993), ("localhost", 993)]);
        let d = run(detect("corp.vn", &r, &FakeHttps::default(), &p));
        assert!(d.imap.is_none());
        assert!(
            p.calls
                .borrow()
                .iter()
                .all(|(h, port)| h != "127.0.0.1" && h != "localhost" && *port != 22),
            "{:?}",
            p.calls.borrow()
        );
    }

    #[test]
    fn unsupported_srv_lookup_is_noted_and_the_next_steps_still_run() {
        let r = FakeResolver {
            srv_unsupported: true,
            ..Default::default()
        };
        let mut h = FakeHttps::default();
        h.pages.insert(
            ("autoconfig.corp.vn".into(), "/mail/config-v1.1.xml".into()),
            Ok(XML.as_bytes().to_vec()),
        );
        let p = FakeProbe::new(&[("imap.corp.vn", 993), ("smtp.corp.vn", 587)]);
        let d = run(detect("corp.vn", &r, &h, &p));
        assert!(d.is_complete());
        assert_eq!(d.imap.unwrap().source, Source::AutoconfigSubdomain);
        assert!(d.notes.iter().any(|n| n.contains("SRV")));
    }

    #[test]
    fn autoconfig_requests_only_use_the_domain_and_fixed_paths() {
        let r = FakeResolver::default();
        let h = FakeHttps::default();
        run(detect("corp.vn", &r, &h, &FakeProbe::new(&[])));
        let calls = h.calls.borrow();
        assert_eq!(
            *calls,
            vec![
                (
                    "autoconfig.corp.vn".to_string(),
                    "/mail/config-v1.1.xml".to_string()
                ),
                (
                    "corp.vn".to_string(),
                    "/.well-known/autoconfig/mail/config-v1.1.xml".to_string()
                ),
            ]
        );
        for (host, path) in calls.iter() {
            assert!(!host.contains('@') && !path.contains('?') && !path.contains('@'));
        }
    }

    #[test]
    fn xml_candidates_must_pass_policy_and_probe() {
        let evil = r#"<clientConfig>
          <incomingServer type="imap"><hostname>127.0.0.1</hostname><port>993</port><socketType>SSL</socketType></incomingServer>
          <incomingServer type="imap"><hostname>imap.corp.vn</hostname><port>143</port><socketType>STARTTLS</socketType></incomingServer>
          <incomingServer type="imap"><hostname>imap.corp.vn</hostname><port>993</port><socketType>plain</socketType></incomingServer>
          <incomingServer type="imap"><hostname>db.corp.vn</hostname><port>3306</port><socketType>SSL</socketType></incomingServer>
          <outgoingServer type="smtp"><hostname>smtp.corp.vn</hostname><port>25</port><socketType>STARTTLS</socketType></outgoingServer>
          <outgoingServer type="smtp"><hostname>smtp.corp.vn</hostname><port>587</port><socketType>STARTTLS</socketType></outgoingServer>
        </clientConfig>"#;
        let mut h = FakeHttps::default();
        h.pages.insert(
            ("autoconfig.corp.vn".into(), "/mail/config-v1.1.xml".into()),
            Ok(evil.as_bytes().to_vec()),
        );
        let p = FakeProbe::new(&[
            ("smtp.corp.vn", 587),
            ("db.corp.vn", 3306),
            ("127.0.0.1", 993),
            ("smtp.corp.vn", 25),
        ]);
        let d = run(detect("corp.vn", &FakeResolver::default(), &h, &p));
        assert_eq!(d.smtp.unwrap().endpoint.port, 587);
        assert!(d.imap.is_none());
        assert!(
            p.calls
                .borrow()
                .iter()
                .all(|(host, port)| host != "127.0.0.1" && *port != 3306 && *port != 25)
        );
    }

    #[test]
    fn hostile_xml_is_refused_and_noted() {
        let mut h = FakeHttps::default();
        let bomb = r#"<!DOCTYPE x [<!ENTITY a "b">]><x/>"#;
        h.pages.insert(
            ("autoconfig.corp.vn".into(), "/mail/config-v1.1.xml".into()),
            Ok(bomb.as_bytes().to_vec()),
        );
        let d = run(detect(
            "corp.vn",
            &FakeResolver::default(),
            &h,
            &FakeProbe::new(&[]),
        ));
        assert!(d.imap.is_none());
        assert!(d.notes.iter().any(|n| n.contains("từ chối")));
    }

    #[test]
    fn fetch_failures_move_on_to_the_next_source() {
        let mut h = FakeHttps::default();
        h.pages.insert(
            ("autoconfig.corp.vn".into(), "/mail/config-v1.1.xml".into()),
            Err(FetchError::Redirect),
        );
        h.pages.insert(
            (
                "corp.vn".into(),
                "/.well-known/autoconfig/mail/config-v1.1.xml".into(),
            ),
            Ok(XML.as_bytes().to_vec()),
        );
        let p = FakeProbe::new(&[("imap.corp.vn", 993), ("smtp.corp.vn", 587)]);
        let d = run(detect("corp.vn", &FakeResolver::default(), &h, &p));
        assert_eq!(d.imap.unwrap().source, Source::AutoconfigWellKnown);
    }

    #[test]
    fn guesses_are_offered_only_when_the_probe_confirms_them() {
        let p = FakeProbe::new(&[("mail.corp.vn", 993), ("mail.corp.vn", 465)]);
        let d = run(detect(
            "corp.vn",
            &FakeResolver::default(),
            &FakeHttps::default(),
            &p,
        ));
        assert_eq!(d.imap.as_ref().unwrap().endpoint.host, "mail.corp.vn");
        assert_eq!(d.smtp.as_ref().unwrap().source, Source::Guess);
        let none = run(detect(
            "corp.vn",
            &FakeResolver::default(),
            &FakeHttps::default(),
            &FakeProbe::new(&[]),
        ));
        assert!(none.imap.is_none() && none.smtp.is_none());
        assert!(none.notes.iter().any(|n| n.contains("thủ công")));
    }

    #[test]
    fn probe_budget_is_bounded() {
        let mut r = FakeResolver::default();
        let many: Vec<SrvRecord> = (0..50)
            .map(|i| srv(i, 0, 993, &format!("h{i}.corp.vn")))
            .collect();
        r.srv.insert("_imaps._tcp.corp.vn".into(), many);
        let p = FakeProbe::new(&[]);
        run(detect("corp.vn", &r, &FakeHttps::default(), &p));
        let srv_probes = p
            .calls
            .borrow()
            .iter()
            .filter(|(h, _)| h.starts_with('h'))
            .count();
        assert_eq!(
            srv_probes, MAX_SRV_TRIES,
            "only the first few SRV targets are tried"
        );
        assert!(
            p.calls.borrow().len() <= MAX_PROBES,
            "{}",
            p.calls.borrow().len()
        );
    }

    #[test]
    fn invalid_domains_do_nothing() {
        let (r, h, p) = (
            FakeResolver::default(),
            FakeHttps::default(),
            FakeProbe::new(&[]),
        );
        for d in ["", "localhost", "127.0.0.1", "a b.com"] {
            let det = run(detect(d, &r, &h, &p));
            assert!(det.imap.is_none() && !det.notes.is_empty(), "{d:?}");
        }
        assert!(
            r.srv_calls.borrow().is_empty()
                && h.calls.borrow().is_empty()
                && p.calls.borrow().is_empty()
        );
    }
}
