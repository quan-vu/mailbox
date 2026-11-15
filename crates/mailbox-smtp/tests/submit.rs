//! Full-envelope submission (MB-151) against a scripted in-memory server (no network, no TLS).
use mailbox_core::secrets::Secret;
use mailbox_smtp::submit::{
    PartialPolicy, RcptStatus, SendReport, Stage, Submission, SubmitError, submit_stream,
};
use mailbox_smtp::{SendOutcome, SmtpError};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

#[derive(Clone)]
struct Script {
    caps: &'static str,
    mail_reply: &'static str,
    /// (address fragment, reply); recipients not listed get `250`.
    rcpt: Vec<(&'static str, &'static str)>,
    data_reply: &'static str,
    /// `None`: the connection dies right after the message was handed over.
    content_reply: Option<&'static str>,
    /// Close the connection when the n-th RCPT arrives.
    drop_at_rcpt: Option<usize>,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            caps: "250-mail.example.com\r\n250-SIZE 52428800\r\n250-8BITMIME\r\n250 AUTH PLAIN LOGIN\r\n",
            mail_reply: "250 ok\r\n",
            rcpt: Vec::new(),
            data_reply: "354 go\r\n",
            content_reply: Some("250 2.0.0 queued\r\n"),
            drop_at_rcpt: None,
        }
    }
}

const NO_8BIT: &str = "250-mail.example.com\r\n250 AUTH PLAIN\r\n";
const UTF8: &str = "250-mail.example.com\r\n250-8BITMIME\r\n250-SMTPUTF8\r\n250 AUTH PLAIN\r\n";
const USER: &str = "user@example.com";

/// Returns (commands seen, message lines received inside DATA).
async fn server(io: DuplexStream, s: Script) -> (Vec<String>, Vec<String>) {
    let (mut commands, mut content) = (Vec::new(), Vec::new());
    let (r, mut w) = tokio::io::split(io);
    let mut r = BufReader::new(r);
    // A client that refuses locally closes the stream without reading anything.
    if w.write_all(b"220 mail.example.com ESMTP\r\n")
        .await
        .is_err()
    {
        return (commands, content);
    }
    let (mut in_data, mut rcpts) = (false, 0);
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).await.unwrap_or(0) == 0 {
            break;
        }
        let cmd = line.trim_end_matches(['\r', '\n']).to_string();
        if in_data {
            if cmd == "." {
                in_data = false;
                match s.content_reply {
                    Some(reply) => w.write_all(reply.as_bytes()).await.unwrap(),
                    None => break,
                }
            } else {
                content.push(cmd);
            }
            continue;
        }
        commands.push(cmd.clone());
        let upper = cmd.to_ascii_uppercase();
        let reply: String = if upper.starts_with("EHLO") {
            s.caps.into()
        } else if upper.starts_with("AUTH PLAIN") {
            "235 ok\r\n".into()
        } else if upper.starts_with("MAIL FROM") {
            s.mail_reply.into()
        } else if upper.starts_with("RCPT TO") {
            rcpts += 1;
            if s.drop_at_rcpt == Some(rcpts) {
                break;
            }
            s.rcpt
                .iter()
                .find(|(who, _)| cmd.contains(who))
                .map(|(_, reply)| reply.to_string())
                .unwrap_or_else(|| "250 ok\r\n".into())
        } else if upper == "DATA" {
            if s.data_reply.starts_with("354") {
                in_data = true;
            }
            s.data_reply.into()
        } else if upper == "RSET" {
            "250 reset\r\n".into()
        } else if upper == "QUIT" {
            w.write_all(b"221 bye\r\n").await.unwrap();
            break;
        } else {
            "500 what\r\n".into()
        };
        w.write_all(reply.as_bytes()).await.unwrap();
    }
    (commands, content)
}

fn submission(recipients: &[&str]) -> Submission {
    Submission {
        from: USER.into(),
        recipients: recipients.iter().map(|r| r.to_string()).collect(),
        data: b"Subject: t\r\n\r\nline\r\n.\r\n..two\r\nend\r\n".to_vec(),
        fallback_7bit: None,
        smtputf8: false,
    }
}

