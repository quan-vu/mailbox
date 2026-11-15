use super::{DataError, MailboxDataSource};
use mailbox_contracts::BootstrapResponse;

/// API origin, fixed at build time (`MAILBOX_API_URL`). It is never taken from the page URL or
/// user input, so a link cannot point the app at another server.
const API_BASE: &str = match option_env!("MAILBOX_API_URL") {
    Some(url) => url,
    None => "http://127.0.0.1:3000",
};

/// Web source: reads DTOs from the HTTP API. The response is validated with the same core rules
/// as native data before it reaches the UI.
#[derive(Debug, Clone, Copy, Default)]
pub struct HttpDataSource;

impl MailboxDataSource for HttpDataSource {
    const KEY: &'static str = "http-api";
    const LOCAL: bool = false;

    async fn bootstrap(&self) -> Result<BootstrapResponse, DataError> {
        let url = format!("{API_BASE}/api/v1/demo/bootstrap");
        let response = gloo_net::http::Request::get(&url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|_| DataError::Unavailable)?;
        if !response.ok() {
            return Err(DataError::BadResponse);
        }
        let data: BootstrapResponse = response.json().await.map_err(|_| DataError::BadResponse)?;
        mailbox_core::demo::validate(&data).map_err(|_| DataError::BadResponse)?;
        Ok(data)
    }
}
