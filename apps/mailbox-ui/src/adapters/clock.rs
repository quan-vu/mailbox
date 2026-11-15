//! Local time-zone offset from the operating system (MB-218), Desktop only.
//!
//! Unix: `localtime_r`, which applies the system zone rules (including daylight saving) to the
//! given instant. Other platforms: `None`, and the UI shows and picks times in UTC, labelled.

/// Seconds east of UTC at `epoch` in the system time zone, if the platform tells.
#[cfg(unix)]
pub fn local_offset(epoch: i64) -> Option<i32> {
    // `time_t` is 32 bits on some platforms; the conversion is a no-op where it is 64.
    #[allow(clippy::useless_conversion)]
    let t: libc::time_t = epoch.try_into().ok()?;
    // SAFETY: `localtime_r` writes only into `tm`, which we own; it is thread-safe, unlike
    // `localtime`. A null return means the conversion failed and `tm` is not read.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { !libc::localtime_r(&t, &mut tm).is_null() };
    if !ok {
        return None;
    }
    let offset = i32::try_from(tm.tm_gmtoff).ok()?;
    // Real zones are within ±14 h; anything else is not trusted.
    (offset.abs() <= 14 * 3600).then_some(offset)
}

#[cfg(not(unix))]
pub fn local_offset(_epoch: i64) -> Option<i32> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn the_system_offset_is_plausible() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let offset = local_offset(now).expect("unix reports an offset");
        assert!(offset.abs() <= 14 * 3600);
        assert_eq!(offset % 900, 0, "zones are whole quarter hours");
    }
}
