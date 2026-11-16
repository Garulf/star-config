//! Sizes and dates as the StarStrings and Maintenance screens show them.

const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const SECONDS_PER_DAY: i64 = 86_400;

/// `1.8 GB`, `812 MB`, `512 bytes`: one decimal below ten, none above.
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
    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
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

/// The UTC calendar date of a Unix time, as `12 Nov 2026`.
pub fn date_of_unix(seconds: i64) -> String {
    let (year, month, day) = civil_from_days(seconds.div_euclid(SECONDS_PER_DAY));
    format!("{day} {} {year}", MONTHS[month as usize - 1])
}

/// The date of an ISO 8601 timestamp such as `2026-11-12T17:31:51Z`, or the text itself
/// when it does not start with a date.
pub fn date_of_iso(timestamp: &str) -> String {
    parse_iso_date(timestamp)
        .map(|(year, month, day)| format!("{day} {} {year}", MONTHS[month as usize - 1]))
        .unwrap_or_else(|| timestamp.to_string())
}

fn parse_iso_date(timestamp: &str) -> Option<(i64, u32, u32)> {
    let date = timestamp.get(..10)?;
    let mut parts = date.split('-');
    let year = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    ((1..=12).contains(&month) && (1..=31).contains(&day)).then_some((year, month, day))
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_use_one_decimal_below_ten() {
        assert_eq!(bytes(0), "0 bytes");
        assert_eq!(bytes(1), "1 byte");
        assert_eq!(bytes(2_516_582), "2.4 MB");
        assert_eq!(bytes(851_443_712), "812 MB");
        assert_eq!(bytes(1_932_735_283), "1.8 GB");
        assert_eq!(bytes(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn unix_times_become_dates() {
        assert_eq!(date_of_unix(0), "1 Jan 1970");
        assert_eq!(date_of_unix(1_794_960_000), "18 Nov 2026");
        assert_eq!(date_of_unix(951_782_400), "29 Feb 2000");
    }

    #[test]
    fn iso_timestamps_become_dates() {
        assert_eq!(date_of_iso("2026-11-12T17:31:51Z"), "12 Nov 2026");
        assert_eq!(date_of_iso("soon"), "soon");
    }
}
