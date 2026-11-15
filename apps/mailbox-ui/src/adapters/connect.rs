//! Account connection check boundary (MB-111). Desktop performs a real IMAP check over verified
//! TLS; the Web build has no backend for it yet (MB-110) and keeps the password field locked.
//! The password is handed over once, wrapped as a `Secret`, and is not stored anywhere.

/// True when this build can run a real connection check.
use crate::i18n::tr;
pub const REAL_CHECK: bool = cfg!(not(target_arch = "wasm32"));

/// Result of a successful real check. Contains no account data beyond a message count.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckOk {
    pub capabilities: Vec<String>,
    pub inbox_messages: Option<u32>,
}

// Each platform constructs only some variants (Web: `Unsupported`; Desktop: the rest).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckFailure {
    InvalidEndpoint,
    Unreachable,
    Timeout,
    CertificateInvalid,
    TlsFailed,
    NotImap,
    LoginDisabled,
    AuthFailed,
    NotSmtp,
    StartTlsUnavailable,
    AuthUnsupported,
    /// A host taken from autoconfig resolved to a loopback, private or link-local address
    /// (SSRF guard, MB-304). The user may still type a local server by hand.
    BlockedAddress,
    /// Local database or keychain failed.
    Storage,
    Unsupported,
}

impl CheckFailure {
    pub fn key(self) -> &'static str {
        match self {
            CheckFailure::InvalidEndpoint => "invalid-endpoint",
            CheckFailure::Unreachable => "unreachable",
            CheckFailure::Timeout => "timeout",
            CheckFailure::CertificateInvalid => "certificate-error",
            CheckFailure::TlsFailed => "tls-failed",
            CheckFailure::NotImap => "not-imap",
            CheckFailure::LoginDisabled => "login-disabled",
            CheckFailure::AuthFailed => "auth-failed",
            CheckFailure::NotSmtp => "not-smtp",
            CheckFailure::StartTlsUnavailable => "starttls-unavailable",
            CheckFailure::AuthUnsupported => "auth-unsupported",
            CheckFailure::BlockedAddress => "blocked-address",
            CheckFailure::Storage => "storage",
            CheckFailure::Unsupported => "unsupported",
        }
    }

    /// What happened and the safe next step. Never suggests weakening TLS.
    pub fn message(self) -> &'static str {
        match self {
            CheckFailure::InvalidEndpoint => {
                tr("Tên máy chủ hoặc cổng không hợp lệ. Kiểm tra lại thông tin IMAP.")
            }
            CheckFailure::Unreachable => {
                tr("Không kết nối được tới máy chủ. Kiểm tra tên máy chủ, cổng và kết nối mạng.")
            }
            CheckFailure::Timeout => {
                tr("Máy chủ không phản hồi kịp. Kiểm tra cổng IMAP (thường là 993) rồi thử lại.")
            }
            CheckFailure::CertificateInvalid => tr(
                "Chứng chỉ của máy chủ không hợp lệ cho tên máy chủ này. MailBox không cho phép bỏ qua — hãy dùng đúng tên máy chủ do nhà cung cấp hướng dẫn hoặc liên hệ họ.",
            ),
            CheckFailure::TlsFailed => {
                tr("Không thiết lập được kết nối mã hóa với máy chủ. Kiểm tra cổng SSL/TLS (993).")
            }
            CheckFailure::NotImap => {
                tr("Máy chủ này không trả lời như một máy chủ IMAP. Kiểm tra tên máy chủ và cổng.")
            }
            CheckFailure::LoginDisabled => tr(
                "Máy chủ không cho đăng nhập bằng mật khẩu (thường cần OAuth). Loại tài khoản này chưa được hỗ trợ.",
            ),
            CheckFailure::AuthFailed => tr(
                "Sai địa chỉ email hoặc mật khẩu. Nếu tài khoản bật xác thực 2 bước, hãy dùng mật khẩu ứng dụng.",
            ),
            CheckFailure::NotSmtp => tr(
                "Máy chủ này không trả lời như một máy chủ gửi thư (SMTP). Kiểm tra tên máy chủ và cổng (465 hoặc 587).",
            ),
            CheckFailure::StartTlsUnavailable => tr(
                "Máy chủ không hỗ trợ mã hóa STARTTLS trên cổng này nên MailBox dừng lại, không gửi mật khẩu. Hãy dùng cổng 465 (SSL/TLS).",
            ),
            CheckFailure::AuthUnsupported => tr(
                "Máy chủ gửi thư không cho đăng nhập bằng mật khẩu (thường cần OAuth). Loại tài khoản này chưa được hỗ trợ.",
            ),
            CheckFailure::BlockedAddress => tr(
                "Máy chủ do tự động phát hiện trỏ tới một địa chỉ nội bộ (mạng riêng hoặc máy này) nên MailBox không kết nối. Nếu đây đúng là máy chủ trong mạng của bạn, hãy nhập tay tên máy chủ.",
            ),
            CheckFailure::Storage => tr(
                "Không lưu được dữ liệu trên máy này (cơ sở dữ liệu cục bộ hoặc Keychain). Kiểm tra quyền truy cập rồi thử lại.",
            ),
            CheckFailure::Unsupported => tr("Bản này chưa hỗ trợ kiểm tra kết nối thật."),
        }
    }
}

