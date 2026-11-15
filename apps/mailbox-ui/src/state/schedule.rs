//! Scheduled send in the composer (MB-218): the time the user picks (local wall clock) to UTC and
//! back, the rule the UI states, and the loop that sends due messages while the app runs.
//!
//! Times are stored as UTC epoch seconds. The local offset comes from the operating system for
//! the instant in question (so a date across a daylight-saving change gets its own offset); when
//! the platform cannot tell, times are shown and picked in UTC and labelled as such.
// The Web build has no sender: the loop and the offset source are Desktop-only.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]
use crate::i18n::tr;
use mailbox_core::outbox::ScheduleError;

/// The rule shown wherever a send time is picked or shown. Must match
/// [`mailbox_core::outbox::due_action`]; a test keeps the hour count in step.
pub const RULE_TEXT: &str = "Thư hẹn giờ chỉ được gửi khi MailBox đang mở. Nếu lúc đến giờ MailBox \
    đang đóng hoặc máy đang ngủ, thư được gửi ngay khi MailBox mở lại trong vòng 12 giờ sau giờ \
    hẹn; trễ hơn 12 giờ, thư không tự gửi mà chờ bạn quyết định.";

pub const RULE_TEXT_EN: &str = "Scheduled mail is sent only while MailBox is open. If MailBox is \
    closed or the computer is asleep at the scheduled time, the message is sent as soon as MailBox \
    opens again within 12 hours of that time; later than 12 hours, it is not sent automatically and \
    waits for your decision.";

/// The rule in the interface language.
pub fn rule_text() -> &'static str {
    crate::i18n::bi(RULE_TEXT, RULE_TEXT_EN)
}

/// The "closed app" limit in the interface language.
pub fn closed_app_note() -> &'static str {
    crate::i18n::bi(
        CLOSED_APP_NOTE,
        "MailBox cannot send mail while the app is closed.",
    )
}

/// Short label for the "closed app" limit.
pub const CLOSED_APP_NOTE: &str = "MailBox không gửi được thư khi ứng dụng đang đóng.";

pub fn error_text(e: ScheduleError) -> &'static str {
    match e {
        ScheduleError::TooSoon => tr("Chọn thời điểm ít nhất 1 phút sau bây giờ."),
        ScheduleError::TooFar => tr("Chỉ hẹn giờ được trong vòng một năm."),
    }
}

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Civil date of a day count since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        _ => 28,
    }
}

/// Wall-clock seconds (as if UTC) of a `datetime-local` value `YYYY-MM-DDTHH:MM[:SS]`.
pub fn parse_local(input: &str) -> Option<i64> {
    let (date, time) = input.trim().split_once(['T', ' '])?;
    let mut d = date.split('-');
    let y: i64 = d.next()?.parse().ok()?;
    let m: u32 = d.next()?.parse().ok()?;
    let day: u32 = d.next()?.parse().ok()?;
    let mut t = time.split(':');
    let h: i64 = t.next()?.parse().ok()?;
    let mi: i64 = t.next()?.parse().ok()?;
    let s: i64 = t.next().map(str::parse).transpose().ok()?.unwrap_or(0);
    if d.next().is_some()
        || t.next().is_some()
        || !(1970..=9999).contains(&y)
        || !(1..=12).contains(&m)
        || day == 0
        || day > days_in_month(y, m)
        || !(0..24).contains(&h)
        || !(0..60).contains(&mi)
        || !(0..60).contains(&s)
    {
        return None;
    }
    Some(days_from_civil(y, m, day) * 86_400 + h * 3600 + mi * 60 + s)
}

/// UTC epoch of a local wall-clock value. `offset_at(epoch)` is the local offset (seconds east of
/// UTC) at that instant. Two passes settle the offset across a daylight-saving change.
pub fn local_to_utc(input: &str, offset_at: impl Fn(i64) -> i32) -> Option<i64> {
    let wall = parse_local(input)?;
    let first = wall - i64::from(offset_at(wall));
    Some(wall - i64::from(offset_at(first)))
}

