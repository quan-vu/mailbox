//! Desktop side of link confirmation (AUDIT S-09).
//!
//! [`gate`] is the WebView's navigation handler. Links in mail carry the inert `mailbox-link:`
//! scheme, so a click arrives here instead of in a browser; the address is queued for the UI,
//! which asks the user. Only after that does [`open`] hand the address to the operating system.
use mailbox_core::link::{GUARD_SCHEME, LinkTarget, MAX_URL};
use std::sync::{Mutex, OnceLock};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

struct Queue {
    tx: UnboundedSender<String>,
    rx: Mutex<Option<UnboundedReceiver<String>>>,
}

fn queue() -> &'static Queue {
    static QUEUE: OnceLock<Queue> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let (tx, rx) = unbounded_channel();
        Queue {
            tx,
            rx: Mutex::new(Some(rx)),
        }
    })
}

/// Navigation policy for everything the framework does not serve itself. Returns whether the
/// WebView may navigate: only the frame's own `about:srcdoc`/`about:blank` documents may.
/// A guarded link never navigates; it is queued for the confirmation dialog.
#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
pub fn gate(url: &str) -> bool {
    if url.starts_with(GUARD_SCHEME) {
        if url.len() <= GUARD_SCHEME.len() + 2 * MAX_URL {
            let _ = queue().tx.send(url.to_string());
        }
        return false;
    }
    url.starts_with("about:")
}

/// Hand queued clicks to the UI. Runs for the life of the app; a second call returns at once.
pub async fn pump() {
    let taken = queue().rx.lock().ok().and_then(|mut rx| rx.take());
    let Some(mut rx) = taken else { return };
    while let Some(navigation) = rx.recv().await {
        crate::state::link::request(&navigation);
    }
}

/// Open a confirmed link with the system's default browser or mail program.
pub fn open(target: &LinkTarget) -> bool {
    webbrowser::open(&target.url).is_ok()
}

#[cfg(test)]
mod tests {
    use super::gate;

    #[test]
    fn the_webview_never_navigates_to_a_link_from_mail() {
        // The frame's own document loads.
        assert!(gate("about:srcdoc") && gate("about:blank"));
        // A guarded link is taken over; anything else is simply refused.
        for blocked in [
            "mailbox-link:68747470733a2f2f6578616d706c652e6f72672f",
            "mailbox-link:",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,x",
            "ftp://example.org/",
            "custom-scheme://x",
        ] {
            assert!(!gate(blocked), "{blocked}");
        }
    }
}
