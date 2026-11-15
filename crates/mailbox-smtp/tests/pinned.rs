//! MB-304: SMTP check over addresses checked by the SSRF guard, without a second DNS lookup.
//! Loopback only; no outside host is contacted.
use mailbox_core::secrets::Secret;
use mailbox_core::{MailServerEndpoint, TlsMode};
use mailbox_smtp::{SmtpError, check_pinned};

fn endpoint(host: &str, port: u16, tls_mode: TlsMode) -> MailServerEndpoint {
    MailServerEndpoint {
        hostname: host.into(),
        port,
        tls_mode,
    }
}

#[tokio::test]
async fn pinned_starttls_check_talks_to_the_given_address_and_stops_without_starttls() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        s.write_all(b"220 test ESMTP\r\n").await.unwrap();
        let mut buf = [0u8; 256];
        let n = s.read(&mut buf).await.unwrap();
        let ehlo = String::from_utf8_lossy(&buf[..n]).to_string();
        s.write_all(b"250-test\r\n250 SIZE 1000\r\n").await.unwrap();
        let mut rest = Vec::new();
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            s.read_to_end(&mut rest),
        )
        .await;
        (ehlo, String::from_utf8_lossy(&rest).to_string())
    });
    let secret = Secret::new("not-a-real-password".to_string());
    let r = check_pinned(
        &endpoint("smtp.rebind.invalid", addr.port(), TlsMode::StartTls),
        &[addr],
        Some(("user", &secret)),
    )
    .await;
    assert_eq!(r.err(), Some(SmtpError::StartTlsUnavailable));
    let (ehlo, rest) = server.await.unwrap();
    assert!(ehlo.starts_with("EHLO"));
    assert!(!rest.contains("AUTH") && !rest.contains("not-a-real-password"));
}

#[tokio::test]
async fn pinned_check_refuses_mismatched_or_missing_addresses() {
    let other = "127.0.0.1:1".parse().unwrap();
    let ep = endpoint("smtp.example.com", 465, TlsMode::ImplicitTls);
    assert_eq!(
        check_pinned(&ep, &[], None).await.err(),
        Some(SmtpError::InvalidEndpoint)
    );
    assert_eq!(
        check_pinned(&ep, &[other], None).await.err(),
        Some(SmtpError::InvalidEndpoint)
    );
}
