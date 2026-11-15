//! The link from a message that waits for the user's decision (AUDIT S-09).
//!
//! Links in the mail frame are inert (`mailbox_core::link::guard_links`). A click reaches the
//! Desktop shell as a blocked navigation, which lands here; nothing opens until the user has
//! seen the real address and confirmed.
use dioxus::prelude::*;
use mailbox_core::link::{LinkRefusal, LinkTarget, inspect, unguard};

/// `Ok`: ask before opening. `Err`: explain why MailBox does not open it.
pub static PENDING: GlobalSignal<Option<Result<LinkTarget, LinkRefusal>>> = Signal::global(|| None);

/// A click on a guarded link: decode it, inspect it and ask.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn request(navigation: &str) {
    let decision = unguard(navigation)
        .ok_or(LinkRefusal::Malformed)
        .and_then(|url| inspect(&url));
    *PENDING.write() = Some(decision);
}

pub fn dismiss() {
    *PENDING.write() = None;
}
