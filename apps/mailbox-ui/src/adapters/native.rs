use super::{DataError, MailboxDataSource};
use mailbox_contracts::BootstrapResponse;
use mailbox_core::MailboxError;
use mailbox_core::demo::{DemoStore, load_bootstrap};

/// Desktop source: calls native Rust use cases in-process. No HTTP, no serialization.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeDataSource;

impl MailboxDataSource for NativeDataSource {
    const KEY: &'static str = "native-core";
    const LOCAL: bool = true;

    async fn bootstrap(&self) -> Result<BootstrapResponse, DataError> {
        load_bootstrap(&DemoStore).map_err(|e| match e {
            MailboxError::TransientConnectionFailure => DataError::Unavailable,
            _ => DataError::BadResponse,
        })
    }
}
