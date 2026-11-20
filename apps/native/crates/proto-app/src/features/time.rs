//! Local wall-clock formatting for task times ("14:05" today, "31.10. 14:05" before), as the reference's
//! `toLocaleTimeString("de-DE")`. The zone offset comes from the C library's `localtime_r` (macOS `struct tm`
//! carries `tm_gmtoff`), so DST is right for each instant.

/// Seconds east of UTC at `epoch_s`, from the system zone.
#[cfg(target_os = "macos")]
pub fn utc_offset_s(epoch_s: i64) -> i64 {
    #[repr(C)]
    struct Tm {
        tm_sec: i32,
        tm_min: i32,
        tm_hour: i32,
        tm_mday: i32,
        tm_mon: i32,
        tm_year: i32,
        tm_wday: i32,
        tm_yday: i32,
        tm_isdst: i32,
        tm_gmtoff: std::ffi::c_long,
        tm_zone: *const std::ffi::c_char,
    }
    unsafe extern "C" {
        fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    }
    let mut tm = std::mem::MaybeUninit::<Tm>::zeroed();
    // SAFETY: localtime_r writes a full `struct tm` (layout as Darwin's <time.h>) or returns null.
    let ok = unsafe { !localtime_r(&epoch_s, tm.as_mut_ptr()).is_null() };
    if ok { unsafe { tm.assume_init().tm_gmtoff as i64 } } else { 0 }
}

#[cfg(not(target_os = "macos"))]
pub fn utc_offset_s(_epoch_s: i64) -> i64 {
    0
}

/// `YYYY-MM-DDTHH:MM[:SS[.fff]](Z|±HH:MM)` → epoch seconds. None when it doesn't parse.
pub fn parse_iso(iso: &str) -> Option<i64> {
    let b = iso.as_bytes();
    let num = |a: usize, n: usize| -> Option<i64> { iso.get(a..a + n)?.parse().ok() };
    let (y, mo, d, h, mi) = (num(0, 4)?, num(5, 2)?, num(8, 2)?, num(11, 2)?, num(14, 2)?);
    let mut i = 16;
    let mut sec = 0;
    if b.get(i) == Some(&b':') {
        sec = num(17, 2)?;
        i = 19;
        while i < b.len() && (b[i] == b'.' || b[i].is_ascii_digit()) {
            i += 1;
        }
    }
    let offset = match b.get(i) {
        None | Some(b'Z') | Some(b'z') => 0,
        Some(&sign @ (b'+' | b'-')) => {
            let (oh, om) = (num(i + 1, 2)?, num(i + 4, 2).unwrap_or(0));
            (oh * 3600 + om * 60) * if sign == b'-' { -1 } else { 1 }
        }
        _ => return None,
    };
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + sec - offset)
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Days since 1970-01-01 → (year, month, day).
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Local (day number, hour, minute) of `epoch_s` with zone `offset` seconds.
fn local_parts(epoch_s: i64, offset: i64) -> (i64, i64, i64) {
    let t = epoch_s + offset;
    (t.div_euclid(86_400), t.rem_euclid(86_400) / 3600, t.rem_euclid(3600) / 60)
}

/// "14:05" when `iso` is on the same local day as `now_s`, else "31.10. 14:05"; "" when it doesn't parse.
pub fn when_with(iso: &str, now_s: i64, offset_at: impl Fn(i64) -> i64) -> String {
    let Some(t) = parse_iso(iso) else { return String::new() };
    let (day, h, m) = local_parts(t, offset_at(t));
    let (today, _, _) = local_parts(now_s, offset_at(now_s));
    if day == today {
        format!("{h:02}:{m:02}")
    } else {
        let (_, mo, d) = civil_from_days(day);
        format!("{d:02}.{mo:02}. {h:02}:{m:02}")
    }
}

/// [`when_with`] in the system zone, now.
pub fn when(iso: &str) -> String {
    let now = (proto_core::ops::now_ms() / 1000.0) as i64;
    when_with(iso, now, utc_offset_s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iso_forms() {
        assert_eq!(parse_iso("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(parse_iso("1970-01-01T01:00+01:00"), Some(0));
        assert_eq!(parse_iso("2026-11-18T10:05:00Z"), Some(days_from_civil(2026, 11, 18) * 86_400 + 36_300));
        assert_eq!(civil_from_days(days_from_civil(2026, 2, 28)), (2026, 2, 28));
        assert_eq!(parse_iso("nope"), None);
    }

    #[test]
    fn formats_in_the_local_zone() {
        let now = parse_iso("2026-11-18T22:30:00Z").unwrap();
        let berlin = |_: i64| 3600;
        // 22:05 UTC is 23:05 the same Berlin day.
        assert_eq!(when_with("2026-11-18T22:05:00.000Z", now, berlin), "23:05");
        // 23:30 UTC is already the next day in Berlin; `now` (23:30 Berlin) is still the 18th.
        assert_eq!(when_with("2026-11-18T23:30:00Z", now, berlin), "19.11. 00:30");
        assert_eq!(when_with("2026-11-12T14:47:00Z", now, berlin), "12.11. 15:47");
    }

    #[test]
    fn system_offset_is_sane() {
        let o = utc_offset_s(1_800_000_000);
        assert!((-14 * 3600..=14 * 3600).contains(&o));
    }
}

/// Epoch milliseconds as "18.11. 14:05" (local), for lists of past items.
pub fn when_ms(ms: f64) -> String {
    let t = (ms / 1000.0) as i64;
    let (day, h, m) = local_parts(t, utc_offset_s(t));
    let (_, mo, d) = civil_from_days(day);
    format!("{d:02}.{mo:02}. {h:02}:{m:02}")
}