/// True when `host` is the one autoconfig proposed for that service (IMAP when `imap`). Such a
/// host came from DNS / an HTTPS answer the domain controls, so its final check is guarded.
pub fn host_from_detection(
    detection: Option<&mailbox_autoconfig::Detection>,
    imap: bool,
    host: &str,
) -> bool {
    let found = detection.and_then(|d| {
        if imap {
            d.imap.as_ref()
        } else {
            d.smtp.as_ref()
        }
    });
    found.is_some_and(|f| f.endpoint.host.eq_ignore_ascii_case(host.trim()))
}

/// Resolve `host` once through the autoconfig SSRF guard and return the addresses the check
/// must connect to (MB-304). Any non-public answer refuses the whole host.
#[cfg(not(target_arch = "wasm32"))]
pub async fn guarded_addrs<R: mailbox_autoconfig::Resolver>(
    resolver: &R,
    host: &str,
    port: u16,
) -> Result<Vec<std::net::SocketAddr>, CheckFailure> {
    use mailbox_autoconfig::Refusal;
    mailbox_autoconfig::checked_addrs(resolver, host.trim(), port)
        .await
        .map_err(|r| match r {
            Refusal::BadName => CheckFailure::InvalidEndpoint,
            Refusal::NonPublic => CheckFailure::BlockedAddress,
            Refusal::NoAddress | Refusal::Lookup => CheckFailure::Unreachable,
        })
}

/// Check IMAP login over verified TLS. Returns the server capabilities on success. With
/// `guarded` (host proposed by autoconfig) the host is resolved once through the SSRF guard and
/// only the checked addresses are contacted.
#[cfg(not(target_arch = "wasm32"))]
pub async fn check_imap(
    host: &str,
    port: u16,
    username: &str,
    password: String,
    guarded: bool,
) -> Result<CheckOk, CheckFailure> {
    let resolver = mailbox_autoconfig::system::SystemResolver;
    check_imap_via(&resolver, guarded, host, port, username, password).await
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn check_imap_via<R: mailbox_autoconfig::Resolver>(
    resolver: &R,
    guarded: bool,
    host: &str,
    port: u16,
    username: &str,
    password: String,
) -> Result<CheckOk, CheckFailure> {
    use mailbox_core::secrets::Secret;
    use mailbox_core::{MailServerEndpoint, TlsMode};
    use mailbox_imap::ConnectError;
    let secret = Secret::new(password);
    let endpoint = MailServerEndpoint {
        hostname: host.to_string(),
        port,
        tls_mode: TlsMode::ImplicitTls,
    };
    let result = if guarded {
        let addrs = guarded_addrs(resolver, host, port).await?;
        mailbox_imap::check_login_pinned(&endpoint, &addrs, username, &secret).await
    } else {
        mailbox_imap::check_login(&endpoint, username, &secret).await
    };
    result
        .map(|info| CheckOk {
            capabilities: info.capabilities,
            inbox_messages: info.inbox_messages,
        })
        .map_err(|e| match e {
            ConnectError::InvalidEndpoint | ConnectError::UnsupportedTlsMode => {
                CheckFailure::InvalidEndpoint
            }
            ConnectError::Unreachable => CheckFailure::Unreachable,
            ConnectError::Timeout => CheckFailure::Timeout,
            ConnectError::CertificateInvalid => CheckFailure::CertificateInvalid,
            ConnectError::TlsFailed => CheckFailure::TlsFailed,
            ConnectError::NotImap => CheckFailure::NotImap,
            ConnectError::LoginDisabled => CheckFailure::LoginDisabled,
            ConnectError::AuthFailed => CheckFailure::AuthFailed,
        })
}

/// Result of a successful SMTP check. No mail is sent by the check.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SmtpOk {
    pub max_size_bytes: Option<u64>,
}