/// `datetime-local` value (`YYYY-MM-DDTHH:MM`) of an instant, in the given offset.
pub fn input_value(epoch: i64, offset: i32) -> String {
    let wall = epoch + i64::from(offset);
    let (y, m, d) = civil_from_days(wall.div_euclid(86_400));
    let secs = wall.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60
    )
}

/// `UTC+07:00`-style name of an offset.
pub fn offset_name(offset: i32) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let a = offset.unsigned_abs();
    format!("UTC{sign}{:02}:{:02}", a / 3600, (a % 3600) / 60)
}

/// `17/11/2026 08:30 (UTC+07:00)`. Without a known offset the time is shown in UTC.
pub fn display(epoch: i64, offset: Option<i32>) -> String {
    let off = offset.unwrap_or(0);
    let wall = epoch + i64::from(off);
    let (y, m, d) = civil_from_days(wall.div_euclid(86_400));
    let secs = wall.rem_euclid(86_400);
    format!(
        "{d:02}/{m:02}/{y:04} {:02}:{:02} ({})",
        secs / 3600,
        (secs % 3600) / 60,
        offset_name(off)
    )
}

/// The next day at `hour:00` local time.
pub fn tomorrow_at(now: i64, hour: u32, offset_at: impl Fn(i64) -> i32) -> i64 {
    let wall = now + i64::from(offset_at(now));
    let day = wall.div_euclid(86_400) + 1;
    let (y, m, d) = civil_from_days(day);
    let text = format!("{y:04}-{m:02}-{d:02}T{hour:02}:00");
    local_to_utc(&text, offset_at).unwrap_or(now + 86_400)
}

/// One hour from now, rounded up to the next 5 minutes.
pub fn in_one_hour(now: i64) -> i64 {
    let t = now + 3600;
    t + (300 - t.rem_euclid(300)) % 300
}

/// Local offset at an instant, from the operating system; `None` when unknown (shown as UTC).
pub fn local_offset(epoch: i64) -> Option<i32> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::adapters::clock::local_offset(epoch)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = epoch;
        None
    }
}

/// [`local_offset`] with UTC as the fallback, for conversions.
pub fn offset_or_utc(epoch: i64) -> i32 {
    local_offset(epoch).unwrap_or(0)
}

pub fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// How long the loop sleeps at most between two looks at the outbox.
pub const TICK_SECS: i64 = 15;

/// Seconds to wait before the next look: until the next send time, at most [`TICK_SECS`].
pub fn next_wait(next: Option<i64>, now: i64) -> i64 {
    next.map_or(TICK_SECS, |n| (n - now).clamp(1, TICK_SECS))
}

