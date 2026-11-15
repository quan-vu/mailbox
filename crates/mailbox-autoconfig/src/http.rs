//! Minimal HTTPS GET for autoconfig files. Deliberately small and strict:
//! HTTPS with a validated certificate (via `mailbox-net`), port 443, connect only to addresses
//! that passed [`checked_addrs`], no redirects at all, `Connection: close`, `identity` encoding,
//! hard caps on time and bytes. Only the domain's host name is sent (Host header, SNI).
use crate::detect::{FetchError, Https, Refusal, Resolver, checked_addrs};
use crate::policy::HTTPS_PORT;
use crate::xml::MAX_XML_BYTES;
use mailbox_core::{MailServerEndpoint, TlsMode};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

const HEADER_CAP: usize = 16 * 1024;
/// Wall-clock limit for one request including connect and handshake.
pub const TOTAL_TIMEOUT: Duration = Duration::from_secs(12);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub fn request(host: &str, path: &str) -> String {
    format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: MailBox-autoconfig/1\r\nAccept: text/xml, application/xml;q=0.9\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n"
    )
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn decode_chunked(mut b: &[u8]) -> Result<Vec<u8>, FetchError> {
    let mut out = Vec::new();
    loop {
        let line_end = find(b, b"\r\n").ok_or(FetchError::Protocol)?;
        let size_text = std::str::from_utf8(&b[..line_end]).map_err(|_| FetchError::Protocol)?;
        let size_text = size_text.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| FetchError::Protocol)?;
        b = &b[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        // `size` is server-controlled: compare without arithmetic that could wrap (MB-304).
        if size > MAX_XML_BYTES || out.len() + size > MAX_XML_BYTES {
            return Err(FetchError::TooLarge);
        }
        if b.len() < size + 2 {
            return Err(FetchError::Protocol);
        }
        out.extend_from_slice(&b[..size]);
        b = &b[size + 2..];
    }
}

/// Body of a raw HTTP/1.x response, or why it is not acceptable.
pub fn parse_response(raw: &[u8]) -> Result<Vec<u8>, FetchError> {
    let head_end = find(raw, b"\r\n\r\n").ok_or(FetchError::Protocol)?;
    if head_end > HEADER_CAP {
        return Err(FetchError::TooLarge);
    }
    let head = std::str::from_utf8(&raw[..head_end]).map_err(|_| FetchError::Protocol)?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or(FetchError::Protocol)?;
    let mut parts = status_line.split_whitespace();
    if !parts.next().is_some_and(|v| v.starts_with("HTTP/1.")) {
        return Err(FetchError::Protocol);
    }
    let status: u16 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or(FetchError::Protocol)?;
    match status {
        200 => {}
        300..=399 => return Err(FetchError::Redirect),
        other => return Err(FetchError::Status(other)),
    }
    let mut chunked = false;
    let mut length = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let (name, value) = (
            name.trim().to_ascii_lowercase(),
            value.trim().to_ascii_lowercase(),
        );
        match name.as_str() {
            "transfer-encoding" if value.contains("chunked") => chunked = true,
            "content-length" => length = value.parse::<usize>().ok(),
            "content-encoding" if value != "identity" => return Err(FetchError::Protocol),
            _ => {}
        }
    }
    let body = &raw[head_end + 4..];
    if chunked {
        return decode_chunked(body);
    }
    if length.is_some_and(|l| l > MAX_XML_BYTES) || body.len() > MAX_XML_BYTES {
        return Err(FetchError::TooLarge);
    }
    Ok(body.to_vec())
}

/// Send the request over `stream` and read the answer, with a size cap while reading (an endless
/// body is cut off, not buffered) and a time limit.
pub async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    host: &str,
    path: &str,
    limit: Duration,
) -> Result<Vec<u8>, FetchError> {
    let run = async {
        stream
            .write_all(request(host, path).as_bytes())
            .await
            .map_err(|_| FetchError::Unreachable)?;
        let mut raw = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = stream
                .read(&mut buf)
                .await
                .map_err(|_| FetchError::Protocol)?;
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&buf[..n]);
            if raw.len() > HEADER_CAP + MAX_XML_BYTES + 1024 {
                return Err(FetchError::TooLarge);
            }
        }
        parse_response(&raw)
    };
    tokio::time::timeout(limit, run)
        .await
        .map_err(|_| FetchError::Timeout)?
}

/// HTTPS client that resolves through `R` and connects only to checked addresses.
pub struct HttpsClient<R> {
    pub resolver: R,
}

impl<R: Resolver> Https for HttpsClient<R> {
    async fn get(&self, host: &str, path: &str) -> Result<Vec<u8>, FetchError> {
        let work = async {
            let addrs = checked_addrs(&self.resolver, host, HTTPS_PORT)
                .await
                .map_err(FetchError::Refused)?;
            let endpoint = MailServerEndpoint {
                hostname: host.to_string(),
                port: HTTPS_PORT,
                tls_mode: TlsMode::ImplicitTls,
            };
            let mut last = FetchError::Unreachable;
            for addr in addrs {
                let tcp =
                    match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
                        Ok(Ok(tcp)) => tcp,
                        _ => continue,
                    };
                let mut tls = match mailbox_net::upgrade_tls(tcp, &endpoint).await {
                    Ok(t) => t,
                    Err(_) => {
                        last = FetchError::Tls;
                        continue;
                    }
                };
                return exchange(&mut tls, host, path, TOTAL_TIMEOUT).await;
            }
            Err(last)
        };
        tokio::time::timeout(TOTAL_TIMEOUT, work)
            .await
            .unwrap_or(Err(FetchError::Timeout))
    }
}

