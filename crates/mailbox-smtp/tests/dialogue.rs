//! SMTP dialogue tests against a scripted in-memory server (no network, no TLS).
use mailbox_core::secrets::Secret;
use mailbox_smtp::{
    SendOutcome, SmtpError, TextMessage, check_stream, negotiate_starttls, send_stream,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

#[derive(Clone, Copy)]
struct Script {
    greeting: &'static str,
    caps: &'static str,
    /// base64 of "\0user@example.com\0pässword" accepted by AUTH PLAIN.
    accept_plain: bool,
    data_reply: Option<&'static str>,
    rcpt_reply: &'static str,
    starttls_reply: &'static str,
}

const OK: Script = Script {
    greeting: "220 mail.example.com ESMTP\r\n",
    caps: "250-mail.example.com\r\n250-SIZE 52428800\r\n250-8BITMIME\r\n250 AUTH PLAIN LOGIN\r\n",
    accept_plain: true,
    data_reply: Some("250 2.0.0 queued\r\n"),
    rcpt_reply: "250 ok\r\n",
    starttls_reply: "220 go ahead\r\n",
};

const USER: &str = "user@example.com";
const PASS: &str = "pässword";
// printf '\0user@example.com\0pässword' | base64
const PLAIN_TOKEN: &str = "AHVzZXJAZXhhbXBsZS5jb20AcMOkc3N3b3Jk";

/// Returns every line the client sent.
async fn server(io: DuplexStream, s: Script) -> Vec<String> {
    let mut seen = Vec::new();
    let (r, mut w) = tokio::io::split(io);
    let mut r = BufReader::new(r);
    if !s.greeting.is_empty() {
        w.write_all(s.greeting.as_bytes()).await.unwrap();
    }
    let mut in_data = false;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).await.unwrap_or(0) == 0 {
            break;
        }
        let cmd = line.trim_end().to_string();
        seen.push(cmd.clone());
        if in_data {
            if cmd == "." {
                in_data = false;
                match s.data_reply {
                    Some(reply) => w.write_all(reply.as_bytes()).await.unwrap(),
                    None => break, // connection dies after the message was handed over
                }
            }
            continue;
        }
        let upper = cmd.to_ascii_uppercase();
        let reply: String = if upper.starts_with("EHLO") {
            s.caps.into()
        } else if upper.starts_with("STARTTLS") {
            s.starttls_reply.into()
        } else if upper.starts_with("AUTH PLAIN") {
            if s.accept_plain && cmd.ends_with(PLAIN_TOKEN) {
                "235 ok\r\n".into()
            } else {
                "535 5.7.8 bad credentials\r\n".into()
            }
        } else if upper.starts_with("MAIL FROM") {
            "250 ok\r\n".into()
        } else if upper.starts_with("RCPT TO") {
            s.rcpt_reply.into()
        } else if upper == "DATA" {
            in_data = true;
            "354 end with .\r\n".into()
        } else if upper == "QUIT" {
            w.write_all(b"221 bye\r\n").await.unwrap();
            break;
        } else {
            "500 what\r\n".into()
        };
        w.write_all(reply.as_bytes()).await.unwrap();
    }
    seen
}

fn message() -> TextMessage {
    TextMessage {
        from: USER.into(),
        to: "dest@example.org".into(),
        subject: "Thử".into(),
        body: ".dòng bắt đầu bằng chấm\n.\nhết".into(),
    }
}

#[tokio::test]
async fn check_reads_capabilities_authenticates_and_never_sends_mail() {
    let (client, srv) = tokio::io::duplex(8192);
    let task = tokio::spawn(server(srv, OK));
    let info = check_stream(client, true, Some((USER, &Secret::new(PASS))))
        .await
        .unwrap();
    assert_eq!(info.max_size(), Some(52_428_800));
    assert_eq!(info.auth_mechanisms(), vec!["PLAIN", "LOGIN"]);
    let seen = task.await.unwrap();
    for forbidden in ["MAIL FROM", "RCPT TO", "DATA"] {
        assert!(
            !seen
                .iter()
                .any(|l| l.to_ascii_uppercase().starts_with(forbidden)),
            "{forbidden} was sent"
        );
    }
    assert!(
        seen.iter()
            .any(|l| l == &format!("AUTH PLAIN {PLAIN_TOKEN}")),
        "exact credential bytes"
    );
}

#[tokio::test]
async fn probe_without_credentials_sends_no_auth() {
    let (client, srv) = tokio::io::duplex(8192);
    let task = tokio::spawn(server(srv, OK));
    check_stream(client, true, None).await.unwrap();
    assert!(
        !task
            .await
            .unwrap()
            .iter()
            .any(|l| l.to_ascii_uppercase().starts_with("AUTH"))
    );
}

#[tokio::test]
async fn wrong_password_is_auth_failed_without_server_text() {
    let (client, srv) = tokio::io::duplex(8192);
    tokio::spawn(server(srv, OK));
    let err = check_stream(client, true, Some((USER, &Secret::new("wrong-secret"))))
        .await
        .unwrap_err();
    assert_eq!(err, SmtpError::AuthFailed);
    assert!(!format!("{err:?}").contains("wrong-secret"));
}

#[tokio::test]
async fn server_without_password_auth_is_reported() {
    let (client, srv) = tokio::io::duplex(8192);
    let task = tokio::spawn(server(
        srv,
        Script {
            caps: "250-x\r\n250 AUTH XOAUTH2\r\n",
            ..OK
        },
    ));
    assert_eq!(
        check_stream(client, true, Some((USER, &Secret::new(PASS)))).await,
        Err(SmtpError::AuthUnsupported)
    );
    assert!(
        !task.await.unwrap().iter().any(|l| l.starts_with("AUTH")),
        "no credentials sent"
    );
}

