//! What the Channels screen says, kept apart so the wording can be read and tested in one place.

use super::super::attr_edit::thousands;
use super::super::units;
use sc_core::channels::{CopyOutcome, CopyPlan, Part, SwapJournal};

pub const SUBTITLE: &str = "LIVE, PTU and the other game channels in this install.";
pub const GAME_CAPTION: &str = "The game itself: Data.p4k and the program files. Your settings and screenshots are not touched.";
pub const SETTINGS_CAPTION: &str = "Copy or exchange your own files between two channels.";
pub const CALCULATING: &str = "Calculating...";
pub const PLANNING: &str = "Working out what needs copying...";
pub const COMPARE_CONTENTS: &str = "Compare contents instead of size and date (slow)";
pub const IN_PLACE: &str = "There is not enough room to copy safely. Overwrite in place instead? If this is cancelled part-way, the target needs Verify in the launcher.";
pub const NOT_COPIED: &str = "Logs and login data are not copied.";
pub const SWAP_TRAVEL: &str = "Move settings and screenshots along with the game files";
pub const SWAPPING: &str = "Swapping...";
pub const SWAP_UNDONE: &str = "The swap failed and was undone. Nothing changed.";
pub const SWAP_STOPPED: &str =
    "The swap stopped part-way. Restart star-config to finish or undo it.";
pub const CANNOT_TELL: &str = "star-config cannot tell how far the swap got, so it will not touch these folders. Check them by hand.";
pub const STARSTRINGS_NOTE: &str = "Built for LIVE. Copy it only if you know the builds match.";
pub const PENDING_TITLE: &str = "A channel swap did not finish";
pub const SAME_CHANNEL: &str = "From and To are the same channel. Pick two different ones.";
pub const SWAP_NEEDS_BOTH: &str = "Swapping needs game files in both channels.";
pub const SETTINGS_BLOCKED: &str = "Close the game to copy or swap settings.";
pub const PENDING_BLOCKS: &str =
    "Finish or undo the unfinished swap before you copy or swap game files.";
pub const STUCK_BLOCKS: &str = "Game file actions stay off until the unfinished swap is sorted out. Put the folders back by hand, then restart star-config.";
pub const RESUME_BLOCKED: &str = "Close the game and the launcher to finish or undo the swap.";
pub const RESUMING: &str = "Working on the swap...";
pub const NO_PARTS: &str = "Tick at least one part.";
pub const STOPPING: &str = "Stopping after the current file...";

pub const PARTS: [Part; 5] = [
    Part::UserCfg,
    Part::Keybinds,
    Part::Layouts,
    Part::GameSettings,
    Part::StarStrings,
];

pub fn part_label(part: Part) -> &'static str {
    match part {
        Part::UserCfg => "user.cfg",
        Part::Keybinds => "Keybinds",
        Part::Layouts => "Exported layouts",
        Part::GameSettings => "Game settings",
        Part::StarStrings => "StarStrings",
    }
}

pub fn count(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", thousands(n), if n == 1 { one } else { many })
}

pub fn parts_text(n: usize) -> String {
    count(n, "part", "parts")
}

/// `Has settings and 214 screenshots`, `No settings yet`.
pub fn user_files_line(has_settings: bool, screenshots: usize) -> String {
    let shots = count(screenshots, "screenshot", "screenshots");
    match (has_settings, screenshots) {
        (true, 0) => "Has settings".to_string(),
        (true, _) => format!("Has settings and {shots}"),
        (false, 0) => "No settings yet".to_string(),
        (false, _) => format!("No settings yet, {shots}"),
    }
}

pub fn blocked_text(game: bool, launcher: bool) -> Option<&'static str> {
    match (game, launcher) {
        (true, true) => Some("Close the game and the launcher to copy or swap game files."),
        (true, false) => Some("Close the game to copy or swap game files."),
        (false, true) => Some("Close the launcher to copy or swap game files."),
        (false, false) => None,
    }
}

