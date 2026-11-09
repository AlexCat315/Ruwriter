//! Short edit times for the document list: relative for the past week, a
//! calendar date after that.

use ruwriter_core::document::now;

pub fn relative(timestamp: u64) -> String {
    relative_to(timestamp, now(), local_offset_seconds())
}

pub fn relative_to(timestamp: u64, now: u64, offset: i64) -> String {
    let elapsed = now.saturating_sub(timestamp);
    match elapsed {
        0..60 => "刚刚".to_owned(),
        60..3600 => format!("{} 分钟前", elapsed / 60),
        3600..86_400 => format!("{} 小时前", elapsed / 3600),
        86_400..604_800 => format!("{} 天前", elapsed / 86_400),
        _ => {
            let (year, month, day) = civil(timestamp as i64 + offset);
            let (current, _, _) = civil(now as i64 + offset);
            if year == current {
                format!("{month}月{day}日")
            } else {
                format!("{year}年{month}月{day}日")
            }
        }
    }
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant).
fn civil(seconds: i64) -> (i64, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(unix)]
fn local_offset_seconds() -> i64 {
    let now = now() as libc::time_t;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::zeroed();
    // SAFETY: `localtime_r` writes into the provided buffer and reads `now`
    // only; both outlive the call.
    let result = unsafe { libc::localtime_r(&now, tm.as_mut_ptr()) };
    if result.is_null() {
        return 0;
    }
    // SAFETY: `localtime_r` succeeded, so the struct is initialized.
    // `tm_gmtoff` is a C `long`: 32-bit on some Android targets.
    #[allow(clippy::useless_conversion)]
    i64::from(unsafe { tm.assume_init() }.tm_gmtoff)
}

#[cfg(windows)]
fn local_offset_seconds() -> i64 {
    #[repr(C)]
    struct SystemTime {
        parts: [u16; 8],
    }
    #[repr(C)]
    struct TimeZoneInformation {
        bias: i32,
        standard_name: [u16; 32],
        standard_date: SystemTime,
        standard_bias: i32,
        daylight_name: [u16; 32],
        daylight_date: SystemTime,
        daylight_bias: i32,
    }
    unsafe extern "system" {
        fn GetTimeZoneInformation(information: *mut TimeZoneInformation) -> u32;
    }
    let mut information = std::mem::MaybeUninit::<TimeZoneInformation>::zeroed();
    // SAFETY: the kernel fills the caller-owned struct; the declaration
    // matches `TIME_ZONE_INFORMATION`.
    let kind = unsafe { GetTimeZoneInformation(information.as_mut_ptr()) };
    if kind == u32::MAX {
        return 0;
    }
    // SAFETY: the call succeeded and zeroed memory is a valid struct anyway.
    let information = unsafe { information.assume_init() };
    let bias = match kind {
        2 => information.bias + information.daylight_bias,
        _ => information.bias + information.standard_bias,
    };
    -i64::from(bias) * 60
}

#[cfg(not(any(unix, windows)))]
fn local_offset_seconds() -> i64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_times_are_relative() {
        assert_eq!(relative_to(1000, 1030, 0), "刚刚");
        assert_eq!(relative_to(1000, 1000 + 125, 0), "2 分钟前");
        assert_eq!(relative_to(0, 3 * 3600, 0), "3 小时前");
        assert_eq!(relative_to(0, 2 * 86_400, 0), "2 天前");
    }

    #[test]
    fn older_times_are_dates() {
        // 2026-03-05 12:00 UTC, viewed on 2026-11-09.
        let march = 1_772_712_000;
        let november = 1_794_225_600;
        assert_eq!(relative_to(march, november, 0), "3月5日");
        // 2025-12-31 23:00 UTC is already 2026-01-01 at UTC+8.
        let eve = 1_767_222_000;
        assert_eq!(relative_to(eve, november, 8 * 3600), "1月1日");
        assert_eq!(relative_to(eve, november, 0), "2025年12月31日");
    }
}
