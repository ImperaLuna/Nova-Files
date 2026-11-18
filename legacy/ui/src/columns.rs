use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColumnText {
    pub modified: String,
    pub size: String,
}

const SIZE_UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
const SIZE_STEP: f64 = 1000.0;
const ONE_DECIMAL_BELOW: f64 = 10.0;
const DATE_FORMAT: &str = "%Y-%m-%d %H:%M";

pub fn size(bytes: u64) -> String {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a shown size keeps at most two significant digits"
    )]
    let mut value = bytes as f64;
    if value < SIZE_STEP {
        return format!("{bytes} B");
    }
    let mut unit = SIZE_UNITS[0];
    for next in SIZE_UNITS {
        value /= SIZE_STEP;
        unit = next;
        if value < SIZE_STEP {
            break;
        }
    }
    if value < ONE_DECIMAL_BELOW {
        format!("{value:.1} {unit}")
    } else {
        format!("{value:.0} {unit}")
    }
}

pub fn modified(time: SystemTime) -> String {
    let since_epoch = match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).ok(),
        Err(before) => {
            let before = before.duration();
            let secs = before.as_secs() + u64::from(before.subsec_nanos() > 0);
            i64::try_from(secs).ok().map(|secs| -secs)
        }
    };
    since_epoch
        .and_then(|secs| DateTime::from_timestamp(secs, 0))
        .map(|time| time.with_timezone(&Local).format(DATE_FORMAT).to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn sizes_use_decimal_units() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(999), "999 B");
        assert_eq!(size(1000), "1.0 KB");
        assert_eq!(size(10_000), "10 KB");
        assert_eq!(size(1_000_000), "1.0 MB");
        assert_eq!(size(640_000), "640 KB");
        assert_eq!(size(1_800_000), "1.8 MB");
        assert_eq!(size(999_999_999), "1000 MB");
        assert_eq!(size(12_000_000_000), "12 GB");
    }

    #[test]
    fn times_out_of_range_show_nothing() {
        let far = Duration::from_secs(u64::MAX / 2);
        assert_eq!(modified(UNIX_EPOCH.checked_add(far).unwrap()), "");
        assert_eq!(modified(UNIX_EPOCH.checked_sub(far).unwrap()), "");
        assert_ne!(modified(UNIX_EPOCH - Duration::from_secs(1)), "");
        assert_ne!(modified(SystemTime::now()), "");
    }

    #[test]
    fn times_before_the_epoch_round_away_from_it() {
        let before = |secs: f64| modified(UNIX_EPOCH - Duration::from_secs_f64(secs));
        assert_eq!(before(60.5), before(61.0));
        assert_ne!(before(60.5), before(60.0));
    }
}