async fn run(
    script: Script,
    s: &Submission,
    policy: PartialPolicy,
) -> (Result<SendReport, SubmitError>, Vec<String>, Vec<String>) {
    let (client, srv) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(server(srv, script));
    let result = submit_stream(client, true, USER, &Secret::new("pw"), s, policy).await;
    let (commands, content) = task.await.unwrap();
    (result, commands, content)
}

fn count(commands: &[String], prefix: &str) -> usize {
    commands
        .iter()
        .filter(|c| c.to_ascii_uppercase().starts_with(prefix))
        .count()
}

#[tokio::test]
async fn multiple_recipients_are_all_named_and_the_message_is_dot_stuffed() {
    let s = submission(&["a@x.org", "b@y.org", "c@z.org"]);
    let (result, commands, content) =
        run(Script::default(), &s, PartialPolicy::AbortIfAnyRejected).await;
    let report = result.unwrap();
    assert_eq!(report.outcome, SendOutcome::Accepted);
    assert_eq!(
        report.recipients,
        vec![
            ("a@x.org".to_string(), RcptStatus::Accepted),
            ("b@y.org".to_string(), RcptStatus::Accepted),
            ("c@z.org".to_string(), RcptStatus::Accepted),
        ]
    );
    let size = s.data.len();
    assert!(
        commands.contains(&format!("MAIL FROM:<{USER}> SIZE={size}")),
        "{commands:?}"
    );
    let rcpt: Vec<&String> = commands
        .iter()
        .filter(|c| c.starts_with("RCPT TO"))
        .collect();
    assert_eq!(
        rcpt,
        [
            "RCPT TO:<a@x.org>",
            "RCPT TO:<b@y.org>",
            "RCPT TO:<c@z.org>"
        ]
    );
    assert_eq!(content, ["Subject: t", "", "line", "..", "...two", "end"]);
}

#[tokio::test]
async fn a_rejected_recipient_stops_the_send_and_is_reported_per_recipient() {
    let script = Script {
        rcpt: vec![("b@y.org", "550 5.1.1 no such user\r\n")],
        ..Script::default()
    };
    let s = submission(&["a@x.org", "b@y.org", "c@z.org"]);
    let (result, commands, content) = run(script, &s, PartialPolicy::AbortIfAnyRejected).await;
    let report = result.unwrap();
    assert_eq!(report.outcome, SendOutcome::Rejected { permanent: true });
    assert_eq!(report.stage, Some(Stage::Rcpt));
    assert_eq!(
        report.recipients,
        vec![
            ("a@x.org".to_string(), RcptStatus::Accepted),
            ("b@y.org".to_string(), RcptStatus::Rejected { code: 550 }),
            ("c@z.org".to_string(), RcptStatus::Accepted),
        ],
        "every recipient was asked, so the user sees all problems at once"
    );
    assert_eq!(report.rejected(), vec!["b@y.org"]);
    assert_eq!(count(&commands, "DATA"), 0, "nothing was sent");
    assert_eq!(count(&commands, "RSET"), 1);
    assert!(content.is_empty());
}

#[tokio::test]
async fn partial_acceptance_can_be_sent_on_request_and_is_still_reported() {
    let script = Script {
        rcpt: vec![("b@y.org", "451 4.7.1 try later\r\n")],
        ..Script::default()
    };
    let s = submission(&["a@x.org", "b@y.org"]);
    let (result, commands, content) = run(script, &s, PartialPolicy::SendToAccepted).await;
    let report = result.unwrap();
    assert_eq!(report.outcome, SendOutcome::Accepted);
    assert_eq!(
        report.recipients[1],
        ("b@y.org".to_string(), RcptStatus::Rejected { code: 451 })
    );
    assert!(!RcptStatus::Rejected { code: 451 }.permanent());
    assert_eq!(count(&commands, "DATA"), 1);
    assert!(!content.is_empty());

    // Nobody accepted: nothing to send, whatever the policy.
    let script = Script {
        rcpt: vec![("@", "550 no\r\n")],
        ..Script::default()
    };
    let (result, commands, _) = run(script, &s, PartialPolicy::SendToAccepted).await;
    assert_eq!(
        result.unwrap().outcome,
        SendOutcome::Rejected { permanent: true }
    );
    assert_eq!(count(&commands, "DATA"), 0);
}