/// `1,204 files, 96 GB to copy. 312 files already match and are skipped. 148 GB free on this disk.`
pub fn plan_summary(plan: &CopyPlan) -> String {
    let copying = plan.files.iter().filter(|file| file.copy).count();
    let matching = plan.files.len() - copying;
    let mut text = if copying == 0 {
        format!(
            "Nothing to copy. {} already {}.",
            count(matching, "file", "files"),
            if matching == 1 { "matches" } else { "match" }
        )
    } else {
        format!(
            "{}, {} to copy.",
            count(copying, "file", "files"),
            units::bytes(plan.bytes_to_copy)
        )
    };
    if copying > 0 && matching > 0 {
        text.push_str(&format!(
            " {} already {} and {} skipped.",
            count(matching, "file", "files"),
            if matching == 1 { "matches" } else { "match" },
            if matching == 1 { "is" } else { "are" }
        ));
    }
    if let Some(free) = plan.free_bytes {
        text.push_str(&format!(" {} free on this disk.", units::bytes(free)));
    }
    text
}

pub fn progress_caption(done: u64, total: u64) -> String {
    format!("{} of {}", units::bytes(done), units::bytes(total))
}

pub fn copy_finished(target: &str) -> String {
    format!("Copy finished. Open the launcher and run Verify on {target} before you play.")
}

pub fn copy_stopped(target: &str, outcome: &CopyOutcome, total: usize, in_place: bool) -> String {
    if outcome.files_copied == 0 && !in_place {
        return format!("Copy stopped. {target} was not changed.");
    }
    let done = format!(
        "Copy stopped after {} of {}.",
        thousands(outcome.files_copied),
        count(total, "file", "files")
    );
    if in_place {
        format!(
            "{done} The file being copied may be part-written, so {target} needs Verify in the launcher before you play."
        )
    } else {
        format!(
            "{done} Every copied file is complete, but {target} now mixes two builds. Run Verify on {target} in the launcher before you play."
        )
    }
}

pub fn swap_finished(a: &str, b: &str) -> String {
    format!("Swap finished. Open the launcher and run Verify on {a} and {b} before you play.")
}

pub fn swap_undone(a: &str, b: &str) -> String {
    format!("The swap was undone. {a} and {b} are back as they were.")
}

pub fn swap_title(a: &str, b: &str) -> String {
    format!("Swap {a} and {b}?")
}

/// What a swap does, in order, from its plan.
pub fn swap_steps(journal: &SwapJournal, travel: bool) -> Vec<String> {
    let (a, b) = (&journal.a, &journal.b);
    let mut steps = vec![
        format!("{a}'s folder is set aside under a temporary name."),
        format!("{b}'s folder is renamed to {a}."),
        format!("The set-aside folder is renamed to {b}."),
    ];
    let moves = journal.steps.len().saturating_sub(3);
    steps.push(match (travel, moves) {
        (true, _) => "Settings and screenshots stay in the folders they are in, so they go with the game files.".to_string(),
        (false, 0) => "Neither channel has settings to move back.".to_string(),
        (false, _) => format!(
            "Settings and screenshots move back to their own channel ({}).",
            count(moves, "move", "moves")
        ),
    });
    steps
}

pub fn swap_summary(a: &str, b: &str, travel: bool) -> String {
    let settings = if travel {
        "Your settings and screenshots move with the game files."
    } else {
        "Your settings stay with their channel."
    };
    format!(
        "{a}'s game files become {b}'s and {b}'s become {a}'s. Nothing is copied or deleted. {settings}"
    )
}

/// `star-config was swapping LIVE and PTU and stopped after step 2 of 5.`
pub fn pending_body(journal: &SwapJournal) -> String {
    let total = journal.steps.len();
    if journal.done == 0 {
        format!(
            "star-config was swapping {} and {} and stopped before the first of {total} steps.",
            journal.a, journal.b
        )
    } else {
        format!(
            "star-config was swapping {} and {} and stopped after step {} of {total}.",
            journal.a, journal.b, journal.done
        )
    }
}

