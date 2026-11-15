//! Real-network checks. They send no credentials. Run with:
//! `MAILBOX_TEST_IMAP_HOST=<host> cargo test -p mailbox-imap --test network -- --ignored`
use mailbox_core::{MailServerEndpoint, TlsMode};
use mailbox_imap::{ConnectError, probe};

fn endpoint(host: &str, port: u16) -> MailServerEndpoint {
    MailServerEndpoint {
        hostname: host.into(),
        port,
        tls_mode: TlsMode::ImplicitTls,
    }
}

#[tokio::test]
async fn invalid_endpoints_fail_before_any_network_access() {
    assert_eq!(
        probe(&endpoint("bad host/evil", 993)).await,
        Err(ConnectError::InvalidEndpoint)
    );
    assert_eq!(
        probe(&endpoint("imap.example.com", 0)).await,
        Err(ConnectError::InvalidEndpoint)
    );
    let starttls = MailServerEndpoint {
        hostname: "imap.example.com".into(),
        port: 143,
        tls_mode: TlsMode::StartTls,
    };
    assert_eq!(
        probe(&starttls).await,
        Err(ConnectError::UnsupportedTlsMode)
    );
}

/// badssl.com is a public test service with deliberately broken certificates.
#[tokio::test]
#[ignore = "needs internet"]
async fn invalid_certificates_abort_the_connection() {
    for host in [
        "expired.badssl.com",
        "wrong.host.badssl.com",
        "self-signed.badssl.com",
        "untrusted-root.badssl.com",
    ] {
        assert_eq!(
            probe(&endpoint(host, 443)).await,
            Err(ConnectError::CertificateInvalid),
            "{host}"
        );
    }
}

/// An HTTPS server has a valid certificate but never sends an IMAP greeting: the check gives up
/// after the step timeout instead of hanging.
#[tokio::test]
#[ignore = "needs internet"]
async fn valid_tls_peer_that_never_greets_times_out() {
    assert_eq!(
        probe(&endpoint("badssl.com", 443)).await,
        Err(ConnectError::Timeout)
    );
}

#[tokio::test]
#[ignore = "needs internet"]
async fn unreachable_host_is_reported() {
    assert_eq!(
        probe(&endpoint("no-such-host.invalid", 993)).await,
        Err(ConnectError::Unreachable)
    );
}

#[tokio::test]
#[ignore = "needs internet and MAILBOX_TEST_IMAP_HOST"]
async fn real_provider_presents_a_valid_certificate_and_speaks_imap() {
    let host = std::env::var("MAILBOX_TEST_IMAP_HOST").expect("set MAILBOX_TEST_IMAP_HOST");
    let info = probe(&endpoint(&host, 993))
        .await
        .expect("verified TLS + IMAP greeting");
    assert!(info.has("IMAP4rev1"));
    println!("capabilities: {}", info.capabilities.join(" "));
}
