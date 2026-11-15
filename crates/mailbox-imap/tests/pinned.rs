//! MB-304: the wizard check can connect to addresses checked by the SSRF guard, without a
//! second DNS lookup. Loopback only; no outside host is contacted.
use mailbox_core::secrets::Secret;
use mailbox_core::{MailServerEndpoint, TlsMode};
use mailbox_imap::{ConnectError, check_login_pinned};

fn endpoint(host: &str, port: u16) -> MailServerEndpoint {
    MailServerEndpoint {
        hostname: host.into(),
        port,
        tls_mode: TlsMode::ImplicitTls,
    }
}

#[tokio::test]
async fn pinned_check_connects_to_the_given_address_and_still_requires_tls() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let (mut s, _) = listener.accept().await.unwrap();
        // A plain-text greeting where a TLS handshake is expected: the check must stop here.
        let _ = s.write_all(b"* OK plain\r\n").await;
        true
    });
    let secret = Secret::new("not-a-real-password".to_string());
    let r = check_login_pinned(
        &endpoint("imap.rebind.invalid", addr.port()),
        &[addr],
        "user",
        &secret,
    )
    .await;
    assert!(seen.await.unwrap(), "the pinned address was used");
    assert!(matches!(
        r,
        Err(ConnectError::TlsFailed | ConnectError::CertificateInvalid)
    ));
}

#[tokio::test]
async fn pinned_check_refuses_mismatched_or_missing_addresses() {
    let secret = Secret::new("x".to_string());
    let other = "127.0.0.1:1".parse().unwrap();
    assert_eq!(
        check_login_pinned(&endpoint("imap.example.com", 993), &[], "u", &secret).await,
        Err(ConnectError::InvalidEndpoint)
    );
    assert_eq!(
        check_login_pinned(&endpoint("imap.example.com", 993), &[other], "u", &secret).await,
        Err(ConnectError::InvalidEndpoint)
    );
}
