//! IMAP dialogue tests against a scripted in-memory server (no network, no TLS).
use mailbox_core::secrets::Secret;
use mailbox_imap::{ConnectError, check_login_stream, probe_stream};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};

const CAPS: &str = "* CAPABILITY IMAP4rev1 IDLE SPECIAL-USE AUTH=PLAIN\r\n";

/// Minimal scripted server. Records everything the client sent.
async fn server(
    io: DuplexStream,
    greeting: &str,
    caps: &str,
    accept_password: Option<&str>,
) -> Vec<u8> {
    let mut seen = Vec::new();
    let (r, mut w) = tokio::io::split(io);
    let mut r = BufReader::new(r);
    w.write_all(greeting.as_bytes()).await.unwrap();
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).await.unwrap_or(0) == 0 {
            break;
        }
        seen.extend_from_slice(line.as_bytes());
        let tag = line.split(' ').next().unwrap_or("").to_string();
        if line.contains("CAPABILITY") {
            w.write_all(format!("{caps}{tag} OK done\r\n").as_bytes())
                .await
                .unwrap();
        } else if line.contains("LOGIN") {
            // "<tag> LOGIN {n}" then user literal, " {m}", password literal.
            let n: usize = line
                .trim_end()
                .rsplit('{')
                .next()
                .unwrap()
                .trim_end_matches('}')
                .parse()
                .unwrap();
            w.write_all(b"+ go\r\n").await.unwrap();
            let mut user = vec![0u8; n];
            r.read_exact(&mut user).await.unwrap();
            let mut rest = String::new();
            r.read_line(&mut rest).await.unwrap();
            let m: usize = rest
                .trim()
                .trim_start_matches('{')
                .trim_end_matches('}')
                .parse()
                .unwrap();
            w.write_all(b"+ go\r\n").await.unwrap();
            let mut pass = vec![0u8; m];
            r.read_exact(&mut pass).await.unwrap();
            let mut crlf = String::new();
            r.read_line(&mut crlf).await.unwrap();
            seen.extend_from_slice(&user);
            let ok = accept_password.is_some_and(|p| p.as_bytes() == pass);
            let reply = if ok {
                format!("{tag} OK logged in\r\n")
            } else {
                format!("{tag} NO [AUTHENTICATIONFAILED] invalid\r\n")
            };
            w.write_all(reply.as_bytes()).await.unwrap();
        } else if line.contains("EXAMINE INBOX") {
            w.write_all(
                format!(
                    "* FLAGS (\\Seen)\r\n* 42 EXISTS\r\n* 0 RECENT\r\n{tag} OK [READ-ONLY] done\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        } else if line.contains("LOGOUT") {
            w.write_all(format!("* BYE\r\n{tag} OK bye\r\n").as_bytes())
                .await
                .unwrap();
            break;
        }
    }
    seen
}

#[tokio::test]
async fn probe_reads_capabilities_and_sends_no_credentials() {
    let (client, srv) = tokio::io::duplex(4096);
    let task = tokio::spawn(server(srv, "* OK ready\r\n", CAPS, None));
    let info = probe_stream(client).await.unwrap();
    assert!(info.has("imap4rev1") && info.has("IDLE"));
    let seen = String::from_utf8(task.await.unwrap()).unwrap();
    assert!(!seen.contains("LOGIN"));
}

#[tokio::test]
async fn login_succeeds_with_exact_password_bytes() {
    // Quotes, backslash, braces, spaces and non-ASCII must arrive unchanged.
    let password = r#"p"a\ss {3} wörd"#;
    let (client, srv) = tokio::io::duplex(4096);
    let task = tokio::spawn(server(srv, "* OK ready\r\n", CAPS, Some(password)));
    let info = check_login_stream(client, "user@example.com", &Secret::new(password))
        .await
        .unwrap();
    assert!(info.has("SPECIAL-USE"));
    assert_eq!(info.inbox_messages, Some(42));
    let seen = String::from_utf8_lossy(&task.await.unwrap()).to_string();
    assert!(seen.contains("user@example.com"));
    // Read-only: the mailbox is examined, never selected for writing.
    assert!(seen.contains("EXAMINE INBOX") && !seen.contains("SELECT"));
}

#[tokio::test]
async fn wrong_password_is_auth_failed_without_server_text() {
    let (client, srv) = tokio::io::duplex(4096);
    tokio::spawn(server(srv, "* OK ready\r\n", CAPS, Some("right")));
    let err = check_login_stream(client, "user@example.com", &Secret::new("wrong-secret"))
        .await
        .unwrap_err();
    assert_eq!(err, ConnectError::AuthFailed);
    assert!(!format!("{err:?}").contains("wrong-secret"));
}

#[tokio::test]
async fn login_disabled_server_never_receives_credentials() {
    let (client, srv) = tokio::io::duplex(4096);
    let task = tokio::spawn(server(
        srv,
        "* OK ready\r\n",
        "* CAPABILITY IMAP4rev1 LOGINDISABLED AUTH=XOAUTH2\r\n",
        Some("x"),
    ));
    let err = check_login_stream(client, "user@example.com", &Secret::new("x"))
        .await
        .unwrap_err();
    assert_eq!(err, ConnectError::LoginDisabled);
    assert!(!String::from_utf8_lossy(&task.await.unwrap()).contains("LOGIN "));
}

#[tokio::test]
async fn non_imap_peers_are_rejected() {
    for greeting in [
        "HTTP/1.1 400 Bad Request\r\n",
        "220 smtp.example.com ESMTP\r\n",
        "* BYE overloaded\r\n",
    ] {
        let (client, srv) = tokio::io::duplex(4096);
        tokio::spawn(server(srv, greeting, CAPS, None));
        assert_eq!(
            probe_stream(client).await.unwrap_err(),
            ConnectError::NotImap,
            "{greeting:?}"
        );
    }
    // Speaks IMAP-like lines but advertises no IMAP4 revision.
    let (client, srv) = tokio::io::duplex(4096);
    tokio::spawn(server(srv, "* OK ready\r\n", "* CAPABILITY POP3\r\n", None));
    assert_eq!(
        probe_stream(client).await.unwrap_err(),
        ConnectError::NotImap
    );
}

#[tokio::test]
async fn oversized_or_unterminated_lines_are_rejected() {
    let (client, mut srv) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let _ = srv.write_all(&vec![b'x'; 20_000]).await;
        let _ = srv.write_all(b"\r\n").await;
    });
    assert_eq!(
        probe_stream(client).await.unwrap_err(),
        ConnectError::NotImap
    );
}

#[tokio::test]
async fn line_breaks_in_credentials_are_refused_before_any_io() {
    let (client, _srv) = tokio::io::duplex(64);
    let err = check_login_stream(client, "user\r\na1 LOGOUT", &Secret::new("x"))
        .await
        .unwrap_err();
    assert_eq!(err, ConnectError::AuthFailed);
    let (client, _srv) = tokio::io::duplex(64);
    assert_eq!(
        check_login_stream(client, "user", &Secret::new("a\nb"))
            .await
            .unwrap_err(),
        ConnectError::AuthFailed
    );
}
