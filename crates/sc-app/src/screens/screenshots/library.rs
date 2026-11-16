//! Which screenshots the grid shows, how they group into days, and how they are named.

use crate::screens::backups::when;
use crate::screens::units;
use chrono::{NaiveDate, NaiveDateTime};
use sc_core::screenshots::Shot;
use std::fmt;
use std::path::Path;

pub const ARCHIVE: &str = "Archive";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Source {
    #[default]
    All,
    Channel(String),
    Archive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Period {
    #[default]
    AllTime,
    Today,
    Week,
    Month,
}

impl Period {
    pub const ALL: [Period; 4] = [Period::AllTime, Period::Today, Period::Week, Period::Month];

    fn days(self) -> Option<i64> {
        match self {
            Period::AllTime => None,
            Period::Today => Some(1),
            Period::Week => Some(7),
            Period::Month => Some(30),
        }
    }
}

impl fmt::Display for Period {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Period::AllTime => "All time",
            Period::Today => "Today",
            Period::Week => "Last 7 days",
            Period::Month => "Last 30 days",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Filter {
    pub source: Source,
    pub favourites: bool,
    pub period: Period,
}

impl Filter {
    pub fn keeps(&self, shot: &Shot, favourite: bool, taken: NaiveDate, today: NaiveDate) -> bool {
        let source = match &self.source {
            Source::All => true,
            Source::Channel(name) => shot.channel.as_deref() == Some(name.as_str()),
            Source::Archive => shot.channel.is_none(),
        };
        let recent = self
            .period
            .days()
            .is_none_or(|days| (today - taken).num_days() < days);
        source && recent && (favourite || !self.favourites)
    }
}

/// One day of the grid: its heading and the positions of its shots in the shown list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Day {
    pub heading: String,
    pub len: usize,
}

/// Groups `dates` (newest first) into consecutive days.
pub fn days(dates: &[NaiveDate], today: NaiveDate) -> Vec<Day> {
    let mut out: Vec<(NaiveDate, usize)> = Vec::new();
    for date in dates {
        match out.last_mut() {
            Some((day, len)) if day == date => *len += 1,
            _ => out.push((*date, 1)),
        }
    }
    out.into_iter()
        .map(|(date, len)| Day {
            heading: when::day_heading(date, today),
            len,
        })
        .collect()
}

pub fn taken(shot: &Shot) -> NaiveDateTime {
    when::local(shot.modified_unix)
}

/// `214 screenshots, 1.9 GB`.
pub fn count_line(count: usize, bytes: u64) -> String {
    format!("{}, {}", screenshots(count), units::bytes(bytes))
}

/// `1 screenshot`, `2,140 screenshots`.
pub fn screenshots(count: usize) -> String {
    units::count(count, "screenshot", "screenshots")
}

pub fn channel_label(shot: &Shot) -> &str {
    shot.channel.as_deref().unwrap_or(ARCHIVE)
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `.jpg`, or nothing for a file without an extension.
pub fn extension(path: &Path) -> String {
    path.extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_default()
}

/// Turns a core error message into a sentence: capital first letter, full stop at the end.
pub fn sentence(message: &str) -> String {
    let mut chars = message.trim().chars();
    let mut out: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => return String::new(),
    };
    if !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::screenshots::ShotKind;
    use std::path::PathBuf;

    fn shot(channel: Option<&str>) -> Shot {
        Shot {
            path: PathBuf::from("/shots/a.jpg"),
            channel: channel.map(str::to_string),
            size: 10,
            modified_unix: 0,
            kind: ShotKind::Image,
        }
    }

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, day).unwrap()
    }

    #[test]
    fn filter_checks_source_period_and_favourites() {
        let today = date(15);
        let live = shot(Some("LIVE"));
        let archived = shot(None);
        let ptu_only = Filter {
            source: Source::Channel("PTU".into()),
            ..Filter::default()
        };
        assert!(Filter::default().keeps(&live, false, date(1), today));
        assert!(!ptu_only.keeps(&live, false, today, today));
        let archive = Filter {
            source: Source::Archive,
            ..Filter::default()
        };
        assert!(archive.keeps(&archived, false, today, today));
        assert!(!archive.keeps(&live, false, today, today));
        let week = Filter {
            period: Period::Week,
            ..Filter::default()
        };
        assert!(week.keeps(&live, false, date(9), today));
        assert!(!week.keeps(&live, false, date(8), today));
        let today_only = Filter {
            period: Period::Today,
            ..Filter::default()
        };
        assert!(!today_only.keeps(&live, false, date(14), today));
        let favourites = Filter {
            favourites: true,
            ..Filter::default()
        };
        assert!(favourites.keeps(&live, true, today, today));
        assert!(!favourites.keeps(&live, false, today, today));
    }

    #[test]
    fn days_group_consecutive_dates() {
        let today = date(15);
        let groups = days(&[date(15), date(15), date(14), date(3)], today);
        let summary: Vec<_> = groups.iter().map(|d| (d.heading.as_str(), d.len)).collect();
        assert_eq!(
            summary,
            [
                ("Today", 2),
                ("Yesterday", 1),
                ("Tuesday 3 November 2026", 1)
            ]
        );
    }

    #[test]
    fn counts_read_naturally() {
        assert_eq!(count_line(214, 2_040_109_465), "214 screenshots, 1.9 GB");
        assert_eq!(screenshots(1), "1 screenshot");
        assert_eq!(screenshots(2000), "2,000 screenshots");
        assert_eq!(screenshots(1_234_567), "1,234,567 screenshots");
        assert_eq!(screenshots(999), "999 screenshots");
    }

    #[test]
    fn names_and_sentences() {
        let path = Path::new("/shots/ScreenShot-2026-11-14_19-02-11.jpg");
        assert_eq!(file_stem(path), "ScreenShot-2026-11-14_19-02-11");
        assert_eq!(extension(path), ".jpg");
        assert_eq!(extension(Path::new("/shots/notes")), "");
        assert_eq!(
            sentence("a file with that name already exists"),
            "A file with that name already exists."
        );
        assert_eq!(sentence("Done."), "Done.");
        assert_eq!(channel_label(&shot(None)), "Archive");
    }
}
