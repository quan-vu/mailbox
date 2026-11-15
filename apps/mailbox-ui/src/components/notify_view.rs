//! Tells the notification adapter what the user is looking at (list folder, window focus), so
//! mail in the folder on screen does not also pop up a notification (MB-211).
use crate::routes::Route;
use crate::state::AppState;
use crate::state::folders::RealView;
use dioxus::prelude::*;

#[component]
pub fn NotifyViewHost() -> Element {
    let app = use_context::<AppState>();
    let route = use_route::<Route>();
    let on_list = matches!(route, Route::Inbox {});
    let focused = use_signal(|| true);
    use_future(move || async move {
        let mut focused = focused;
        let mut ev = document::eval(
            "const send = () => dioxus.send(document.hasFocus() ? '1' : '0'); \
             window.addEventListener('focus', send); window.addEventListener('blur', send); send();",
        );
        while let Ok(v) = ev.recv::<String>().await {
            focused.set(v == "1");
        }
    });
    let filter = app.filter.read().clone();
    let folder_key = on_list.then(|| match crate::state::folders::RealState::get() {
        Some(real) if app.is_real() => match &*real.view.read() {
            RealView::Inbox => "inbox".to_string(),
            RealView::Role(role) => role.clone(),
            RealView::Folder(id) => id.clone(),
        },
        _ => filter.folder.key().to_string(),
    });
    #[cfg(not(target_arch = "wasm32"))]
    crate::adapters::notify::set_viewing(mailbox_core::notify::Viewing {
        window_focused: focused() && app.is_real(),
        folder_key,
        account_id: filter.account.clone(),
    });
    #[cfg(target_arch = "wasm32")]
    let _ = (folder_key, focused);
    rsx! {}
}
