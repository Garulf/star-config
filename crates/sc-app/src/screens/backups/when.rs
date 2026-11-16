//! How snapshot times and labels read in the UI, in local time.

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime};

const RESTORE_PREFIX: &str = "Before restore of ";

pub(crate) fn local(unix: i64) -> NaiveDateTime {
    DateTime::from_timestamp(unix, 0)
        .map(|utc| utc.with_timezone(&Local).naive_local())
        .unwrap_or_default()
}

pub(crate) fn now() -> NaiveDateTime {
    Local::now().naive_local()
}

/// `Today 21:14`, `Yesterday 08:30`, `14 Nov 19:02`, or with the year when it is not this one.
pub(crate) fn list_label(at: NaiveDateTime, now: NaiveDateTime) -> String {
    let time = at.format("%H:%M");
    if at.date() == now.date() {
        format!("Today {time}")
    } else if now.date().pred_opt() == Some(at.date()) {
        format!("Yesterday {time}")
    } else {
        short(at, now)
    }
}

/// `14 Nov 19:02`, with the year when it is not this one. Reads well after "on".
pub(crate) fn short(at: NaiveDateTime, now: NaiveDateTime) -> String {
    if at.year() == now.year() {
        at.format("%-d %b %H:%M").to_string()
    } else {
        at.format("%-d %b %Y %H:%M").to_string()
    }
}

/// `14 November 2026, 19:02`.
pub(crate) fn full(at: NaiveDateTime) -> String {
    at.format("%-d %B %Y, %H:%M").to_string()
}

/// `Today`, `Yesterday`, or the whole date as `Saturday 14 November 2026`.
pub(crate) fn day_heading(day: NaiveDate, today: NaiveDate) -> String {
    if day == today {
        "Today".to_string()
    } else if today.pred_opt() == Some(day) {
        "Yesterday".to_string()
    } else {
        day.format("%A %-d %B %Y").to_string()
    }
}

/// `21:14`.
pub(crate) fn clock(at: NaiveDateTime) -> String {
    at.format("%H:%M").to_string()
}

/// The UTC time a snapshot id was stamped with, as local time.
fn id_time(id: &str) -> Option<NaiveDateTime> {
    let stamp = id.get(..16)?;
    let utc = NaiveDateTime::parse_from_str(stamp, "%Y%m%dT%H%M%SZ").ok()?;
    Some(utc.and_utc().with_timezone(&Local).naive_local())
}

/// The label as the user reads it: the core's `Before restore of <id>` names the time instead.
pub(crate) fn label(label: &str, now: NaiveDateTime) -> String {
    let named = label
        .strip_prefix(RESTORE_PREFIX)
        .and_then(id_time)
        .map(|at| format!("Before restoring {}", short(at, now)));
    named.unwrap_or_else(|| label.to_string())
}

pub(crate) fn files(count: usize) -> String {
    if count == 1 {
        "1 file".to_string()
    } else {
        format!("{count} files")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, day)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
    }

    #[test]
    fn list_label_says_today_yesterday_or_the_date() {
        let now = at(15, 22, 0);
        assert_eq!(list_label(at(15, 21, 14), now), "Today 21:14");
        assert_eq!(list_label(at(14, 8, 30), now), "Yesterday 08:30");
        assert_eq!(list_label(at(3, 19, 2), now), "3 Nov 19:02");
    }

    #[test]
    fn other_years_show_the_year() {
        let last_year = NaiveDate::from_ymd_opt(2025, 12, 30)
            .unwrap()
            .and_hms_opt(9, 5, 0)
            .unwrap();
        assert_eq!(list_label(last_year, at(15, 22, 0)), "30 Dec 2025 09:05");
    }

    #[test]
    fn full_spells_out_the_month() {
        assert_eq!(full(at(14, 19, 2)), "14 November 2026, 19:02");
    }

    #[test]
    fn day_headings_say_today_yesterday_or_the_whole_date() {
        let today = at(15, 22, 0).date();
        assert_eq!(day_heading(at(15, 1, 0).date(), today), "Today");
        assert_eq!(day_heading(at(14, 1, 0).date(), today), "Yesterday");
        assert_eq!(
            day_heading(at(13, 1, 0).date(), today),
            "Friday 13 November 2026"
        );
        assert_eq!(clock(at(3, 9, 5)), "09:05");
    }

    #[test]
    fn restore_labels_name_the_time_instead_of_the_id() {
        let now = at(15, 22, 0);
        let utc = at(14, 19, 2).and_utc().with_timezone(&Local).naive_local();
        assert_eq!(
            label("Before restore of 20261114T190200Z-2", now),
            format!("Before restoring {}", short(utc, now))
        );
        assert_eq!(label("Config changes", now), "Config changes");
        assert_eq!(
            label("Before restore of nonsense", now),
            "Before restore of nonsense"
        );
    }

    #[test]
    fn file_counts_are_singular_for_one() {
        assert_eq!(files(1), "1 file");
        assert_eq!(files(3), "3 files");
    }
}
