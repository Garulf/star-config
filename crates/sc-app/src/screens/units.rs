//! Sizes and counts as every screen shows them. Dates live in `backups::when`.

const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];

/// `1.8 GB`, `96.4 GB`, `812 MB`, `512 bytes`: one decimal below a hundred, none above.
pub fn bytes(count: u64) -> String {
    if count < 1024 {
        return if count == 1 {
            "1 byte".to_string()
        } else {
            format!("{count} bytes")
        };
    }
    let mut value = count as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value < 100.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

/// `1 cache`, `2 caches`, `1,204 caches`.
pub fn count(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{} {many}", thousands(count))
    }
}

/// Groups digits in threes: `1204` becomes `1,204`.
pub fn thousands(count: usize) -> String {
    let digits = count.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_use_one_decimal_below_a_hundred() {
        assert_eq!(bytes(0), "0 bytes");
        assert_eq!(bytes(1), "1 byte");
        assert_eq!(bytes(2_516_582), "2.4 MB");
        assert_eq!(bytes(851_443_712), "812 MB");
        assert_eq!(bytes(1_932_735_283), "1.8 GB");
        assert_eq!(bytes(1024 * 1024), "1.0 MB");
        assert_eq!(bytes(103_514_210_304), "96.4 GB");
        assert_eq!(bytes(158_913_789_952), "148 GB");
    }

    #[test]
    fn counts_group_thousands_and_pick_the_word() {
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(1204), "1,204");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(count(1, "cache", "caches"), "1 cache");
        assert_eq!(count(0, "cache", "caches"), "0 caches");
        assert_eq!(count(1204, "cache", "caches"), "1,204 caches");
    }
}