#[tokio::test]
async fn temporary_and_permanent_refusals_are_told_apart_at_every_stage() {
    let s = submission(&["a@x.org"]);
    let cases: Vec<(Script, bool, Stage)> = vec![
        (
            Script {
                mail_reply: "451 busy\r\n",
                ..Script::default()
            },
            false,
            Stage::Mail,
        ),
        (
            Script {
                mail_reply: "550 sender refused\r\n",
                ..Script::default()
            },
            true,
            Stage::Mail,
        ),
        (
            Script {
                rcpt: vec![("a@x.org", "450 greylisted\r\n")],
                ..Script::default()
            },
            false,
            Stage::Rcpt,
        ),
        (
            Script {
                data_reply: "554 no\r\n",
                ..Script::default()
            },
            true,
            Stage::Data,
        ),
        (
            Script {
                content_reply: Some("452 4.3.1 full\r\n"),
                ..Script::default()
            },
            false,
            Stage::Content,
        ),
        (
            Script {
                content_reply: Some("554 5.7.1 spam\r\n"),
                ..Script::default()
            },
            true,
            Stage::Content,
        ),
    ];
    for (script, permanent, stage) in cases {
        let (result, _, _) = run(script, &s, PartialPolicy::AbortIfAnyRejected).await;
        let report = result.unwrap();
        assert_eq!(
            report.outcome,
            SendOutcome::Rejected { permanent },
            "{stage:?}"
        );
        assert_eq!(report.stage, Some(stage));
    }
}

#[tokio::test]
async fn connection_lost_after_data_is_unknown_and_nothing_is_sent_twice() {
    let script = Script {
        content_reply: None,
        ..Script::default()
    };
    let s = submission(&["a@x.org"]);
    let (result, commands, content) = run(script, &s, PartialPolicy::AbortIfAnyRejected).await;
    let report = result.unwrap();
    assert_eq!(report.outcome, SendOutcome::Unknown);
    assert_eq!(report.stage, Some(Stage::Content));
    assert_eq!(
        count(&commands, "MAIL FROM"),
        1,
        "no automatic second attempt"
    );
    assert_eq!(count(&commands, "DATA"), 1);
    assert_eq!(content.iter().filter(|l| *l == "Subject: t").count(), 1);
}

#[tokio::test]
async fn connection_lost_before_data_is_a_definite_failure() {
    let script = Script {
        drop_at_rcpt: Some(2),
        ..Script::default()
    };
    let s = submission(&["a@x.org", "b@y.org"]);
    let (result, commands, content) = run(script, &s, PartialPolicy::AbortIfAnyRejected).await;
    assert_eq!(
        result.unwrap_err(),
        SubmitError::Session(SmtpError::NotSmtp)
    );
    assert_eq!(count(&commands, "DATA"), 0);
    assert!(content.is_empty());
}

#[tokio::test]
async fn messages_over_the_advertised_size_are_refused_before_mail_from() {
    let script = Script {
        caps: "250-mail.example.com\r\n250-SIZE 20\r\n250 AUTH PLAIN\r\n",
        ..Script::default()
    };
    let s = submission(&["a@x.org"]);
    let (result, commands, _) = run(script, &s, PartialPolicy::AbortIfAnyRejected).await;
    assert_eq!(
        result.unwrap_err(),
        SubmitError::TooLarge {
            limit: 20,
            actual: s.data.len() as u64
        }
    );
    assert_eq!(count(&commands, "MAIL FROM"), 0);

    // A server that only objects when it sees the SIZE parameter.
    let script = Script {
        mail_reply: "552 5.3.4 too big\r\n",
        ..Script::default()
    };
    let (result, commands, _) = run(script, &s, PartialPolicy::AbortIfAnyRejected).await;
    let report = result.unwrap();
    assert_eq!(
        (report.outcome, report.code),
        (SendOutcome::Rejected { permanent: true }, Some(552))
    );
    assert_eq!(count(&commands, "RCPT"), 0);
}

