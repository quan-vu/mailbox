//! Data-source boundary (MB-103). Screens and state never know where mailbox data comes from:
//! Desktop reads it through native core use cases, Web through the HTTP API.
//! No credentials cross this boundary; the Web bundle only ever receives DTOs.
use crate::i18n::tr;
use mailbox_contracts::BootstrapResponse;
use std::future::Future;
use std::task::{Context, Poll, Waker};

#[cfg(not(target_arch = "wasm32"))]
pub mod account;
pub mod actions;
pub mod body;
#[cfg(not(target_arch = "wasm32"))]
pub mod clock;
pub mod connect;
#[cfg(not(target_arch = "wasm32"))]
pub mod history;
#[cfg(target_arch = "wasm32")]
mod http;
#[cfg(not(target_arch = "wasm32"))]
pub mod link;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub mod notify;
#[cfg(not(target_arch = "wasm32"))]
pub mod prefs;
#[cfg(not(target_arch = "wasm32"))]
pub mod send;
#[cfg(not(target_arch = "wasm32"))]
pub mod worker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataError {
    /// The source could not be reached (network down, API not running).
    Unavailable,
    /// The source answered, but not with usable mailbox data.
    BadResponse,
}

impl DataError {
    /// User-facing copy: says what happened and the safe next step, without technical detail.
    pub fn message(self) -> &'static str {
        match self {
            DataError::Unavailable => tr(
                "Không kết nối được máy chủ dữ liệu MailBox. Kiểm tra máy chủ đang chạy rồi thử lại.",
            ),
            DataError::BadResponse => tr("Máy chủ trả về dữ liệu không hợp lệ. Thử lại sau."),
        }
    }
}

#[allow(async_fn_in_trait)]
pub trait MailboxDataSource {
    /// Stable identifier shown in diagnostics and tests.
    const KEY: &'static str;
    /// True when data is available in-process without waiting on I/O.
    const LOCAL: bool;
    async fn bootstrap(&self) -> Result<BootstrapResponse, DataError>;
}

#[cfg(target_arch = "wasm32")]
pub use http::HttpDataSource as PlatformSource;
#[cfg(not(target_arch = "wasm32"))]
pub use native::NativeDataSource as PlatformSource;

/// Poll a future once. Local sources resolve immediately, which lets the first render
/// (and server-side rendering in tests) show data without a loading frame.
pub fn now_or_never<F: Future>(future: F) -> Option<F::Output> {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => Some(value),
        Poll::Pending => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contract every data source must satisfy.
    async fn contract(source: &impl MailboxDataSource) {
        let data = source.bootstrap().await.expect("source provides data");
        assert_eq!(data.mode, "demo");
        assert!(!data.accounts.is_empty());
        assert!(
            data.messages
                .iter()
                .all(|m| data.accounts.iter().any(|a| a.id == m.account_id))
        );
        let json = serde_json_like(&data);
        for forbidden in ["password", "token", "secret"] {
            assert!(
                !json.contains(forbidden),
                "{forbidden} leaked through the data source"
            );
        }
    }

    fn serde_json_like(data: &BootstrapResponse) -> String {
        format!("{data:?}").to_lowercase()
    }

    #[test]
    fn native_source_meets_the_contract_and_is_immediate() {
        assert_eq!(PlatformSource::KEY, "native-core");
        const { assert!(PlatformSource::LOCAL) };
        now_or_never(contract(&PlatformSource)).expect("local source resolves without waiting");
    }

    #[test]
    fn errors_have_actionable_copy_without_internals() {
        for e in [DataError::Unavailable, DataError::BadResponse] {
            let m = e.message();
            assert!(m.contains("hử lại"));
            assert!(!m.contains("http") && !m.contains("127.0.0.1"));
        }
    }
}
