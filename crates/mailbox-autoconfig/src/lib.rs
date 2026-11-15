//! Mail server autoconfiguration (MB-202).
//!
//! From the domain of an email address, propose IMAP and SMTP host, port and TLS mode and say
//! where each value came from. Nothing is saved here and nothing is trusted: every candidate that
//! did not come from the built-in table is probed over verified TLS before it is offered, and all
//! network access goes through the SSRF guard in [`detect::checked_addrs`].
//!
//! What leaves the machine: the *domain* (DNS names, HTTPS host, TLS SNI). Never the local part of
//! the address, never a password.
pub mod detect;
pub mod dns;
pub mod policy;
pub mod providers;
pub mod types;
pub mod xml;

#[cfg(not(target_arch = "wasm32"))]
pub mod http;
#[cfg(not(target_arch = "wasm32"))]
pub mod system;

pub use detect::{Https, LookupError, Probe, Refusal, Resolver, checked_addrs, detect};
pub use policy::domain_of;
pub use types::{Detection, Endpoint, Found, Security, Service, Source};