#[tokio::test]
async fn eight_bit_content_needs_8bitmime_or_falls_back_to_the_encoded_form() {
    let mut s = submission(&["a@x.org"]);
    s.data = "Subject: t\r\n\r\nXin chào\r\n".as_bytes().to_vec();
    s.fallback_7bit = Some(b"Subject: t\r\n\r\nXin ch=C3=A0o\r\n".to_vec());

    let (result, commands, content) =
        run(Script::default(), &s, PartialPolicy::AbortIfAnyRejected).await;
    assert!(result.unwrap().used_8bit);
    assert!(
        commands
            .iter()
            .any(|c| c.starts_with("MAIL FROM") && c.ends_with(" BODY=8BITMIME"))
    );
    assert!(content.contains(&"Xin chào".to_string()));

    let script = Script {
        caps: NO_8BIT,
        ..Script::default()
    };
    let (result, commands, content) =
        run(script.clone(), &s, PartialPolicy::AbortIfAnyRejected).await;
    let report = result.unwrap();
    assert!(!report.used_8bit && report.outcome == SendOutcome::Accepted);
    assert_eq!(
        commands
            .iter()
            .find(|c| c.starts_with("MAIL FROM"))
            .unwrap(),
        &format!("MAIL FROM:<{USER}>")
    );
    assert!(content.contains(&"Xin ch=C3=A0o".to_string()) && content.iter().all(|l| l.is_ascii()));

    s.fallback_7bit = None;
    let (result, commands, _) = run(script, &s, PartialPolicy::AbortIfAnyRejected).await;
    assert_eq!(result.unwrap_err(), SubmitError::EightBitUnavailable);
    assert_eq!(count(&commands, "MAIL FROM"), 0);
}

#[tokio::test]
async fn non_ascii_addresses_are_refused_without_smtputf8() {
    let mut s = submission(&["người@ví-dụ.vn"]);
    s.smtputf8 = true;
    let (result, commands, _) = run(Script::default(), &s, PartialPolicy::AbortIfAnyRejected).await;
    assert_eq!(result.unwrap_err(), SubmitError::SmtpUtf8Unavailable);
    assert_eq!(count(&commands, "MAIL FROM"), 0);

    let script = Script {
        caps: UTF8,
        ..Script::default()
    };
    let (result, commands, _) = run(script, &s, PartialPolicy::AbortIfAnyRejected).await;
    assert_eq!(result.unwrap().outcome, SendOutcome::Accepted);
    assert!(
        commands
            .iter()
            .any(|c| c.starts_with("MAIL FROM") && c.ends_with(" SMTPUTF8"))
    );
    assert!(commands.contains(&"RCPT TO:<người@ví-dụ.vn>".to_string()));

    // A non-ASCII address that was not declared is refused locally, not sent and hoped for.
    s.smtputf8 = false;
    let (result, commands, _) = run(
        Script {
            caps: UTF8,
            ..Script::default()
        },
        &s,
        PartialPolicy::AbortIfAnyRejected,
    )
    .await;
    assert_eq!(result.unwrap_err(), SubmitError::InvalidEnvelope);
    assert!(commands.is_empty(), "refused before any I/O");
}

#[tokio::test]
async fn envelope_addresses_cannot_inject_commands() {
    for bad in [
        "a@x.org>\r\nRCPT TO:<victim@x.org",
        "a b@x.org",
        "<a@x.org>",
        "no-at",
        "",
    ] {
        let s = submission(&["ok@x.org", bad]);
        let (result, commands, _) = run(Script::default(), &s, PartialPolicy::SendToAccepted).await;
        assert_eq!(result.unwrap_err(), SubmitError::InvalidEnvelope, "{bad:?}");
        assert!(commands.is_empty(), "refused before any I/O");
    }
    let mut s = submission(&[]);
    let (result, _, _) = run(Script::default(), &s, PartialPolicy::SendToAccepted).await;
    assert_eq!(
        result.unwrap_err(),
        SubmitError::InvalidEnvelope,
        "no recipients"
    );
    s = submission(&["a@x.org"]);
    s.from = "me@x.org\r\nDATA".into();
    let (result, _, _) = run(Script::default(), &s, PartialPolicy::SendToAccepted).await;
    assert_eq!(result.unwrap_err(), SubmitError::InvalidEnvelope);
}

#[tokio::test]
async fn bare_line_feeds_in_the_message_are_refused() {
    let mut s = submission(&["a@x.org"]);
    s.data = b"Subject: t\r\n\r\nline\n.\nQUIT\r\n".to_vec();
    let (result, commands, _) = run(Script::default(), &s, PartialPolicy::AbortIfAnyRejected).await;
    assert_eq!(result.unwrap_err(), SubmitError::InvalidMessage);
    assert!(commands.is_empty());
}
