//! Real-network SMTP checks.
//!
//! Credential-free probes:
//!   MAILBOX_TEST_SMTP_HOST=<host> cargo test -p mailbox-smtp --test network probe -- --ignored --nocapture
//!
//! Owner tests (need the git-ignored `.env.smtp`; the password is read by the test process and
//! never printed):
//!   (values are read from `.env.smtp` at the repo root, or from the environment)
//!   cargo test -p mailbox-smtp --test network owner_login -- --ignored --nocapture
//!   MAILBOX_SMTP_SEND_TO=<recipient> cargo test -p mailbox-smtp --test network owner_send -- --ignored --nocapture
use mailbox_core::secrets::Secret;
use mailbox_core::{MailServerEndpoint, TlsMode};
use mailbox_smtp::{SendOutcome, SmtpError, TextMessage, check, identity_matches_login, send};

fn endpoint(host: &str, port: u16) -> MailServerEndpoint {
    let tls_mode = if port == 465 {
        TlsMode::ImplicitTls
    } else {
        TlsMode::StartTls
    };
    MailServerEndpoint {
        hostname: host.into(),
        port,
        tls_mode,
    }
}

/// Value from the process environment, else from the git-ignored `.env.smtp` at the repo root
/// (plain `KEY=VALUE` lines; optional surrounding quotes). Values are never printed.
fn env(key: &str) -> String {
    if let Ok(v) = std::env::var(key) {
        return v;
    }
    let path = std::env::var("MAILBOX_SMTP_ENV_FILE").unwrap_or_else(|_| "../../.env.smtp".into());
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{key} is not set and {path} is not readable"));
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
        .unwrap_or_else(|| panic!("{key} is missing from {path}"))
}

fn owner() -> (MailServerEndpoint, String, Secret, String) {
    let port: u16 = env("SMTP_PORT_1")
        .trim()
        .parse()
        .expect("SMTP_PORT_1 must be a port number");
    (
        endpoint(env("SMTP_HOST_1").trim(), port),
        env("SMTP_USER_1").trim().to_string(),
        Secret::new(env("SMTP_PASS_1")),
        env("SMTP_FROM_1"),
    )
}

/// `Name <addr>` or bare `addr`.
fn bare_address(from: &str) -> String {
    match (from.find('<'), from.rfind('>')) {
        (Some(a), Some(b)) if b > a => from[a + 1..b].trim().to_string(),
        _ => from.trim().to_string(),
    }
}

#[tokio::test]
async fn invalid_endpoint_fails_before_any_network_access() {
    assert_eq!(
        check(&endpoint("bad host", 465), None).await,
        Err(SmtpError::InvalidEndpoint)
    );
}

#[tokio::test]
#[ignore = "needs internet"]
async fn probe_invalid_certificate_aborts() {
    for host in [
        "expired.badssl.com",
        "wrong.host.badssl.com",
        "self-signed.badssl.com",
    ] {
        let e = MailServerEndpoint {
            hostname: host.into(),
            port: 443,
            tls_mode: TlsMode::ImplicitTls,
        };
        assert_eq!(
            check(&e, None).await.err(),
            Some(SmtpError::CertificateInvalid),
            "{host}"
        );
    }
}

#[tokio::test]
#[ignore = "needs internet and MAILBOX_TEST_SMTP_HOST"]
async fn probe_real_provider_on_465_and_587() {
    let host = env("MAILBOX_TEST_SMTP_HOST");
    for port in [465u16, 587] {
        let info = check(&endpoint(&host, port), None)
            .await
            .unwrap_or_else(|e| panic!("port {port}: {e:?}"));
        assert!(info.has("AUTH"), "port {port} offers AUTH after TLS");
        println!("port {port}: {}", info.capabilities.join(" | "));
    }
}

#[tokio::test]
#[ignore = "owner only: needs .env.smtp"]
async fn owner_login_is_accepted() {
    let (endpoint, user, pass, from) = owner();
    let info = check(&endpoint, Some((&user, &pass)))
        .await
        .expect("login accepted over verified TLS");
    println!(
        "login ok on port {}; capabilities: {}",
        endpoint.port,
        info.capabilities.join(" | ")
    );
    println!("max message size: {:?}", info.max_size());
    println!(
        "From matches login: {}",
        identity_matches_login(&user, &bare_address(&from))
    );
}

#[tokio::test]
#[ignore = "owner only: needs .env.smtp and MAILBOX_SMTP_SEND_TO; sends one real email"]
async fn owner_send_one_test_message() {
    let (endpoint, user, pass, from) = owner();
    // `self` sends to the login mailbox itself (useful to seed a mailbox for IMAP sync tests).
    let to = match env("MAILBOX_SMTP_SEND_TO").trim() {
        "self" => user.clone(),
        other => other.to_string(),
    };
    let message = TextMessage {
        from: bare_address(&from),
        to: to.trim().to_string(),
        subject: "MailBox: thư thử SMTP".into(),
        body: "Đây là thư thử gửi từ bộ kiểm tra SMTP của MailBox (MB-112).\nNếu bạn nhận được thư này, đăng nhập và gửi qua TLS đã hoạt động.\n".into(),
    };
    let outcome = send(&endpoint, &user, &pass, &message)
        .await
        .expect("SMTP session");
    println!("outcome: {outcome:?}");
    assert_eq!(outcome, SendOutcome::Accepted);
}
