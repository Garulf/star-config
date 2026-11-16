//! Core errors in words for the user: file names instead of paths, and no OS error codes.
//! Every screen words its errors through here so the same failure reads the same way.

use sc_core::error::Error;
use std::io::ErrorKind;
use std::path::Path;

pub const GAME_RUNNING: &str =
    "Star Citizen is running, so nothing was written. Your edits were kept.";
pub const ROLLBACK_FAILED: &str = "Writing failed, and putting the earlier files back failed too. \
    Some files may have changed. The snapshot on the Backups screen holds the originals.";

/// A core error in words for the user.
pub fn words(err: &Error) -> String {
    match err {
        Error::Io { path, source } => format!("{}: {}", name(path), io_words(source)),
        Error::ChangedOnDisk { path } => {
            format!("{} changed on disk since it was loaded", name(path))
        }
        Error::Parse {
            path,
            line,
            message,
        } => format!("{}, line {line}: {message}", name(path)),
        Error::AmbiguousCase { first, second } => {
            format!("{} and {} differ only by case", name(first), name(second))
        }
        Error::GameRunning => "Star Citizen is running".to_string(),
        Error::RollbackFailed { .. } => ROLLBACK_FAILED.to_string(),
        other => without_code(&other.to_string()),
    }
}

/// What went wrong with a file, in a few words.
pub fn io_words(err: &std::io::Error) -> String {
    match err.kind() {
        ErrorKind::PermissionDenied => "permission denied".to_string(),
        ErrorKind::NotFound => "it is missing".to_string(),
        ErrorKind::StorageFull => "the disk is full".to_string(),
        ErrorKind::ReadOnlyFilesystem => "the disk is read-only".to_string(),
        _ => without_code(&err.to_string()),
    }
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "a file".to_string())
}

// Windows ends its messages with a full stop, and callers add their own.
fn without_code(text: &str) -> String {
    let message = match text.find(" (os error ") {
        Some(at) => &text[..at],
        None => text,
    };
    message.trim_end().trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn io_errors_name_the_file_without_its_folder_or_code() {
        let err = Error::Io {
            path: PathBuf::from("/home/someone/Games/StarCitizen/LIVE/user.cfg"),
            source: std::io::Error::from_raw_os_error(21),
        };
        let text = words(&err);
        assert!(text.starts_with("user.cfg: "), "{text}");
        assert!(!text.contains("os error"), "{text}");
        assert!(!text.contains("/home"), "{text}");
    }

    #[test]
    fn windows_style_messages_lose_their_code_and_full_stop() {
        let err = std::io::Error::other("The data is invalid. (os error 13)");
        assert_eq!(io_words(&err), "The data is invalid");
    }

    #[test]
    fn other_errors_keep_their_file_names_only() {
        let err = Error::ChangedOnDisk {
            path: PathBuf::from("/games/LIVE/user.cfg"),
        };
        assert_eq!(words(&err), "user.cfg changed on disk since it was loaded");
        let err = Error::Parse {
            path: PathBuf::from("/games/LIVE/user.cfg"),
            line: 3,
            message: "this is not a setting".into(),
        };
        assert_eq!(words(&err), "user.cfg, line 3: this is not a setting");
    }
}
