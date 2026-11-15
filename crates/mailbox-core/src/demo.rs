//! Demo data use case. The store returns deterministic fixtures; `load_bootstrap` is the single
//! place that enforces the "demo never looks real" invariants before data reaches any UI or API.
use crate::MailboxError;
use mailbox_contracts::{AccountStatus, BootstrapResponse, fixtures};

/// Read port for the initial mailbox snapshot. Real stores (SQLite/PostgreSQL) implement this later.
pub trait BootstrapPort {
    fn bootstrap(&self) -> Result<BootstrapResponse, MailboxError>;
}

/// In-process fixture store used by the Desktop demo and the local demo API.
#[derive(Debug, Clone, Copy, Default)]
pub struct DemoStore;

impl BootstrapPort for DemoStore {
    fn bootstrap(&self) -> Result<BootstrapResponse, MailboxError> {
        Ok(fixtures::bootstrap())
    }
}

/// Reject data that would mislead the user: messages without a known account, or demo data
/// claiming a verified connection. Applied to every snapshot, whatever transport delivered it.
pub fn validate(data: &BootstrapResponse) -> Result<(), MailboxError> {
    let orphan = data
        .messages
        .iter()
        .any(|m| !data.accounts.iter().any(|a| a.id == m.account_id));
    if orphan {
        return Err(MailboxError::InconsistentData(
            "message without a known account",
        ));
    }
    if data.mode == "demo"
        && data
            .accounts
            .iter()
            .any(|a| a.status == AccountStatus::Connected)
    {
        return Err(MailboxError::InconsistentData(
            "demo account claims a verified connection",
        ));
    }
    Ok(())
}

/// Load the snapshot from a store and validate it.
pub fn load_bootstrap(port: &impl BootstrapPort) -> Result<BootstrapResponse, MailboxError> {
    let data = port.bootstrap()?;
    validate(&data)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(BootstrapResponse);
    impl BootstrapPort for Fake {
        fn bootstrap(&self) -> Result<BootstrapResponse, MailboxError> {
            Ok(self.0.clone())
        }
    }

    struct Down;
    impl BootstrapPort for Down {
        fn bootstrap(&self) -> Result<BootstrapResponse, MailboxError> {
            Err(MailboxError::TransientConnectionFailure)
        }
    }

    #[test]
    fn demo_store_passes_validation() {
        let data = load_bootstrap(&DemoStore).expect("fixtures are consistent");
        assert_eq!(data.mode, "demo");
        assert_eq!(data.accounts.len(), 3);
    }

    #[test]
    fn rejects_messages_without_account() {
        let mut data = fixtures::bootstrap();
        data.messages[0].account_id = "ghost".into();
        assert_eq!(
            load_bootstrap(&Fake(data)),
            Err(MailboxError::InconsistentData(
                "message without a known account"
            ))
        );
    }

    #[test]
    fn rejects_demo_data_that_claims_connected() {
        let mut data = fixtures::bootstrap();
        data.accounts[0].status = AccountStatus::Connected;
        assert!(matches!(
            load_bootstrap(&Fake(data)),
            Err(MailboxError::InconsistentData(_))
        ));
    }

    #[test]
    fn store_errors_pass_through() {
        assert_eq!(
            load_bootstrap(&Down),
            Err(MailboxError::TransientConnectionFailure)
        );
    }
}