/// Check SMTP login over verified TLS: implicit TLS on 465, STARTTLS on any other port.
/// `guarded` as for [`check_imap`].
#[cfg(not(target_arch = "wasm32"))]
pub async fn check_smtp(
    host: &str,
    port: u16,
    username: &str,
    password: String,
    guarded: bool,
) -> Result<SmtpOk, CheckFailure> {
    let resolver = mailbox_autoconfig::system::SystemResolver;
    check_smtp_via(&resolver, guarded, host, port, username, password).await
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn check_smtp_via<R: mailbox_autoconfig::Resolver>(
    resolver: &R,
    guarded: bool,
    host: &str,
    port: u16,
    username: &str,
    password: String,
) -> Result<SmtpOk, CheckFailure> {
    use mailbox_core::secrets::Secret;
    use mailbox_core::{MailServerEndpoint, TlsMode};
    use mailbox_smtp::SmtpError;
    let secret = Secret::new(password);
    let tls_mode = if port == 465 {
        TlsMode::ImplicitTls
    } else {
        TlsMode::StartTls
    };
    let endpoint = MailServerEndpoint {
        hostname: host.to_string(),
        port,
        tls_mode,
    };
    let result = if guarded {
        let addrs = guarded_addrs(resolver, host, port).await?;
        mailbox_smtp::check_pinned(&endpoint, &addrs, Some((username, &secret))).await
    } else {
        mailbox_smtp::check(&endpoint, Some((username, &secret))).await
    };
    result
        .map(|info| SmtpOk {
            max_size_bytes: info.max_size(),
        })
        .map_err(|e| match e {
            SmtpError::InvalidEndpoint | SmtpError::InvalidAddress => CheckFailure::InvalidEndpoint,
            SmtpError::Unreachable => CheckFailure::Unreachable,
            SmtpError::Timeout => CheckFailure::Timeout,
            SmtpError::CertificateInvalid => CheckFailure::CertificateInvalid,
            SmtpError::TlsFailed => CheckFailure::TlsFailed,
            SmtpError::NotSmtp => CheckFailure::NotSmtp,
            SmtpError::StartTlsUnavailable => CheckFailure::StartTlsUnavailable,
            SmtpError::AuthUnsupported => CheckFailure::AuthUnsupported,
            SmtpError::AuthFailed => CheckFailure::AuthFailed,
        })
}

#[cfg(target_arch = "wasm32")]
pub async fn check_smtp(
    _host: &str,
    _port: u16,
    _username: &str,
    _password: String,
    _guarded: bool,
) -> Result<SmtpOk, CheckFailure> {
    Err(CheckFailure::Unsupported)
}

#[cfg(target_arch = "wasm32")]
pub async fn check_imap(
    _host: &str,
    _port: u16,
    _username: &str,
    _password: String,
    _guarded: bool,
) -> Result<CheckOk, CheckFailure> {
    Err(CheckFailure::Unsupported)
}

/// Autoconfiguration for the domain of `email` (MB-202), Desktop only. Only the domain is used;
/// the local part never leaves this function. `None` when the address has no usable domain.
#[cfg(not(target_arch = "wasm32"))]
pub async fn detect_servers(email: &str) -> Option<mailbox_autoconfig::Detection> {
    let domain = mailbox_autoconfig::domain_of(email)?;
    Some(mailbox_autoconfig::system::detect_system(&domain).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [CheckFailure; 14] = [
        CheckFailure::InvalidEndpoint,
        CheckFailure::Unreachable,
        CheckFailure::Timeout,
        CheckFailure::CertificateInvalid,
        CheckFailure::TlsFailed,
        CheckFailure::NotImap,
        CheckFailure::LoginDisabled,
        CheckFailure::AuthFailed,
        CheckFailure::NotSmtp,
        CheckFailure::StartTlsUnavailable,
        CheckFailure::AuthUnsupported,
        CheckFailure::BlockedAddress,
        CheckFailure::Storage,
        CheckFailure::Unsupported,
    ];

    #[test]
    fn every_failure_has_copy_and_none_offers_a_tls_bypass() {
        for f in ALL {
            let m = f.message().to_lowercase();
            assert!(m.len() > 20, "{f:?}");
            for risky in [
                "tắt ssl",
                "tắt tls",
                "không mã hóa",
                "cổng 143",
                "tin cậy chứng chỉ",
                "vẫn tiếp tục",
            ] {
                assert!(!m.contains(risky), "{f:?} suggests `{risky}`");
            }
        }
        assert!(
            CheckFailure::CertificateInvalid
                .message()
                .contains("không cho phép bỏ qua")
        );
    }

    // MB-304: the final check of a host found by autoconfig goes through the same SSRF guard,
    // resolving once; a DNS answer that changes afterwards cannot redirect the connection.
    #[cfg(not(target_arch = "wasm32"))]
    mod guard {
        use super::super::*;
        use mailbox_autoconfig::dns::SrvRecord;
        use mailbox_autoconfig::{
            Detection, Endpoint, Found, LookupError, Resolver, Security, Source,
        };
        use std::cell::Cell;
        use std::net::IpAddr;

        /// First answer public, every later answer loopback: a DNS rebinding attacker.
        struct Rebinding {
            calls: Cell<u32>,
        }
        impl Resolver for Rebinding {
            async fn ips(&self, _: &str) -> Result<Vec<IpAddr>, LookupError> {
                let n = self.calls.get();
                self.calls.set(n + 1);
                Ok(vec![
                    if n == 0 { "93.184.216.34" } else { "127.0.0.1" }
                        .parse()
                        .unwrap(),
                ])
            }
            async fn srv(&self, _: &str) -> Result<Vec<SrvRecord>, LookupError> {
                Err(LookupError::Failed)
            }
        }

        struct Fixed(Vec<&'static str>);
        impl Resolver for Fixed {
            async fn ips(&self, _: &str) -> Result<Vec<IpAddr>, LookupError> {
                Ok(self.0.iter().map(|s| s.parse().unwrap()).collect())
            }
            async fn srv(&self, _: &str) -> Result<Vec<SrvRecord>, LookupError> {
                Err(LookupError::Failed)
            }
        }

        #[tokio::test]
        async fn rebinding_after_the_check_cannot_change_the_address_used() {
            let r = Rebinding {
                calls: Cell::new(0),
            };
            let addrs = guarded_addrs(&r, "imap.example.com", 993).await.unwrap();
            assert_eq!(addrs, vec!["93.184.216.34:993".parse().unwrap()]);
            assert_eq!(r.calls.get(), 1, "the name is resolved exactly once");
        }

        #[tokio::test]
        async fn private_or_mixed_answers_are_refused_before_any_connection() {
            for answer in [
                vec!["127.0.0.1"],
                vec!["93.184.216.34", "10.0.0.5"],
                vec!["169.254.169.254"],
            ] {
                assert_eq!(
                    guarded_addrs(&Fixed(answer.clone()), "imap.example.com", 993).await,
                    Err(CheckFailure::BlockedAddress),
                    "{answer:?}"
                );
            }
            assert_eq!(
                check_imap_via(
                    &Fixed(vec!["127.0.0.1"]),
                    true,
                    "imap.example.com",
                    993,
                    "u",
                    "p".into()
                )
                .await,
                Err(CheckFailure::BlockedAddress)
            );
            assert_eq!(
                check_smtp_via(
                    &Fixed(vec!["::1"]),
                    true,
                    "smtp.example.com",
                    465,
                    "u",
                    "p".into()
                )
                .await,
                Err(CheckFailure::BlockedAddress)
            );
            assert_eq!(
                guarded_addrs(&Fixed(vec![]), "imap.example.com", 993).await,
                Err(CheckFailure::Unreachable)
            );
            assert_eq!(
                guarded_addrs(&Fixed(vec!["8.8.8.8"]), "localhost", 993).await,
                Err(CheckFailure::InvalidEndpoint)
            );
        }

        #[test]
        fn only_hosts_taken_from_the_detection_are_guarded() {
            let found = |h: &str, port| Found {
                endpoint: Endpoint {
                    host: h.into(),
                    port,
                    security: Security::ImplicitTls,
                },
                source: Source::Guess,
            };
            let d = Detection {
                imap: Some(found("mail.corp.vn", 993)),
                smtp: Some(found("smtp.corp.vn", 465)),
                ..Detection::default()
            };
            assert!(host_from_detection(Some(&d), true, "MAIL.corp.vn"));
            assert!(host_from_detection(Some(&d), false, " smtp.corp.vn "));
            assert!(
                !host_from_detection(Some(&d), true, "imap.lan.corp.vn"),
                "typed by the user"
            );
            assert!(!host_from_detection(None, true, "mail.corp.vn"));
        }

        #[test]
        fn blocked_address_copy_points_to_manual_entry_not_a_bypass() {
            let m = CheckFailure::BlockedAddress.message();
            assert!(m.contains("nhập tay") || m.contains("Nhập tay"));
            assert_eq!(CheckFailure::BlockedAddress.key(), "blocked-address");
        }
    }
}
