use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local};
pub use nova_files_shell::format::size;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColumnText {
    pub modified: String,
    pub size: String,
}

const DATE_FORMAT: &str = "%Y-%m-%d %H:%M";

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
