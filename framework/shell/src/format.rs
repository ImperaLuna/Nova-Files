const SIZE_UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
const SIZE_STEP: f64 = 1000.0;
const ONE_DECIMAL_BELOW: f64 = 10.0;

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

#[cfg(test)]
mod tests {
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
}
