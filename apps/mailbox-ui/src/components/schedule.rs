//! Scheduled send controls (MB-218): pick a send time in the composer, see and cancel a schedule
//! in the composer and the outbox. Every place that shows a schedule also states the rule: mail is
//! sent only while MailBox runs, and a time missed by more than 12 hours waits for the user.
use crate::components::icon::Icon;
use crate::i18n::{bi, tr};
use crate::state::AppState;
use crate::state::outbox::{self, CAN_SEND};
use crate::state::schedule::{
    closed_app_note, display, in_one_hour, input_value, local_offset, local_to_utc, now_epoch,
    offset_name, offset_or_utc, rule_text, tomorrow_at,
};
use dioxus::prelude::*;
use mailbox_core::outbox::SendIntent;

static SCHEDULE_CSS: Asset = asset!("/assets/schedule.css");

/// Where times are read and shown: the computer's zone, or UTC when it cannot be read.
fn zone_note(now: i64) -> String {
    match local_offset(now) {
        Some(off) => bi(
            format!("Giờ theo máy này ({}).", offset_name(off)),
            format!("Times follow this computer ({}).", offset_name(off)),
        ),
        None => tr("Không đọc được múi giờ của máy: giờ được tính theo UTC.").to_string(),
    }
}

/// The "Hẹn giờ gửi" button of the composer footer and its picker.
#[component]
pub fn ScheduleControl(locked: bool) -> Element {
    let app = use_context::<AppState>();
    let real = app.is_real();
    let mut open = use_signal(|| false);
    let mut value = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    if !real || !CAN_SEND {
        return rsx! {
            button { class: "btn", r#type: "button", disabled: true, "data-testid": "compose-schedule-open",
                title: tr("Bản demo không gửi thư nên không hẹn giờ được"),
                Icon { name: "calendar" } {tr("Hẹn giờ gửi (demo)")} }
        };
    }
    let now = now_epoch();
    let min = input_value(now + 60, offset_or_utc(now));
    rsx! {
        document::Stylesheet { href: SCHEDULE_CSS }
        div { class: "schedule-wrap",
            button { class: "btn", r#type: "button", "data-testid": "compose-schedule-open", disabled: locked,
                "aria-expanded": if open() { "true" } else { "false" },
                onclick: move |_| {
                    let now = now_epoch();
                    if value.peek().is_empty() {
                        let at = in_one_hour(now);
                        value.set(input_value(at, offset_or_utc(at)));
                    }
                    error.set(None);
                    open.toggle();
                },
                Icon { name: "calendar" } {tr("Hẹn giờ gửi")} }
            if open() && !locked {
                div { class: "schedule-panel", role: "dialog", "aria-label": tr("Hẹn giờ gửi"), "data-testid": "compose-schedule-panel",
                    strong { {tr("Gửi thư này vào lúc")} }
                    div { class: "schedule-presets",
                        button { class: "btn btn-sm", r#type: "button", "data-testid": "compose-schedule-preset-hour",
                            onclick: move |_| { let at = in_one_hour(now_epoch()); value.set(input_value(at, offset_or_utc(at))); },
                            {tr("Sau 1 giờ")} }
                        button { class: "btn btn-sm", r#type: "button", "data-testid": "compose-schedule-preset-morning",
                            onclick: move |_| { let at = tomorrow_at(now_epoch(), 8, offset_or_utc); value.set(input_value(at, offset_or_utc(at))); },
                            {tr("8:00 sáng mai")} }
                    }
                    input { class: "input", r#type: "datetime-local", "data-testid": "compose-schedule-input",
                        "aria-label": tr("Thời điểm gửi"), min: "{min}", value: "{value}",
                        oninput: move |e| value.set(e.value()) }
                    small { class: "muted", "data-testid": "compose-schedule-zone", "{zone_note(now)}" }
                    p { class: "schedule-rule small", "data-testid": "compose-schedule-rule", {rule_text()} }
                    if let Some(e) = error() {
                        p { class: "schedule-error small", role: "alert", "data-testid": "compose-schedule-error", "{e}" }
                    }
                    div { class: "schedule-actions",
                        button { class: "btn btn-sm", r#type: "button", "data-testid": "compose-schedule-cancel-pick",
                            onclick: move |_| open.set(false), {tr("Thôi")} }
                        button { class: "btn btn-primary btn-sm", r#type: "button", "data-testid": "compose-schedule-confirm", disabled: busy(),
                            onclick: move |_| {
                                let blockers = app.draft.read().real_send_blockers();
                                if let Some(first) = blockers.first() {
                                    error.set(Some(bi(format!("Chưa hẹn giờ được: {first}"), format!("Cannot schedule yet: {first}"))));
                                    return;
                                }
                                let Some(at) = local_to_utc(&value.peek(), offset_or_utc) else {
                                    error.set(Some(tr("Chọn ngày và giờ gửi.").to_string()));
                                    return;
                                };
                                busy.set(true);
                                spawn(async move {
                                    let result = outbox::schedule_send(app, at).await;
                                    busy.set(false);
                                    match result {
                                        Ok(()) => { open.set(false); error.set(None); }
                                        Err(e) => error.set(Some(e.to_string())),
                                    }
                                });
                            },
                            {tr("Hẹn giờ")} }
                    }
                }
            }
        }
    }
}

