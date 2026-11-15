use super::icon::Icon;
use crate::i18n::tr;
use crate::state::{AppState, demo};
use dioxus::prelude::*;

#[component]
pub fn Avatar(name: String, #[props(default = "")] size: &'static str) -> Element {
    let tone = demo::avatar_tone(&name);
    let text = demo::initials(&name);
    rsx! {
        span { class: "avatar tone-{tone} {size}", "aria-hidden": "true", "{text}" }
    }
}

/// Status badges always pair text with an icon; colour is never the only signal.
#[component]
pub fn DemoBadge(
    #[props(default = "Demo")] label: &'static str,
    #[props(default)] testid: &'static str,
) -> Element {
    rsx! {
        span { class: "badge badge-demo", "data-testid": testid, "data-status": "demo",
            Icon { name: "info", size: 14 }
            "{label}"
        }
    }
}

#[component]
pub fn Switch(
    testid: String,
    label: String,
    checked: bool,
    #[props(default)] disabled: bool,
    onchange: EventHandler<bool>,
) -> Element {
    rsx! {
        button {
            class: if checked { "switch on" } else { "switch" },
            r#type: "button",
            role: "switch",
            "aria-checked": if checked { "true" } else { "false" },
            "aria-label": "{label}",
            "data-testid": "{testid}",
            disabled,
            onclick: move |_| onchange.call(!checked),
            span { class: "knob" }
        }
    }
}

#[component]
pub fn EmptyState(
    icon: &'static str,
    title: String,
    detail: String,
    #[props(default)] clear: Option<EventHandler<()>>,
    #[props(default = "empty-state")] testid: &'static str,
) -> Element {
    rsx! {
        div { class: "empty-state", "data-testid": testid, role: "status",
            span { class: "empty-icon", Icon { name: icon, size: 28 } }
            strong { "{title}" }
            p { "{detail}" }
            if let Some(clear) = clear {
                button { class: "btn", r#type: "button", "data-testid": "empty-state-clear", onclick: move |_| clear.call(()), {tr("Xóa bộ lọc")} }
            }
        }
    }
}

#[component]
pub fn ToastHost() -> Element {
    let app = use_context::<AppState>();
    let mut toast = app.toast;
    let current = toast.read().clone();
    rsx! {
        if let Some(t) = current {
            div { class: "toast", role: "status", "aria-live": "polite", "data-testid": "toast",
                Icon { name: "check", size: 16 }
                span { "{t.message}" }
                if t.undo {
                    button { class: "toast-action", r#type: "button", "data-testid": "toast-undo",
                        onclick: move |_| { crate::state::folders::undo(app); toast.set(None); },
                        Icon { name: "undo", size: 16 } {tr("Hoàn tác")}
                    }
                }
                button { class: "toast-close", r#type: "button", "aria-label": tr("Đóng thông báo"), onclick: move |_| toast.set(None),
                    Icon { name: "x", size: 16 }
                }
            }
        }
    }
}

#[component]
pub fn ConfirmHost() -> Element {
    let app = use_context::<AppState>();
    let mut confirm = app.confirm;
    let current = confirm.read().clone();
    rsx! {
        if let Some(c) = current {
            div { class: "overlay", onclick: move |_| confirm.set(None),
                div {
                    class: "dialog",
                    role: "dialog",
                    "aria-modal": "true",
                    "aria-labelledby": "confirm-title",
                    "data-testid": "confirm-dialog",
                    onclick: move |e| e.stop_propagation(),
                    h2 { id: "confirm-title", "{c.title}" }
                    p { "data-testid": "confirm-dialog-body", "{c.body}" }
                    div { class: "dialog-actions",
                        button { class: "btn", r#type: "button", "data-testid": "confirm-dialog-cancel",
                            onmounted: move |e| async move { let _ = e.set_focus(true).await; },
                            onclick: move |_| confirm.set(None), {tr("Hủy")}
                        }
                        button { class: if c.danger { "btn btn-danger" } else { "btn btn-primary" }, r#type: "button", "data-testid": "confirm-dialog-ok",
                            onclick: move |_| app.run_confirm(), "{c.ok_label}"
                        }
                    }
                }
            }
        }
    }
}