/// Send due scheduled messages for as long as the app runs (Desktop, Real mode only). Messages
/// missed by more than the grace period are marked and left for the user.
#[cfg(not(target_arch = "wasm32"))]
pub async fn run(app: super::AppState) {
    use dioxus::prelude::*;
    loop {
        let now = now_epoch();
        let mut wait = TICK_SECS;
        if *app.mode.peek() == super::AppMode::Real
            && let Ok((due, next, missed)) = crate::adapters::send::take_due(now).await
        {
            for draft in &due {
                super::outbox::send_scheduled(app, &draft.id).await;
            }
            if missed > 0 {
                app.notify(
                        tr("Có thư hẹn giờ đã quá hạn hơn 12 giờ nên chưa được gửi. Mở Bản nháp để quyết định."),
                        false,
                    );
            }
            if !due.is_empty() || missed > 0 {
                super::outbox::refresh_items().await;
            }
            wait = next_wait(next, now_epoch());
        }
        tokio::time::sleep(std::time::Duration::from_secs(wait.unsigned_abs())).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailbox_core::outbox::MISSED_GRACE_SECS;

    const VN: i32 = 7 * 3600;

    #[test]
    fn civil_dates_round_trip_including_leap_days() {
        for (y, m, d) in [
            (1970, 1, 1),
            (2000, 2, 29),
            (2024, 2, 29),
            (2026, 11, 15),
            (2100, 3, 1),
        ] {
            assert_eq!(civil_from_days(days_from_civil(y, m, d)), (y, m, d));
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    #[test]
    fn local_input_is_parsed_strictly() {
        assert_eq!(parse_local("1970-01-01T00:01"), Some(60));
        assert!(parse_local("2026-11-15T08:30:15").is_some());
        for bad in [
            "",
            "2026-11-15",
            "2026-13-01T08:00",
            "2026-02-29T08:00",
            "2026-11-31T08:00",
            "2026-11-15T24:00",
            "2026-11-15T08:60",
            "2026-11-15T08:00:00:00",
            "x-11-15T08:00",
        ] {
            assert_eq!(parse_local(bad), None, "{bad}");
        }
        assert!(parse_local("2024-02-29T08:00").is_some());
    }

    #[test]
    fn vietnam_time_converts_to_utc_and_back() {
        // 08:30 in Hà Nội (UTC+7) is 01:30 UTC.
        let at = local_to_utc("2026-11-16T08:30", |_| VN).unwrap();
        assert_eq!(input_value(at, 0), "2026-11-16T01:30");
        assert_eq!(input_value(at, VN), "2026-11-16T08:30");
        assert_eq!(display(at, Some(VN)), "16/11/2026 08:30 (UTC+07:00)");
        assert_eq!(display(at, None), "16/11/2026 01:30 (UTC+00:00)");
        // Crossing midnight backwards.
        let early = local_to_utc("2026-11-16T03:00", |_| VN).unwrap();
        assert_eq!(input_value(early, 0), "2026-11-15T20:00");
        assert_eq!(offset_name(-(3 * 3600 + 1800)), "UTC-03:30");
    }

    #[test]
    fn daylight_saving_dates_get_their_own_offset() {
        // A zone at UTC+1 in winter and UTC+2 from 2026-03-29 01:00 UTC.
        let change = local_to_utc("2026-03-29T01:00", |_| 0).unwrap();
        let cet = move |epoch: i64| if epoch >= change { 7200 } else { 3600 };
        let winter = local_to_utc("2026-03-28T09:00", cet).unwrap();
        let summer = local_to_utc("2026-03-30T09:00", cet).unwrap();
        assert_eq!(input_value(winter, 0), "2026-03-28T08:00");
        assert_eq!(input_value(summer, 0), "2026-03-30T07:00");
        assert_eq!(input_value(summer, cet(summer)), "2026-03-30T09:00");
    }

    #[test]
    fn presets_land_where_they_say() {
        let now = local_to_utc("2026-11-15T22:10", |_| VN).unwrap();
        assert_eq!(
            input_value(tomorrow_at(now, 8, |_| VN), VN),
            "2026-11-16T08:00"
        );
        // Just after midnight local, "tomorrow" is the next local day, not the UTC one.
        let late = local_to_utc("2026-11-16T00:30", |_| VN).unwrap();
        assert_eq!(
            input_value(tomorrow_at(late, 8, |_| VN), VN),
            "2026-11-17T08:00"
        );
        let hour = in_one_hour(now);
        assert_eq!(hour % 300, 0);
        assert!((3600..3900).contains(&(hour - now)));
    }

    #[test]
    fn the_rule_text_matches_the_grace_period_and_waits_are_bounded() {
        let hours = MISSED_GRACE_SECS / 3600;
        assert!(RULE_TEXT.contains(&format!("{hours} giờ sau giờ hẹn")));
        assert!(RULE_TEXT.contains("khi MailBox đang mở"));
        assert_eq!(next_wait(None, 100), TICK_SECS);
        assert_eq!(next_wait(Some(103), 100), 3);
        assert_eq!(next_wait(Some(50), 100), 1);
        assert_eq!(next_wait(Some(10_000), 100), TICK_SECS);
    }
}
