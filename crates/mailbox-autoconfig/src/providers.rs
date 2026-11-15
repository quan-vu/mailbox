//! Built-in table of well-known providers. Looked up by the email domain, no network.
//! IMAP is always implicit TLS on 993 (the only IMAP mode the app supports); SMTP is implicit TLS
//! on 465 or STARTTLS on 587 as the provider documents it.
use crate::types::{Detection, Endpoint, Found, Security, Source};

pub struct Provider {
    pub name: &'static str,
    pub domains: &'static [&'static str],
    pub imap_host: &'static str,
    pub smtp_host: &'static str,
    pub smtp_port: u16,
    /// Shown to the user when the provider does not accept the normal account password.
    pub hint: Option<&'static str>,
}

const APP_PASSWORD: &str = "Nhà cung cấp này yêu cầu mật khẩu ứng dụng (bật xác minh 2 bước rồi tạo mật khẩu ứng dụng trong cài đặt tài khoản), không dùng mật khẩu đăng nhập thường.";

pub const PROVIDERS: &[Provider] = &[
    Provider {
        name: "Gmail / Google",
        domains: &["gmail.com", "googlemail.com"],
        imap_host: "imap.gmail.com",
        smtp_host: "smtp.gmail.com",
        smtp_port: 465,
        hint: Some(APP_PASSWORD),
    },
    Provider {
        name: "Outlook / Microsoft",
        domains: &[
            "outlook.com",
            "hotmail.com",
            "live.com",
            "msn.com",
            "outlook.vn",
            "hotmail.co.uk",
            "live.co.uk",
        ],
        imap_host: "outlook.office365.com",
        smtp_host: "smtp-mail.outlook.com",
        smtp_port: 587,
        hint: Some(
            "Microsoft chỉ cho đăng nhập IMAP bằng mật khẩu ứng dụng khi tài khoản bật xác minh 2 bước; nhiều tài khoản công ty đã tắt hẳn mật khẩu IMAP và cần OAuth (chưa có trong bản này, xem MB-203).",
        ),
    },
    Provider {
        name: "Yahoo Mail",
        domains: &[
            "yahoo.com",
            "ymail.com",
            "rocketmail.com",
            "yahoo.co.uk",
            "yahoo.fr",
            "yahoo.de",
            "yahoo.com.vn",
        ],
        imap_host: "imap.mail.yahoo.com",
        smtp_host: "smtp.mail.yahoo.com",
        smtp_port: 465,
        hint: Some(APP_PASSWORD),
    },
    Provider {
        name: "iCloud Mail",
        domains: &["icloud.com", "me.com", "mac.com"],
        imap_host: "imap.mail.me.com",
        smtp_host: "smtp.mail.me.com",
        smtp_port: 587,
        hint: Some(
            "iCloud yêu cầu mật khẩu riêng cho ứng dụng (tạo tại appleid.apple.com › Đăng nhập và bảo mật › Mật khẩu dành riêng cho ứng dụng).",
        ),
    },
    Provider {
        name: "Zoho Mail",
        domains: &["zoho.com", "zohomail.com"],
        imap_host: "imap.zoho.com",
        smtp_host: "smtp.zoho.com",
        smtp_port: 465,
        hint: Some(
            "Zoho: nếu bật xác thực 2 lớp, dùng mật khẩu ứng dụng; IMAP phải được bật trong Cài đặt › Mail Accounts. Tài khoản ở trung tâm dữ liệu EU/IN dùng tên miền riêng (zoho.eu, zoho.in).",
        ),
    },
    Provider {
        name: "Zoho Mail (EU)",
        domains: &["zoho.eu", "zohomail.eu"],
        imap_host: "imap.zoho.eu",
        smtp_host: "smtp.zoho.eu",
        smtp_port: 465,
        hint: None,
    },
    Provider {
        name: "Fastmail",
        domains: &["fastmail.com", "fastmail.fm", "messagingengine.com"],
        imap_host: "imap.fastmail.com",
        smtp_host: "smtp.fastmail.com",
        smtp_port: 465,
        hint: Some(APP_PASSWORD),
    },
    Provider {
        name: "Yandex Mail",
        domains: &["yandex.com", "yandex.ru", "ya.ru"],
        imap_host: "imap.yandex.com",
        smtp_host: "smtp.yandex.com",
        smtp_port: 465,
        hint: Some("Yandex: bật IMAP trong cài đặt thư và dùng mật khẩu ứng dụng."),
    },
];
// Vietnamese hosting and mail providers (FPT, VNPT, Viettel, Mắt Bão, P.A Vietnam, ...) are not in
// this table: their documented hosts could not be verified from this build. They are found by the
// next steps (SRV, autoconfig, verified guesses `imap.`/`mail.`/`smtp.`) or typed by hand.

