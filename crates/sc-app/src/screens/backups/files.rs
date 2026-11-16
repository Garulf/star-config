//! Blocking work behind the Backups screen: comparing a snapshot with disk, and restoring.

use crate::theme::Tone;
use sc_core::backup::{BackupStore, Scope, SnapshotId, SnapshotInfo, SnapshotState};
use sc_core::diff::{self, DiffLine, FileStatus};
use sc_core::error::Error;
use sc_core::guard::Writer;
use sc_core::install::Channel;
use std::io::ErrorKind;
use std::path::Path;

pub use crate::screens::words::words;
use crate::screens::words::{ROLLBACK_FAILED, io_words};

pub const CLOSE_GAME: &str = "Close the game before restoring.";
const GONE: &str = "That snapshot is gone. It may have been deleted.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileState {
    Same,
    Different,
    OnlyInSnapshot,
    DidNotExistThen,
}

impl FileState {
    pub fn text(self) -> &'static str {
        match self {
            FileState::Same => "Same",
            FileState::Different => "Different",
            FileState::OnlyInSnapshot => "Only in snapshot",
            FileState::DidNotExistThen => "Did not exist then",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            FileState::Same => Tone::Success,
            FileState::Different => Tone::Accent,
            FileState::OnlyInSnapshot => Tone::Warning,
            FileState::DidNotExistThen => Tone::Muted,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRow {
    pub rel: String,
    pub state: FileState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffView {
    Lines(Vec<DiffLine>),
    NotText,
}

fn current(channel: &Channel, rel: &str) -> Result<Option<Vec<u8>>, String> {
    let path = channel.dir.join(rel);
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("Could not read {rel}: {}", io_words(&err))),
    }
}

fn saved(
    store: &BackupStore,
    scope: &Scope,
    id: &SnapshotId,
    rel: &str,
) -> Result<Option<Vec<u8>>, String> {
    store
        .read_file(scope, id, rel)
        .map_err(|err| format!("Could not read {rel} from the snapshot: {}", words(&err)))
}

fn state_of(existed: bool, saved: Option<&[u8]>, current: Option<&[u8]>) -> FileState {
    if !existed {
        return FileState::DidNotExistThen;
    }
    match diff::status(saved, current) {
        FileStatus::Same => FileState::Same,
        FileStatus::OnlyInSnapshot => FileState::OnlyInSnapshot,
        FileStatus::Different | FileStatus::OnlyOnDisk | FileStatus::BothAbsent => {
            FileState::Different
        }
    }
}

/// Each file of `info` with how it compares to the file on disk now.
pub fn rows(
    store: &BackupStore,
    scope: &Scope,
    channel: &Channel,
    info: &SnapshotInfo,
) -> Result<Vec<FileRow>, String> {
    info.files
        .iter()
        .map(|file| {
            let then = if file.existed {
                saved(store, scope, &info.id, &file.rel)?
            } else {
                None
            };
            let now = current(channel, &file.rel)?;
            Ok(FileRow {
                rel: file.rel.clone(),
                state: state_of(file.existed, then.as_deref(), now.as_deref()),
            })
        })
        .collect()
}

/// What restoring `rel` would change: lines only on disk now are removed, lines only in the
/// snapshot are added.
pub fn diff_view(
    store: &BackupStore,
    scope: &Scope,
    channel: &Channel,
    id: &SnapshotId,
    rel: &str,
) -> Result<DiffView, String> {
    let then = saved(store, scope, id, rel)?.unwrap_or_default();
    let now = current(channel, rel)?.unwrap_or_default();
    Ok(diff::lines(&now, &then).map_or(DiffView::NotText, DiffView::Lines))
}

/// Restores through the guarded writer and says in plain words what went wrong.
/// Returns the id of the snapshot taken just before the restore.
pub fn restore(
    writer: &Writer,
    scope: &Scope,
    channel: &Channel,
    id: &SnapshotId,
) -> Result<SnapshotId, String> {
    match writer.restore(scope, channel, id) {
        Ok(before) => Ok(before.id),
        Err(err) => Err(restore_failure(writer.store(), scope, channel, id, &err)),
    }
}

fn restore_failure(
    store: &BackupStore,
    scope: &Scope,
    channel: &Channel,
    id: &SnapshotId,
    err: &Error,
) -> String {
    match err {
        Error::GameRunning => CLOSE_GAME.to_string(),
        Error::NotFound(_) => GONE.to_string(),
        Error::RollbackFailed { .. } => ROLLBACK_FAILED.to_string(),
        _ if partly_restored(store, scope, id) => format!(
            "Could not restore {}. Other files may already be back. Your files from before the restore are in the snapshot marked Interrupted.",
            failed_file(err, channel)
        ),
        _ => format!("Nothing was restored ({}).", words(err)),
    }
}

/// A failed restore leaves its own "before" snapshot `Applying` once it started writing.
fn partly_restored(store: &BackupStore, scope: &Scope, id: &SnapshotId) -> bool {
    let before = format!("Before restore of {}", id.0);
    store.list(scope).is_ok_and(|list| {
        list.iter()
            .any(|info| info.state == SnapshotState::Applying && info.label == before)
    })
}

fn failed_file(err: &Error, channel: &Channel) -> String {
    match err {
        Error::Io { path, source } => format!("{} ({})", rel_name(channel, path), io_words(source)),
        other => words(other),
    }
}

fn rel_name(channel: &Channel, path: &Path) -> String {
    path.strip_prefix(&channel.dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_follow_the_core_status() {
        assert_eq!(state_of(true, Some(b"a"), Some(b"a")), FileState::Same);
        assert_eq!(state_of(true, Some(b"a"), Some(b"b")), FileState::Different);
        assert_eq!(state_of(true, Some(b"a"), None), FileState::OnlyInSnapshot);
        assert_eq!(
            state_of(false, None, Some(b"b")),
            FileState::DidNotExistThen
        );
        assert_eq!(state_of(false, None, None), FileState::DidNotExistThen);
    }
}