/// The composer's state block while the open draft is scheduled.
#[component]
pub fn ScheduledState(at: Option<i64>) -> Element {
    let app = use_context::<AppState>();
    let when = at.map_or_else(
        || tr("thời điểm đã chọn").to_string(),
        |t| display(t, local_offset(t)),
    );
    rsx! {
        document::Stylesheet { href: SCHEDULE_CSS }
        Icon { name: "calendar" }
        div { class: "grow", "data-testid": "compose-scheduled",
            strong { {bi(format!("Đã hẹn giờ gửi: {when}"), format!("Scheduled to send: {when}"))} }
            p { {tr("Thư đang chờ trong hộp thư đi và nội dung được khóa. Muốn sửa, hãy hủy hẹn giờ trước.")} }
            p { class: "schedule-rule small", {rule_text()} }
            p { class: "schedule-limit small", "data-testid": "compose-schedule-limit", {closed_app_note()} }
            div { class: "send-actions",
                button { class: "btn btn-sm", r#type: "button", "data-testid": "compose-schedule-cancel",
                    onclick: move |_| { spawn(async move {
                        if !outbox::cancel_schedule(app).await {
                            app.notify(tr("Không hủy được: thư đã bắt đầu gửi."), false);
                        }
                    }); },
                    {tr("Hủy hẹn giờ để sửa")} }
                button { class: "btn btn-primary btn-sm", r#type: "button", "data-testid": "compose-schedule-send-now",
                    onclick: move |_| { spawn(async move {
                        if outbox::cancel_schedule(app).await {
                            outbox::send(app, SendIntent::Send).await;
                        } else {
                            app.notify(tr("Thư đã bắt đầu gửi theo giờ hẹn."), false);
                        }
                    }); },
                    {tr("Gửi ngay")} }
            }
        }
    }
}

/// Send time and cancel button of a scheduled outbox entry.
#[component]
pub fn OutboxScheduleInfo(id: String, at: Option<i64>) -> Element {
    let app = use_context::<AppState>();
    let when = at.map_or_else(|| "—".to_string(), |t| display(t, local_offset(t)));
    let cancel_id = id.clone();
    rsx! {
        document::Stylesheet { href: SCHEDULE_CSS }
        small { class: "schedule-when", "data-testid": "outbox-scheduled-at-{id}", {bi(format!("Gửi lúc {when}"), format!("Sends at {when}"))} }
        button { class: "btn btn-sm", r#type: "button", "data-testid": "outbox-unschedule-{id}",
            onclick: move |_| {
                let id = cancel_id.clone();
                spawn(async move {
                    if !outbox::cancel_item(&id).await {
                        app.notify(tr("Không hủy được: thư đã bắt đầu gửi."), false);
                    }
                });
            },
            {tr("Hủy hẹn giờ")} }
    }
}

/// One line for the outbox header: the rule, and that a closed app cannot send.
#[component]
pub fn OutboxScheduleNote() -> Element {
    rsx! {
        p { class: "muted small schedule-note", "data-testid": "outbox-schedule-rule", {rule_text()} }
    }
}