/// Table entry for an email domain (lowercase), if any.
pub fn lookup(domain: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.domains.contains(&domain))
}

/// Detection result for a table hit, without any network access.
pub fn detection(provider: &'static Provider) -> Detection {
    let imap = Endpoint {
        host: provider.imap_host.into(),
        port: 993,
        security: Security::ImplicitTls,
    };
    let smtp = Endpoint {
        host: provider.smtp_host.into(),
        port: provider.smtp_port,
        security: if provider.smtp_port == 465 {
            Security::ImplicitTls
        } else {
            Security::StartTls
        },
    };
    Detection {
        imap: Some(Found {
            endpoint: imap,
            source: Source::BuiltIn,
        }),
        smtp: Some(Found {
            endpoint: smtp,
            source: Source::BuiltIn,
        }),
        provider: Some(provider.name),
        hint: provider.hint,
        notes: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{allowed_mail_port, valid_hostname};

    #[test]
    fn well_known_domains_hit() {
        for (d, imap) in [
            ("gmail.com", "imap.gmail.com"),
            ("outlook.com", "outlook.office365.com"),
            ("hotmail.com", "outlook.office365.com"),
            ("yahoo.com", "imap.mail.yahoo.com"),
            ("icloud.com", "imap.mail.me.com"),
            ("zoho.com", "imap.zoho.com"),
            ("fastmail.com", "imap.fastmail.com"),
            ("yandex.com", "imap.yandex.com"),
        ] {
            let p = lookup(d).unwrap_or_else(|| panic!("{d}"));
            assert_eq!(p.imap_host, imap);
            let det = detection(p);
            assert!(det.is_complete());
            assert_eq!(det.imap.unwrap().source, Source::BuiltIn);
        }
        assert!(lookup("example.org").is_none());
        assert!(lookup("notgmail.com").is_none());
        assert!(lookup("gmail.com.evil.test").is_none());
    }

    #[test]
    fn every_entry_is_well_formed_and_domains_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for p in PROVIDERS {
            assert!(
                valid_hostname(p.imap_host) && valid_hostname(p.smtp_host),
                "{}",
                p.name
            );
            assert!(
                allowed_mail_port(p.smtp_port) && p.smtp_port != 143,
                "{}",
                p.name
            );
            for d in p.domains {
                assert!(seen.insert(*d), "{d} listed twice");
                assert_eq!(*d, d.to_ascii_lowercase());
            }
        }
    }

    #[test]
    fn providers_that_need_an_app_password_say_so() {
        for d in ["gmail.com", "yahoo.com", "icloud.com", "fastmail.com"] {
            let h = detection(lookup(d).unwrap()).hint.unwrap();
            assert!(h.contains("mật khẩu"), "{d}");
        }
    }

    #[test]
    fn smtp_security_follows_the_port() {
        let d = detection(lookup("outlook.com").unwrap());
        assert_eq!(d.smtp.unwrap().endpoint.security, Security::StartTls);
        let d = detection(lookup("gmail.com").unwrap());
        assert_eq!(d.smtp.unwrap().endpoint.security, Security::ImplicitTls);
    }
}