#[tokio::test]
async fn non_smtp_peers_and_injected_credentials_are_rejected() {
    for greeting in [
        "* OK IMAP ready\r\n",
        "HTTP/1.1 400 Bad\r\n",
        "554 go away\r\n",
    ] {
        let (client, srv) = tokio::io::duplex(8192);
        tokio::spawn(server(srv, Script { greeting, ..OK }));
        assert_eq!(
            check_stream(client, true, None).await,
            Err(SmtpError::NotSmtp),
            "{greeting:?}"
        );
    }
    let (client, _srv) = tokio::io::duplex(64);
    assert_eq!(
        check_stream(
            client,
            true,
            Some(("u\r\nMAIL FROM:<x@y.z>", &Secret::new("p")))
        )
        .await,
        Err(SmtpError::AuthFailed)
    );
}

#[tokio::test]
async fn starttls_is_required_and_never_downgraded() {
    // Server without STARTTLS: stop, do not continue in plaintext.
    let (client, srv) = tokio::io::duplex(8192);
    let task = tokio::spawn(server(
        srv,
        Script {
            caps: "250-x\r\n250 AUTH PLAIN\r\n",
            ..OK
        },
    ));
    assert!(matches!(
        negotiate_starttls(client).await,
        Err(SmtpError::StartTlsUnavailable)
    ));
    assert!(!task.await.unwrap().iter().any(|l| l.starts_with("AUTH")));

    // Server refuses the STARTTLS command.
    let (client, srv) = tokio::io::duplex(8192);
    tokio::spawn(server(
        srv,
        Script {
            caps: "250-x\r\n250 STARTTLS\r\n",
            starttls_reply: "454 not now\r\n",
            ..OK
        },
    ));
    assert!(matches!(
        negotiate_starttls(client).await,
        Err(SmtpError::StartTlsUnavailable)
    ));

    // Happy path hands back the stream for the TLS handshake.
    let (client, srv) = tokio::io::duplex(8192);
    tokio::spawn(server(
        srv,
        Script {
            caps: "250-x\r\n250 STARTTLS\r\n",
            ..OK
        },
    ));
    assert!(negotiate_starttls(client).await.is_ok());
}

#[tokio::test]
async fn bytes_smuggled_after_the_starttls_reply_are_rejected() {
    let (client, srv) = tokio::io::duplex(8192);
    tokio::spawn(server(
        srv,
        Script {
            caps: "250-x\r\n250 STARTTLS\r\n",
            starttls_reply: "220 go\r\n250 AUTH PLAIN\r\n",
            ..OK
        },
    ));
    assert!(matches!(
        negotiate_starttls(client).await,
        Err(SmtpError::NotSmtp)
    ));
}

#[tokio::test]
async fn send_is_accepted_and_dot_lines_are_stuffed() {
    let (client, srv) = tokio::io::duplex(16384);
    let task = tokio::spawn(server(srv, OK));
    let outcome = send_stream(client, true, USER, &Secret::new(PASS), &message())
        .await
        .unwrap();
    assert_eq!(outcome, SendOutcome::Accepted);
    let seen = task.await.unwrap();
    assert!(
        seen.contains(&format!("MAIL FROM:<{USER}>"))
            && seen.contains(&"RCPT TO:<dest@example.org>".to_string())
    );
    assert_eq!(
        seen.iter().filter(|l| l.as_str() == ".").count(),
        1,
        "only the terminator is a bare dot"
    );
    assert!(
        seen.iter().any(|l| l.starts_with("Message-ID: <"))
            && seen.iter().any(|l| l.starts_with("Date: "))
    );
}

#[tokio::test]
async fn rejected_recipient_sends_nothing() {
    let (client, srv) = tokio::io::duplex(16384);
    let task = tokio::spawn(server(
        srv,
        Script {
            rcpt_reply: "550 5.1.1 no such user\r\n",
            ..OK
        },
    ));
    let outcome = send_stream(client, true, USER, &Secret::new(PASS), &message())
        .await
        .unwrap();
    assert_eq!(outcome, SendOutcome::Rejected { permanent: true });
    assert!(!task.await.unwrap().iter().any(|l| l == "DATA"));
}

#[tokio::test]
async fn lost_connection_after_data_is_unknown_not_retried() {
    let (client, srv) = tokio::io::duplex(16384);
    let task = tokio::spawn(server(
        srv,
        Script {
            data_reply: None,
            ..OK
        },
    ));
    let outcome = send_stream(client, true, USER, &Secret::new(PASS), &message())
        .await
        .unwrap();
    assert_eq!(outcome, SendOutcome::Unknown);
    let seen = task.await.unwrap();
    assert_eq!(
        seen.iter()
            .filter(|l| l.to_ascii_uppercase().starts_with("MAIL FROM"))
            .count(),
        1,
        "exactly one attempt"
    );
}

#[tokio::test]
async fn failed_auth_never_reaches_mail_commands() {
    let (client, srv) = tokio::io::duplex(16384);
    let task = tokio::spawn(server(
        srv,
        Script {
            accept_plain: false,
            ..OK
        },
    ));
    assert_eq!(
        send_stream(client, true, USER, &Secret::new(PASS), &message()).await,
        Err(SmtpError::AuthFailed)
    );
    assert!(
        !task
            .await
            .unwrap()
            .iter()
            .any(|l| l.to_ascii_uppercase().starts_with("MAIL FROM"))
    );
}
