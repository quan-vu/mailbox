//! Transport for mail protocols. The only way to get an encrypted stream out of this crate is
//! through full certificate validation (chain, validity period, host name) against the bundled
//! Mozilla roots. There is deliberately no option to relax or skip it.
use mailbox_core::MailServerEndpoint;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

/// Per-step limit. Some providers take close to 10 s to finish the TLS handshake (reverse DNS
/// on connect), so this is generous while still bounded.
pub const STEP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetError {
    /// Host name or port is not acceptable (checked before any network access).
    InvalidEndpoint,
    /// DNS failure, refused connection or unreachable network.
    Unreachable,
    Timeout,
    /// The server certificate is not valid for this host. Never bypassable.
    CertificateInvalid,
    /// TLS could not be established for another reason.
    TlsFailed,
}

pub type Tls = TlsStream<TcpStream>;

/// Run one protocol step with the shared timeout.
pub async fn step<T, E: From<NetError>>(
    future: impl Future<Output = Result<T, E>>,
) -> Result<T, E> {
    match tokio::time::timeout(STEP_TIMEOUT, future).await {
        Ok(result) => result,
        Err(_) => Err(NetError::Timeout.into()),
    }
}

fn tls_config() -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

fn host_of(endpoint: &MailServerEndpoint) -> Result<String, NetError> {
    endpoint
        .validate_shape()
        .map_err(|_| NetError::InvalidEndpoint)?;
    Ok(endpoint.hostname.trim().to_string())
}

/// Plain TCP connection. Only for protocols that upgrade with STARTTLS before anything sensitive.
pub async fn connect_tcp(endpoint: &MailServerEndpoint) -> Result<TcpStream, NetError> {
    let host = host_of(endpoint)?;
    step(async {
        TcpStream::connect((host.as_str(), endpoint.port))
            .await
            .map_err(|_| NetError::Unreachable)
    })
    .await
}

/// Upgrade a TCP connection to verified TLS for `endpoint`'s host name.
pub async fn upgrade_tls(tcp: TcpStream, endpoint: &MailServerEndpoint) -> Result<Tls, NetError> {
    let host = host_of(endpoint)?;
    let name = ServerName::try_from(host).map_err(|_| NetError::InvalidEndpoint)?;
    step(async {
        TlsConnector::from(tls_config())
            .connect(name, tcp)
            .await
            .map_err(|e| {
                let invalid_cert = e
                    .get_ref()
                    .and_then(|inner| inner.downcast_ref::<tokio_rustls::rustls::Error>())
                    .is_some_and(|tls| {
                        matches!(tls, tokio_rustls::rustls::Error::InvalidCertificate(_))
                    });
                if invalid_cert {
                    NetError::CertificateInvalid
                } else {
                    NetError::TlsFailed
                }
            })
    })
    .await
}

/// Connect and immediately secure the connection (implicit TLS, e.g. IMAPS 993, SMTPS 465).
pub async fn connect_tls(endpoint: &MailServerEndpoint) -> Result<Tls, NetError> {
    upgrade_tls(connect_tcp(endpoint).await?, endpoint).await
}

/// Plain TCP to addresses the caller already resolved and checked (SSRF guard). The host name is
/// not resolved again, so a DNS answer that changes after the check cannot redirect the
/// connection. The addresses are tried in order; the endpoint's port must match theirs.
pub async fn connect_tcp_pinned(
    endpoint: &MailServerEndpoint,
    addrs: &[std::net::SocketAddr],
) -> Result<TcpStream, NetError> {
    host_of(endpoint)?;
    if addrs.is_empty() || addrs.iter().any(|a| a.port() != endpoint.port) {
        return Err(NetError::InvalidEndpoint);
    }
    let mut last = NetError::Unreachable;
    for addr in addrs {
        match step(async {
            TcpStream::connect(addr)
                .await
                .map_err(|_| NetError::Unreachable)
        })
        .await
        {
            Ok(tcp) => return Ok(tcp),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Implicit TLS over a pinned address. The certificate is still validated for the host *name*.
pub async fn connect_tls_pinned(
    endpoint: &MailServerEndpoint,
    addrs: &[std::net::SocketAddr],
) -> Result<Tls, NetError> {
    upgrade_tls(connect_tcp_pinned(endpoint, addrs).await?, endpoint).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailbox_core::TlsMode;
    use std::net::SocketAddr;

    fn endpoint(host: &str, port: u16) -> MailServerEndpoint {
        MailServerEndpoint {
            hostname: host.into(),
            port,
            tls_mode: TlsMode::ImplicitTls,
        }
    }

    /// The pinned connect goes to the checked address and never resolves the name again: the
    /// host below does not exist in DNS (`.invalid`, RFC 6761), yet the loopback listener sees
    /// the connection. A DNS rebinding answer between check and connect therefore has no effect.
    #[tokio::test]
    async fn pinned_connect_uses_the_checked_address_without_dns() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accepted = tokio::spawn(async move { listener.accept().await.is_ok() });
        let tcp = connect_tcp_pinned(&endpoint("imap.rebind.invalid", addr.port()), &[addr]).await;
        assert!(tcp.is_ok(), "{tcp:?}");
        assert!(accepted.await.unwrap());
    }

    #[tokio::test]
    async fn pinned_connect_refuses_an_empty_list_and_a_bad_host() {
        let addr: SocketAddr = "127.0.0.1:9".parse().unwrap();
        assert_eq!(
            connect_tcp_pinned(&endpoint("imap.example.com", 993), &[])
                .await
                .err(),
            Some(NetError::InvalidEndpoint)
        );
        assert_eq!(
            connect_tcp_pinned(&endpoint("bad host", 993), &[addr])
                .await
                .err(),
            Some(NetError::InvalidEndpoint)
        );
    }

    /// TLS over a pinned address still validates the certificate for the host *name*: a plain
    /// TCP peer that does not speak TLS never yields a stream.
    #[tokio::test]
    async fn pinned_tls_still_requires_a_valid_handshake() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut s, _)) = listener.accept().await {
                use tokio::io::AsyncWriteExt;
                let _ = s.write_all(b"* OK not tls\r\n").await;
            }
        });
        let r = connect_tls_pinned(&endpoint("imap.rebind.invalid", addr.port()), &[addr]).await;
        assert!(matches!(
            r.err(),
            Some(NetError::TlsFailed | NetError::CertificateInvalid)
        ));
    }
}
