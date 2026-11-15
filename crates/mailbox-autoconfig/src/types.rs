//! What autoconfiguration returns: servers, where each value came from, and notes.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    ImplicitTls,
    StartTls,
}

impl Security {
    pub fn label(self) -> &'static str {
        match self {
            Security::ImplicitTls => "SSL/TLS",
            Security::StartTls => "STARTTLS",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub security: Security,
}

/// Where a value came from, shown next to it so the user can judge how far to trust it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The built-in table of well-known providers (no network).
    BuiltIn,
    /// DNS SRV records of the domain (RFC 6186).
    DnsSrv,
    /// `autoconfig.<domain>` over HTTPS.
    AutoconfigSubdomain,
    /// `<domain>/.well-known/autoconfig/...` over HTTPS.
    AutoconfigWellKnown,
    /// A common host name that answered with a valid certificate.
    Guess,
}

impl Source {
    pub fn key(self) -> &'static str {
        match self {
            Source::BuiltIn => "built-in",
            Source::DnsSrv => "dns-srv",
            Source::AutoconfigSubdomain => "autoconfig-subdomain",
            Source::AutoconfigWellKnown => "autoconfig-well-known",
            Source::Guess => "guess",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Source::BuiltIn => "Bảng nhà cung cấp có sẵn",
            Source::DnsSrv => "Bản ghi DNS SRV của tên miền",
            Source::AutoconfigSubdomain => "autoconfig.<tên miền> (HTTPS)",
            Source::AutoconfigWellKnown => "<tên miền>/.well-known/autoconfig (HTTPS)",
            Source::Guess => "Đoán tên máy chủ thông dụng, đã kiểm tra chứng chỉ",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub endpoint: Endpoint,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Detection {
    pub imap: Option<Found>,
    pub smtp: Option<Found>,
    /// Provider name when the built-in table matched.
    pub provider: Option<&'static str>,
    /// Extra step the provider needs (for example an app password).
    pub hint: Option<&'static str>,
    /// Short messages about steps that could not run or found nothing.
    pub notes: Vec<&'static str>,
}

impl Detection {
    pub fn is_complete(&self) -> bool {
        self.imap.is_some() && self.smtp.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Imap,
    Smtp,
}
