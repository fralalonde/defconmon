//! Wall-clock time in the container's local timezone.
//!
//! Uses `localtime_r` so the offset (and DST) is whatever the system says,
//! rather than being computed from UTC by hand. Resolved once and cached: the
//! dashboard only needs minute accuracy and a restart fixes a DST edge.
use std::sync::OnceLock;

fn tz_offset_secs() -> i64 {
    static OFF: OnceLock<i64> = OnceLock::new();
    *OFF.get_or_init(|| unsafe {
        let mut now: libc::time_t = 0;
        libc::time(&mut now);
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            0
        } else {
            tm.tm_gmtoff as i64
        }
    })
}

/// (hour, minute, second) in local time.
pub fn now_hms() -> (u32, u32, u32) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let sod = (secs + tz_offset_secs()).rem_euclid(86400);
    ((sod / 3600) as u32, ((sod / 60) % 60) as u32, (sod % 60) as u32)
}

/// "YYYY-MM-DD" in local time.
pub fn now_date() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = (secs + tz_offset_secs()).div_euclid(86400);
    // civil-from-days (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}
