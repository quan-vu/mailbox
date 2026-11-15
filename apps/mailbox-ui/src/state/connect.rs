//! Connect-account wizard state. Demo only: no network access, no password is ever accepted.
use super::compose::is_valid_email;
use crate::i18n::{bi, tr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Google,
    Microsoft,
    Zoho,
    Cpanel,
    Custom,
}

impl Provider {
    pub const ALL: [Provider; 5] = [
        Provider::Google,
        Provider::Microsoft,
        Provider::Zoho,
        Provider::Cpanel,
        Provider::Custom,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Provider::Google => "google",
            Provider::Microsoft => "microsoft",
            Provider::Zoho => "zoho",
            Provider::Cpanel => "cpanel",
            Provider::Custom => "custom",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::Google => "Google Workspace",
            Provider::Microsoft => "Microsoft 365",
            Provider::Zoho => "Zoho Mail",
            Provider::Cpanel => "cPanel / Hosting",
            Provider::Custom => tr("Tùy chỉnh IMAP / SMTP"),
        }
    }

    /// Neutral monogram instead of vendor logos (trademark policy).
    pub fn monogram(self) -> &'static str {
        match self {
            Provider::Google => "G",
            Provider::Microsoft => "M",
            Provider::Zoho => "Z",
            Provider::Cpanel => "cP",
            Provider::Custom => "",
        }
    }

    /// Well-known host suggestions. `{domain}` is replaced by the email domain.
    pub fn suggestion(self, domain: &str) -> Option<(String, u16, String, u16)> {
        match self {
            Provider::Google => Some(("imap.gmail.com".into(), 993, "smtp.gmail.com".into(), 465)),
            Provider::Microsoft => Some((
                "outlook.office365.com".into(),
                993,
                "smtp.office365.com".into(),
                587,
            )),
            Provider::Zoho => Some(("imap.zoho.com".into(), 993, "smtp.zoho.com".into(), 465)),
            Provider::Cpanel if !domain.is_empty() => {
                Some((format!("imap.{domain}"), 993, format!("smtp.{domain}"), 465))
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectForm {
    pub provider: Provider,
    pub autodetect: bool,
    pub email: String,
    pub imap_host: String,
    pub imap_port: String,
    pub smtp_host: String,
    pub smtp_port: String,
}

impl Default for ConnectForm {
    fn default() -> Self {
        Self {
            provider: Provider::Custom,
            autodetect: true,
            email: String::new(),
            imap_host: String::new(),
            imap_port: "993".into(),
            smtp_host: String::new(),
            smtp_port: "465".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Email,
    ImapHost,
    ImapPort,
    SmtpHost,
    SmtpPort,
}

impl Field {
    pub fn key(self) -> &'static str {
        match self {
            Field::Email => "email",
            Field::ImapHost => "imap-host",
            Field::ImapPort => "imap-port",
            Field::SmtpHost => "smtp-host",
            Field::SmtpPort => "smtp-port",
        }
    }
}

fn valid_host(h: &str) -> bool {
    let h = h.trim();
    !h.is_empty()
        && h.len() <= 253
        && h.contains('.')
        && h.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && !h.starts_with(['.', '-'])
        && !h.ends_with(['.', '-'])
}

fn valid_port(p: &str) -> bool {
    p.trim().parse::<u16>().is_ok_and(|n| n > 0)
}

impl ConnectForm {
    pub fn domain(&self) -> String {
        self.email
            .split_once('@')
            .map(|(_, d)| d.trim().to_lowercase())
            .unwrap_or_default()
    }

    /// Fill host/port suggestions for the chosen provider when auto-detect is on.
    pub fn apply_suggestion(&mut self) {
        if !self.autodetect {
            return;
        }
        if let Some((ih, ip, sh, sp)) = self.provider.suggestion(&self.domain()) {
            self.imap_host = ih;
            self.imap_port = ip.to_string();
            self.smtp_host = sh;
            self.smtp_port = sp.to_string();
        }
    }

    /// Fill the server fields from a detection the user accepted. Fields the detection did not
    /// find stay as they are; the email and the password are never touched.
    pub fn apply_detection(&mut self, d: &mailbox_autoconfig::Detection) {
        if let Some(f) = &d.imap {
            self.imap_host = f.endpoint.host.clone();
            self.imap_port = f.endpoint.port.to_string();
        }
        if let Some(f) = &d.smtp {
            self.smtp_host = f.endpoint.host.clone();
            self.smtp_port = f.endpoint.port.to_string();
        }
    }

    pub fn errors(&self) -> Vec<(Field, &'static str)> {
        let mut e = Vec::new();
        if !is_valid_email(&self.email) {
            e.push((
                Field::Email,
                tr("Nhập địa chỉ email hợp lệ, ví dụ ten@congty.vn."),
            ));
        }
        if !valid_host(&self.imap_host) {
            e.push((
                Field::ImapHost,
                tr("Nhập tên máy chủ IMAP, ví dụ imap.congty.vn."),
            ));
        }
        if !valid_port(&self.imap_port) {
            e.push((Field::ImapPort, tr("Cổng phải là số từ 1–65535.")));
        }
        if !valid_host(&self.smtp_host) {
            e.push((
                Field::SmtpHost,
                tr("Nhập tên máy chủ SMTP, ví dụ smtp.congty.vn."),
            ));
        }
        if !valid_port(&self.smtp_port) {
            e.push((Field::SmtpPort, tr("Cổng phải là số từ 1–65535.")));
        }
        e
    }
}

/// Step-2 outcomes the demo can simulate so every state in SCREENS.md S02 is reviewable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    Detecting,
    NeedAppPassword,
    CertificateError,
    AuthFailed,
    ImapOkSmtpFailed,
    Completed,
}

impl Scenario {
    pub const ALL: [Scenario; 6] = [
        Scenario::Detecting,
        Scenario::NeedAppPassword,
        Scenario::CertificateError,
        Scenario::AuthFailed,
        Scenario::ImapOkSmtpFailed,
        Scenario::Completed,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Scenario::Detecting => "detecting",
            Scenario::NeedAppPassword => "need-app-password",
            Scenario::CertificateError => "certificate-error",
            Scenario::AuthFailed => "auth-failed",
            Scenario::ImapOkSmtpFailed => "imap-ok-smtp-failed",
            Scenario::Completed => "completed",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Scenario::Detecting => tr("Đang dò cấu hình"),
            Scenario::NeedAppPassword => tr("Cần mật khẩu ứng dụng"),
            Scenario::CertificateError => tr("Lỗi chứng chỉ"),
            Scenario::AuthFailed => tr("Sai thông tin đăng nhập"),
            Scenario::ImapOkSmtpFailed => tr("IMAP được, SMTP lỗi"),
            Scenario::Completed => tr("Hoàn tất (demo)"),
        }
    }

    /// (tone, message). Tone: "info" | "warn" | "danger".
    pub fn outcome(self, imap_host: &str) -> (&'static str, String) {
        match self {
            Scenario::Detecting => ("info", tr("Đang tìm cấu hình phù hợp… (giả lập, không có kết nối mạng)").into()),
            Scenario::NeedAppPassword => (
                "warn",
                tr("Tài khoản bật xác thực 2 bước: cần tạo mật khẩu ứng dụng tại nhà cung cấp.").into(),
            ),
            Scenario::CertificateError => (
                "danger",
                bi(format!("Chứng chỉ của {imap_host} không hợp lệ. MailBox không cho phép bỏ qua — hãy liên hệ nhà cung cấp."), format!("The certificate of {imap_host} is not valid. MailBox does not allow bypassing this; contact your provider.")),
            ),
            Scenario::AuthFailed => ("danger", tr("Sai thông tin đăng nhập. Kiểm tra mật khẩu ứng dụng.").into()),
            Scenario::ImapOkSmtpFailed => (
                "warn",
                tr("Nhận thư được (IMAP) nhưng máy chủ gửi thư từ chối kết nối. Thử STARTTLS cổng 587.").into(),
            ),
            Scenario::Completed => (
                "warn",
                tr("Kịch bản minh họa kết thúc. Không có tài khoản hay mật khẩu nào được lưu.").into(),
            ),
        }
    }

    pub fn can_finish(self) -> bool {
        self == Scenario::Completed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_form_reports_every_required_field() {
        let mut f = ConnectForm::default();
        f.imap_port.clear();
        let keys: Vec<_> = f.errors().iter().map(|(k, _)| k.key()).collect();
        assert_eq!(keys, vec!["email", "imap-host", "imap-port", "smtp-host"]);
    }

    #[test]
    fn cpanel_suggestion_uses_email_domain_when_autodetect_on() {
        let mut f = ConnectForm {
            provider: Provider::Cpanel,
            email: "minh@Company.vn".into(),
            ..ConnectForm::default()
        };
        f.apply_suggestion();
        assert_eq!(
            (f.imap_host.as_str(), f.smtp_host.as_str()),
            ("imap.company.vn", "smtp.company.vn")
        );
        assert!(f.errors().is_empty());
    }

    #[test]
    fn accepted_detection_fills_servers_only() {
        use mailbox_autoconfig::{Detection, Endpoint, Found, Security, Source};
        let found = |host: &str, port, security| Found {
            endpoint: Endpoint {
                host: host.into(),
                port,
                security,
            },
            source: Source::DnsSrv,
        };
        let mut f = ConnectForm {
            email: "a@corp.vn".into(),
            smtp_host: "mine.corp.vn".into(),
            ..ConnectForm::default()
        };
        f.apply_detection(&Detection {
            imap: Some(found("imap.corp.vn", 993, Security::ImplicitTls)),
            ..Detection::default()
        });
        assert_eq!(
            (f.imap_host.as_str(), f.imap_port.as_str()),
            ("imap.corp.vn", "993")
        );
        assert_eq!(
            f.smtp_host, "mine.corp.vn",
            "an undetected side keeps the user's value"
        );
        assert_eq!(f.email, "a@corp.vn");
        f.apply_detection(&Detection {
            smtp: Some(found("smtp.corp.vn", 587, Security::StartTls)),
            ..Detection::default()
        });
        assert_eq!(
            (f.smtp_host.as_str(), f.smtp_port.as_str()),
            ("smtp.corp.vn", "587")
        );
    }

    #[test]
    fn autodetect_off_keeps_manual_values() {
        let mut f = ConnectForm {
            provider: Provider::Google,
            autodetect: false,
            imap_host: "mail.x.vn".into(),
            ..ConnectForm::default()
        };
        f.apply_suggestion();
        assert_eq!(f.imap_host, "mail.x.vn");
    }

    #[test]
    fn rejects_hosts_with_urls_or_spaces_and_bad_ports() {
        let f = ConnectForm {
            email: "a@b.vn".into(),
            imap_host: "http://imap.b.vn".into(),
            imap_port: "99a".into(),
            smtp_host: "smtp b.vn".into(),
            smtp_port: "70000".into(),
            ..ConnectForm::default()
        };
        assert_eq!(f.errors().len(), 4);
    }

    #[test]
    fn only_completed_scenario_finishes_and_certificate_error_has_no_bypass() {
        assert!(Scenario::Completed.can_finish());
        assert!(!Scenario::CertificateError.can_finish());
        let (tone, msg) = Scenario::CertificateError.outcome("imap.company.vn");
        assert_eq!(tone, "danger");
        assert!(msg.contains("không cho phép bỏ qua"));
    }
}
