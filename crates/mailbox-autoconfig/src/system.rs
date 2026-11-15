//! The real resolver (system lookup for addresses, UDP DNS for SRV) and the real probe.
use crate::detect::{LookupError, Probe, Resolver, checked_addrs};
use crate::dns::{self, DnsError, SrvRecord};
use crate::types::{Endpoint, Security, Service};
use mailbox_core::{MailServerEndpoint, TlsMode};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpStream, UdpSocket, lookup_host};

const LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const GREETING_TIMEOUT: Duration = Duration::from_secs(6);

/// Address lookup through the operating system; SRV through the system's DNS servers over UDP.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemResolver;

fn random_id() -> u16 {
    let mut h = RandomState::new().build_hasher();
    h.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0),
    );
    h.finish() as u16
}

/// Name servers from `/etc/resolv.conf` (Unix). Empty elsewhere: SRV is then reported as unsupported.
fn nameservers() -> Vec<IpAddr> {
    let Ok(text) = std::fs::read_to_string("/etc/resolv.conf") else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("nameserver"))
        .filter_map(|v| v.trim().split('%').next()?.parse::<IpAddr>().ok())
        .take(3)
        .collect()
}

async fn query_srv(server: IpAddr, name: &str) -> Result<Vec<SrvRecord>, LookupError> {
    let id = random_id();
    let query = dns::build_query(id, name).map_err(|_| LookupError::Failed)?;
    let bind: SocketAddr = if server.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    }
    .parse()
    .map_err(|_| LookupError::Failed)?;
    let socket = UdpSocket::bind(bind)
        .await
        .map_err(|_| LookupError::Failed)?;
    // `connect` makes the kernel drop datagrams from any other source address.
    socket
        .connect(SocketAddr::new(server, 53))
        .await
        .map_err(|_| LookupError::Failed)?;
    socket.send(&query).await.map_err(|_| LookupError::Failed)?;
    let mut buf = [0u8; 4096];
    let n = tokio::time::timeout(LOOKUP_TIMEOUT, socket.recv(&mut buf))
        .await
        .map_err(|_| LookupError::Failed)?
        .map_err(|_| LookupError::Failed)?;
    match dns::parse_reply(&buf[..n], id) {
        Ok(records) => Ok(records),
        Err(
            DnsError::BadName | DnsError::BadReply | DnsError::Truncated | DnsError::ServerFailure,
        ) => Err(LookupError::Failed),
    }
}

impl Resolver for SystemResolver {
    async fn ips(&self, host: &str) -> Result<Vec<IpAddr>, LookupError> {
        let found = tokio::time::timeout(LOOKUP_TIMEOUT, lookup_host((host, 0)))
            .await
            .map_err(|_| LookupError::Failed)?
            .map_err(|_| LookupError::NotFound)?;
        let mut ips: Vec<IpAddr> = found.map(|a| a.ip()).collect();
        ips.sort();
        ips.dedup();
        Ok(ips)
    }

    async fn srv(&self, name: &str) -> Result<Vec<SrvRecord>, LookupError> {
        let servers = nameservers();
        if servers.is_empty() {
            return Err(LookupError::Unsupported);
        }
        let mut last = LookupError::Failed;
        for s in servers {
            match query_srv(s, name).await {
                Ok(r) => return Ok(r),
                Err(e) => last = e,
            }
        }
        Err(last)
    }
}

/// Probe: resolve (checked), connect to a checked address, verified TLS for implicit modes, and
/// require a plausible protocol greeting. Reads at most a few hundred bytes. Sends nothing.
pub struct TlsProbe<R> {
    pub resolver: R,
}

fn greeting_ok(service: Service, bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes).to_ascii_uppercase();
    match service {
        Service::Imap => text.starts_with("* OK") || text.starts_with("* PREAUTH"),
        Service::Smtp => text.starts_with("220"),
    }
}

async fn read_greeting<S: AsyncReadExt + Unpin>(stream: &mut S, service: Service) -> bool {
    let mut buf = [0u8; 512];
    match tokio::time::timeout(GREETING_TIMEOUT, stream.read(&mut buf)).await {
        Ok(Ok(n)) if n > 0 => greeting_ok(service, &buf[..n]),
        _ => false,
    }
}

impl<R: Resolver> Probe for TlsProbe<R> {
    async fn probe(&self, endpoint: &Endpoint, service: Service) -> bool {
        let Ok(addrs) = checked_addrs(&self.resolver, &endpoint.host, endpoint.port).await else {
            return false;
        };
        let target = MailServerEndpoint {
            hostname: endpoint.host.clone(),
            port: endpoint.port,
            tls_mode: if endpoint.security == Security::ImplicitTls {
                TlsMode::ImplicitTls
            } else {
                TlsMode::StartTls
            },
        };
        for addr in addrs {
            let Ok(Ok(tcp)) =
                tokio::time::timeout(PROBE_CONNECT_TIMEOUT, TcpStream::connect(addr)).await
            else {
                continue;
            };
            return match endpoint.security {
                Security::ImplicitTls => match mailbox_net::upgrade_tls(tcp, &target).await {
                    Ok(mut tls) => read_greeting(&mut tls, service).await,
                    Err(_) => false,
                },
                // STARTTLS: only the plain greeting is read; the certificate is checked by the
                // real connection test the user starts next (nothing is sent in clear text).
                Security::StartTls => {
                    let mut tcp = tcp;
                    read_greeting(&mut tcp, service).await
                }
            };
        }
        false
    }
}

/// Detect with the real resolver, HTTPS client and probe.
pub async fn detect_system(domain: &str) -> crate::Detection {
    let https = crate::http::HttpsClient {
        resolver: SystemResolver,
    };
    let probe = TlsProbe {
        resolver: SystemResolver,
    };
    crate::detect(domain, &SystemResolver, &https, &probe).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greetings_are_checked_per_service() {
        assert!(greeting_ok(Service::Imap, b"* OK Dovecot ready.\r\n"));
        assert!(greeting_ok(Service::Imap, b"* ok imap\r\n"));
        assert!(!greeting_ok(
            Service::Imap,
            b"220 smtp.example.com ESMTP\r\n"
        ));
        assert!(greeting_ok(
            Service::Smtp,
            b"220 smtp.example.com ESMTP\r\n"
        ));
        assert!(!greeting_ok(Service::Smtp, b"* OK imap\r\n"));
        assert!(!greeting_ok(Service::Imap, b"HTTP/1.1 400 Bad Request\r\n"));
        assert!(!greeting_ok(Service::Imap, b""));
    }

    #[tokio::test]
    async fn probe_never_connects_to_a_private_resolution() {
        struct Local;
        impl Resolver for Local {
            async fn ips(&self, _: &str) -> Result<Vec<IpAddr>, LookupError> {
                Ok(vec!["127.0.0.1".parse().unwrap()])
            }
            async fn srv(&self, _: &str) -> Result<Vec<SrvRecord>, LookupError> {
                Err(LookupError::Failed)
            }
        }
        // A listener on loopback: if the probe connected it would see a connection.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepted = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_millis(400), listener.accept())
                .await
                .is_ok()
        });
        let probe = TlsProbe { resolver: Local };
        // Port is not one the guard cares about; the host resolves to loopback, so it must be refused.
        let ep = Endpoint {
            host: "imap.example.com".into(),
            port,
            security: Security::ImplicitTls,
        };
        assert!(!probe.probe(&ep, Service::Imap).await);
        assert!(
            !accepted.await.unwrap(),
            "the probe must not have connected to loopback"
        );
    }
}
