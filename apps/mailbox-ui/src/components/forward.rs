//! Forward keeps the original's attachments (track G): looks them up in the cached body when a
//! forward is opened in Real mode and says plainly when the original is not downloaded yet.
use crate::state::AppState;
use crate::state::compose::ReplyKind;
use crate::state::outbox::{self, FORWARD_FILES};
use dioxus::prelude::*;

#[component]
pub fn ForwardFilesNote() -> Element {
    let app = use_context::<AppState>();
    let source = match app.draft.read().reply_source.clone() {
        Some((id, ReplyKind::Forward)) if app.is_real() => Some(id),
        _ => None,
    };
    let looked_up = FORWARD_FILES
        .read()
        .as_ref()
        .filter(|(s, _)| Some(s) == source.as_ref())
        .map(|(_, f)| *f);
    // Once per forward opened in this composer (a new forward of the same message looks again,
    // e.g. after the original was downloaded).
    let mut checked = use_signal(|| None::<String>);
    use_effect(move || {
        let source = match app.draft.read().reply_source.clone() {
            Some((id, ReplyKind::Forward)) => Some(id),
            _ => None,
        };
        if source.is_some() && app.is_real() && *checked.peek() != source {
            checked.set(source);
            spawn(outbox::attach_forwarded(app));
        }
    });
    let Some(status) = looked_up else {
        return rsx! {};
    };
    rsx! {
        if let Some(text) = status.message() {
            div { class: if status == outbox::ForwardFiles::NotDownloaded { "alert alert-warn" } else { "alert" },
                "data-testid": "compose-forward-files", "data-state": status.key(),
                span { "{text}" }
            }
        }
    }
}