impl From<Refusal> for FetchError {
    fn from(r: Refusal) -> Self {
        FetchError::Refused(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    fn ok(body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    /// MB-304: a chunk size near `usize::MAX` wrapped the length checks and panicked on the
    /// slice (or overflowed in debug builds).
    #[test]
    fn huge_chunk_sizes_are_refused_without_panicking() {
        for size in ["ffffffffffffffff", "fffffffffffffffe", "10000000000000000"] {
            let raw = format!(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nx\r\n{size}\r\nyy\r\n0\r\n\r\n"
            );
            assert!(parse_response(raw.as_bytes()).is_err(), "{size}");
        }
    }

    #[test]
    fn request_carries_only_host_and_path() {
        let r = request("autoconfig.example.com", "/mail/config-v1.1.xml");
        assert!(
            r.starts_with("GET /mail/config-v1.1.xml HTTP/1.1\r\nHost: autoconfig.example.com\r\n")
        );
        assert!(
            !r.contains('@')
                && !r.contains('?')
                && !r.to_lowercase().contains("authorization")
                && !r.to_lowercase().contains("cookie")
        );
    }

    #[test]
    fn plain_200_is_accepted() {
        assert_eq!(parse_response(&ok("<x/>")).unwrap(), b"<x/>");
    }

    #[test]
    fn every_redirect_status_is_refused_not_followed() {
        for code in [301, 302, 303, 307, 308] {
            let raw = format!("HTTP/1.1 {code} Moved\r\nLocation: https://evil.example/\r\n\r\n");
            assert_eq!(
                parse_response(raw.as_bytes()),
                Err(FetchError::Redirect),
                "{code}"
            );
        }
        assert_eq!(
            parse_response(b"HTTP/1.1 404 Not Found\r\n\r\n"),
            Err(FetchError::Status(404))
        );
        assert_eq!(
            parse_response(b"HTTP/1.1 500 x\r\n\r\n"),
            Err(FetchError::Status(500))
        );
    }

    #[test]
    fn oversized_declared_or_actual_bodies_are_refused() {
        let big = "a".repeat(MAX_XML_BYTES + 1);
        assert_eq!(parse_response(&ok(&big)), Err(FetchError::TooLarge));
        let lying = format!("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n{big}");
        assert_eq!(parse_response(lying.as_bytes()), Err(FetchError::TooLarge));
        let chunk = format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{big}\r\n0\r\n\r\n",
            big.len()
        );
        assert_eq!(parse_response(chunk.as_bytes()), Err(FetchError::TooLarge));
    }

    #[test]
    fn chunked_bodies_decode() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n<x/>\r\n3;ext=1\r\n123\r\n0\r\n\r\n";
        assert_eq!(parse_response(raw).unwrap(), b"<x/>123");
        assert_eq!(
            parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n"),
            Err(FetchError::Protocol)
        );
        assert_eq!(
            parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nab"),
            Err(FetchError::Protocol)
        );
    }

    #[test]
    fn compressed_or_garbage_responses_are_refused() {
        assert_eq!(
            parse_response(b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\nxx"),
            Err(FetchError::Protocol)
        );
        assert_eq!(
            parse_response(b"SSH-2.0-OpenSSH\r\n\r\n"),
            Err(FetchError::Protocol)
        );
        assert_eq!(parse_response(b"no headers end"), Err(FetchError::Protocol));
        let long_header = format!(
            "HTTP/1.1 200 OK\r\nX: {}\r\n\r\n",
            "a".repeat(HEADER_CAP + 10)
        );
        assert_eq!(
            parse_response(long_header.as_bytes()),
            Err(FetchError::TooLarge)
        );
    }

    #[tokio::test]
    async fn exchange_reads_a_normal_answer() {
        let (mut client, mut server) = duplex(64 * 1024);
        let srv = tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let n = server.read(&mut buf).await.unwrap();
            assert!(
                std::str::from_utf8(&buf[..n])
                    .unwrap()
                    .starts_with("GET /p HTTP/1.1")
            );
            server.write_all(&ok("<ok/>")).await.unwrap();
        });
        let body = exchange(&mut client, "example.com", "/p", Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(body, b"<ok/>");
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn exchange_cuts_off_an_endless_body() {
        let (mut client, mut server) = duplex(64 * 1024);
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let _ = server.read(&mut buf).await;
            let _ = server.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await;
            let chunk = [b'a'; 8192];
            while server.write_all(&chunk).await.is_ok() {}
        });
        let result = exchange(&mut client, "example.com", "/p", Duration::from_secs(5)).await;
        assert_eq!(result, Err(FetchError::TooLarge));
    }

    #[tokio::test]
    async fn exchange_times_out_on_a_silent_server() {
        let (mut client, _server) = duplex(1024);
        let result = exchange(&mut client, "example.com", "/p", Duration::from_millis(80)).await;
        assert_eq!(result, Err(FetchError::Timeout));
    }

    #[tokio::test]
    async fn the_client_refuses_private_hosts_before_connecting() {
        use crate::detect::LookupError;
        use crate::dns::SrvRecord;
        use std::net::IpAddr;
        struct Rebinder;
        impl Resolver for Rebinder {
            async fn ips(&self, _: &str) -> Result<Vec<IpAddr>, LookupError> {
                Ok(vec!["127.0.0.1".parse().unwrap()])
            }
            async fn srv(&self, _: &str) -> Result<Vec<SrvRecord>, LookupError> {
                Err(LookupError::Failed)
            }
        }
        let c = HttpsClient { resolver: Rebinder };
        assert_eq!(
            c.get("autoconfig.example.com", "/x").await,
            Err(FetchError::Refused(Refusal::NonPublic))
        );
        assert_eq!(
            c.get("127.0.0.1", "/x").await,
            Err(FetchError::Refused(Refusal::BadName))
        );
    }
}