pub fn copied_settings(n: usize, from: &str, to: &str) -> String {
    format!("Copied {} from {from} to {to}", parts_text(n))
}

pub fn swapped_settings(n: usize, a: &str, b: &str) -> String {
    format!("Swapped {} between {a} and {b}", parts_text(n))
}

pub fn part_count(n: usize, channel: &str) -> String {
    if n == 0 {
        format!("None in {channel}")
    } else {
        count(n, "file", "files")
    }
}

pub fn cannot_rename(path: &str) -> String {
    format!("star-config cannot rename folders in {path}. Check the folder's permissions.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::channels::{PlannedFile, Step};
    use std::path::PathBuf;

    fn plan(copy: usize, skip: usize, bytes: u64, free: Option<u64>) -> CopyPlan {
        let file = |copy| PlannedFile {
            rel: PathBuf::from("Bin64/x.dll"),
            size: 1,
            copy,
        };
        CopyPlan {
            source: PathBuf::from("/a"),
            target: PathBuf::from("/b"),
            files: (0..copy)
                .map(|_| file(true))
                .chain((0..skip).map(|_| file(false)))
                .collect(),
            bytes_to_copy: bytes,
            largest_copy: bytes,
            free_bytes: free,
            needs_in_place: false,
        }
    }

    #[test]
    fn plan_summary_reads_like_the_brief() {
        let gb = 1024 * 1024 * 1024;
        assert_eq!(
            plan_summary(&plan(1204, 312, 96 * gb, Some(148 * gb))),
            "1,204 files, 96 GB to copy. 312 files already match and are skipped. 148 GB free on this disk."
        );
        assert_eq!(
            plan_summary(&plan(1, 1, 2048, None)),
            "1 file, 2.0 KB to copy. 1 file already matches and is skipped."
        );
        assert_eq!(
            plan_summary(&plan(0, 3, 0, None)),
            "Nothing to copy. 3 files already match."
        );
    }

    #[test]
    fn user_files_line_covers_every_case() {
        assert_eq!(
            user_files_line(true, 214),
            "Has settings and 214 screenshots"
        );
        assert_eq!(user_files_line(true, 0), "Has settings");
        assert_eq!(user_files_line(false, 0), "No settings yet");
        assert_eq!(user_files_line(false, 1), "No settings yet, 1 screenshot");
    }

    #[test]
    fn blocked_text_names_what_runs() {
        assert_eq!(
            blocked_text(false, true),
            Some("Close the launcher to copy or swap game files.")
        );
        assert_eq!(blocked_text(false, false), None);
    }

    #[test]
    fn pending_body_counts_steps() {
        let step = Step::Rename {
            from: PathBuf::from("/r/a"),
            to: PathBuf::from("/r/b"),
        };
        let journal = SwapJournal {
            install_root: PathBuf::from("/r"),
            a: "LIVE".into(),
            b: "PTU".into(),
            steps: vec![step; 5],
            done: 2,
            ids: Vec::new(),
        };
        assert_eq!(
            pending_body(&journal),
            "star-config was swapping LIVE and PTU and stopped after step 2 of 5."
        );
    }

    #[test]
    fn stopped_copy_says_what_state_the_target_is_in() {
        let none = CopyOutcome {
            files_copied: 0,
            bytes_copied: 0,
            cancelled: true,
        };
        assert_eq!(
            copy_stopped("PTU", &none, 4, false),
            "Copy stopped. PTU was not changed."
        );
        let some = CopyOutcome {
            files_copied: 2,
            ..none
        };
        assert!(
            copy_stopped("PTU", &some, 4, false).starts_with("Copy stopped after 2 of 4 files.")
        );
        assert!(copy_stopped("PTU", &some, 4, true).contains("needs Verify"));
    }
}
