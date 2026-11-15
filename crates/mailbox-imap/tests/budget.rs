//! Total byte budget per IMAP command (AUDIT S-08).
//!
//! Every single line, literal and response count was already capped, but their product was
//! not: a hostile server could make one command buffer gigabytes. These tests flood one
//! command with responses that each pass the per-item limits.
use mailbox_core::actions::RemoteFailure;
use mailbox_core::secrets::Secret;
use mailbox_imap::{ConnectError, Session};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};

const MIB: usize = 1024 * 1024;

/// One LIST response carrying `literals` literals of 60 KiB each (each under the 64 KiB cap).
fn fat_list(literals: usize) -> Vec<u8> {
    let mut out = b"* LIST (\\HasNoChildren) \".\"".to_vec();
    for _ in 0..literals {
        out.extend_from_slice(format!(" {{{}}}\r\n", 60 * 1024).as_bytes());
        out.resize(out.len() + 60 * 1024, b'x');
    }
    out.extend_from_slice(b"\r\n");
    out
}

/// One untagged line of about 6 KiB (under the 8 KiB line cap).
fn fat_line() -> Vec<u8> {
    let mut out = b"* OK ".to_vec();
    out.resize(6 * 1024, b'x');
    out.extend_from_slice(b"\r\n");
    out
}

/// Answers LIST with `list_bytes` of LIST data and `UID FETCH` with `flag_bytes` of untagged
/// lines, then the tagged OK. Stops quietly when the client hangs up.
async fn server(io: DuplexStream, list_bytes: usize, flag_bytes: usize) {
    let (r, mut w) = tokio::io::split(io);
    let mut r = BufReader::new(r);
    w.write_all(b"* OK ready\r\n").await.unwrap();
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).await.unwrap_or(0) == 0 {
            break;
        }
        let cmd = line.trim_end().to_string();
        let tag = cmd.split(' ').next().unwrap_or("").to_string();
        let upper = cmd[tag.len()..].trim().to_ascii_uppercase();
        let mut reply = Vec::new();
        if upper.starts_with("CAPABILITY") {
            reply.extend_from_slice(b"* CAPABILITY IMAP4rev1\r\n");
        } else if upper.starts_with("LOGIN") {
            for _ in 0..2 {
                w.write_all(b"+ go\r\n").await.unwrap();
                let n: usize = line
                    .trim_end()
                    .rsplit('{')
                    .next()
                    .unwrap()
                    .trim_end_matches('}')
                    .parse()
                    .unwrap();
                let mut buf = vec![0u8; n];
                r.read_exact(&mut buf).await.unwrap();
                line.clear();
                r.read_line(&mut line).await.unwrap();
            }
        } else if upper.starts_with("LIST") {
            let one = fat_list(8);
            while reply.len() < list_bytes {
                reply.extend_from_slice(&one);
            }
        } else if upper.starts_with("UID FETCH") {
            let one = fat_line();
            while reply.len() < flag_bytes {
                reply.extend_from_slice(&one);
            }
        }
        reply.extend_from_slice(format!("{tag} OK\r\n").as_bytes());
        if w.write_all(&reply).await.is_err() {
            break;
        }
    }
}

async fn login(list_bytes: usize, flag_bytes: usize) -> Session<DuplexStream> {
    let (client, srv) = tokio::io::duplex(64 * 1024);
    tokio::spawn(server(srv, list_bytes, flag_bytes));
    Session::login_stream(client, "user@example.com", &Secret::new("pw"))
        .await
        .unwrap()
}

#[tokio::test]
async fn a_flooded_read_command_is_cut_off_at_the_byte_budget() {
    // 48 MiB in ~100 responses: far below the 5 000-response cap, every literal under 64 KiB.
    let mut s = login(48 * MIB, 0).await;
    assert_eq!(s.list_folders().await.err(), Some(ConnectError::NotImap));
}

#[tokio::test]
async fn a_flooded_flag_scan_is_cut_off_at_the_byte_budget() {
    // 48 MiB in ~8 000 lines: far below the 100 000-line cap, every line under 8 KiB.
    let mut s = login(0, 48 * MIB).await;
    assert_eq!(
        s.fetch_flags(1, 100_000).await.err(),
        Some(RemoteFailure::Protocol)
    );
}

#[tokio::test]
async fn the_budget_is_per_command_not_per_session() {
    // 20 MiB per command is allowed, and a second command gets a fresh budget.
    let mut s = login(20 * MIB, 20 * MIB).await;
    assert!(s.list_folders().await.is_ok());
    assert!(s.list_folders().await.is_ok());
    assert!(s.fetch_flags(1, 100_000).await.is_ok());
}
