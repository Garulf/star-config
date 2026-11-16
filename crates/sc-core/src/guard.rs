//! The single guarded write path for files inside a game folder.

use crate::backup::{BackupStore, Scope, SnapshotId, SnapshotInfo, SnapshotState, relative_to};
use crate::error::{Error, Result};
use crate::install::{Channel, case_conflict};
use crate::process::{GameProbe, RunState};
use crate::safe_write::{self, LoadedFile};
use std::path::PathBuf;
use std::sync::Arc;

pub struct Writer {
    store: BackupStore,
    probe: Arc<dyn GameProbe>,
    unlocked: bool,
}

pub struct Change<'a> {
    pub loaded: &'a LoadedFile,
    pub new_bytes: Option<&'a [u8]>,
}

/// One channel's share of an apply that spans several channels.
pub(crate) struct Target<'a> {
    pub scope: &'a Scope,
    pub channel: &'a Channel,
    pub changes: &'a [Change<'a>],
    pub label: &'a str,
}

trait Snapshots {
    fn begin(
        &self,
        scope: &Scope,
        channel: &Channel,
        files: &[PathBuf],
        label: &str,
    ) -> Result<SnapshotInfo>;
    fn finish(&self, scope: &Scope, id: &SnapshotId) -> Result<()>;
    fn roll_back(&self, scope: &Scope, channel: &Channel, id: &SnapshotId) -> Result<()>;
    fn delete(&self, scope: &Scope, id: &SnapshotId) -> Result<()>;
}

impl Snapshots for BackupStore {
    fn begin(
        &self,
        scope: &Scope,
        channel: &Channel,
        files: &[PathBuf],
        label: &str,
    ) -> Result<SnapshotInfo> {
        BackupStore::begin(self, scope, channel, files, label)
    }

    fn finish(&self, scope: &Scope, id: &SnapshotId) -> Result<()> {
        BackupStore::finish(self, scope, id)
    }

    fn roll_back(&self, scope: &Scope, channel: &Channel, id: &SnapshotId) -> Result<()> {
        BackupStore::roll_back(self, scope, channel, id)
    }

    fn delete(&self, scope: &Scope, id: &SnapshotId) -> Result<()> {
        BackupStore::delete(self, scope, id)
    }
}

impl Writer {
    pub fn new(store: BackupStore, probe: Arc<dyn GameProbe>, unlocked: bool) -> Self {
        Self {
            store,
            probe,
            unlocked,
        }
    }

    pub fn store(&self) -> &BackupStore {
        &self.store
    }

    pub fn blocked(&self) -> bool {
        self.probe.game() != RunState::NotRunning && !self.unlocked
    }

    /// Writes `changes` after a snapshot. If every write succeeded but the snapshot could not be
    /// marked done, the files are written and the returned snapshot is still `Applying`.
    pub fn apply(
        &self,
        scope: &Scope,
        channel: &Channel,
        changes: &[Change],
        label: &str,
    ) -> Result<SnapshotInfo> {
        let target = Target {
            scope,
            channel,
            changes,
            label,
        };
        let mut done = self.apply_all(&[target])?;
        Ok(done.remove(0))
    }

    /// Restores a snapshot, refusing while the game runs.
    pub fn restore(
        &self,
        scope: &Scope,
        channel: &Channel,
        id: &SnapshotId,
    ) -> Result<SnapshotInfo> {
        if self.blocked() {
            return Err(Error::GameRunning);
        }
        self.store.restore(scope, channel, id)
    }

    /// Like `apply`, for several channels at once: every snapshot is begun before the first write
    /// and finished after the last, so a crash in between leaves all of them `Applying`.
    pub(crate) fn apply_all(&self, targets: &[Target]) -> Result<Vec<SnapshotInfo>> {
        self.apply_with(&self.store, targets)
    }

    /// Puts back the files of a finished apply after a later step of the same operation failed.
    pub(crate) fn undo_apply(
        &self,
        scope: &Scope,
        channel: &Channel,
        snapshot: &SnapshotId,
        cause: Error,
    ) -> Error {
        match self.store.roll_back(scope, channel, snapshot) {
            Ok(()) => cause,
            Err(restore) => rollback_failed(cause, restore, snapshot),
        }
    }

    fn apply_with(&self, store: &dyn Snapshots, targets: &[Target]) -> Result<Vec<SnapshotInfo>> {
        if self.blocked() {
            return Err(Error::GameRunning);
        }
        for target in targets {
            check_cases(target.channel, target.changes)?;
            check_unchanged(target.changes)?;
        }
        let mut begun: Vec<SnapshotInfo> = Vec::new();
        for target in targets {
            let paths: Vec<PathBuf> = target
                .changes
                .iter()
                .map(|change| change.loaded.path.clone())
                .collect();
            match store.begin(target.scope, target.channel, &paths, target.label) {
                Ok(info) => begun.push(info),
                Err(err) => {
                    for (earlier, info) in targets.iter().zip(&begun) {
                        let _ = store.delete(earlier.scope, &info.id);
                    }
                    return Err(err);
                }
            }
        }
        for target in targets {
            if let Err(err) = write_all(target.changes) {
                return Err(roll_back_all(store, targets, &begun, err));
            }
        }
        for (target, info) in targets.iter().zip(begun.iter_mut()) {
            if store.finish(target.scope, &info.id).is_ok() {
                info.state = SnapshotState::Done;
            }
        }
        Ok(begun)
    }
}

fn rollback_failed(write: Error, restore: Error, snapshot: &SnapshotId) -> Error {
    Error::RollbackFailed {
        write: Box::new(write),
        restore: Box::new(restore),
        snapshot: snapshot.clone(),
    }
}

fn roll_back_all(
    store: &dyn Snapshots,
    targets: &[Target],
    begun: &[SnapshotInfo],
    write: Error,
) -> Error {
    let mut failed = None;
    for (target, info) in targets.iter().zip(begun).rev() {
        if let Err(restore) = store.roll_back(target.scope, target.channel, &info.id) {
            failed = Some((restore, info.id.clone()));
        }
    }
    match failed {
        Some((restore, id)) => rollback_failed(write, restore, &id),
        None => write,
    }
}

fn check_cases(channel: &Channel, changes: &[Change]) -> Result<()> {
    for change in changes {
        let rel = relative_to(channel, &change.loaded.path)?;
        if let Some((first, second)) = case_conflict(&channel.dir, &rel) {
            return Err(Error::AmbiguousCase { first, second });
        }
    }
    Ok(())
}

fn check_unchanged(changes: &[Change]) -> Result<()> {
    for change in changes {
        let now = safe_write::load(&change.loaded.path)?;
        if now.hash != change.loaded.hash {
            return Err(Error::ChangedOnDisk {
                path: change.loaded.path.clone(),
            });
        }
    }
    Ok(())
}

fn write_all(changes: &[Change]) -> Result<()> {
    for change in changes {
        match change.new_bytes {
            Some(bytes) => safe_write::write_atomic(&change.loaded.path, bytes)?,
            None => safe_write::remove(&change.loaded.path)?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{FixedProbe, RunState};
    use crate::safe_write::{load, write_atomic};
    use std::fs;
    use tempfile::{TempDir, tempdir};

    struct Fixture {
        _root: TempDir,
        channel: Channel,
        scope: Scope,
    }

    fn fixture() -> Fixture {
        let root = tempdir().unwrap();
        let channel = Channel {
            name: "LIVE".into(),
            dir: root.path().join("LIVE"),
        };
        fs::create_dir_all(&channel.dir).unwrap();
        Fixture {
            channel,
            scope: Scope {
                install_key: "k".into(),
                channel: "LIVE".into(),
            },
            _root: root,
        }
    }

    impl Fixture {
        fn writer(&self, game: RunState, unlocked: bool) -> Writer {
            Writer::new(
                BackupStore::new(self._root.path().join("backups")),
                Arc::new(FixedProbe {
                    game,
                    launcher: RunState::NotRunning,
                }),
                unlocked,
            )
        }

        fn file(&self, rel: &str, content: &[u8]) -> LoadedFile {
            let path = self.channel.dir.join(rel);
            write_atomic(&path, content).unwrap();
            load(&path).unwrap()
        }

        fn snapshots(&self, writer: &Writer) -> usize {
            writer.store().list(&self.scope).unwrap().len()
        }
    }

    fn change<'a>(loaded: &'a LoadedFile, new_bytes: Option<&'a [u8]>) -> Change<'a> {
        Change { loaded, new_bytes }
    }

    #[test]
    fn apply_refuses_when_game_running() {
        let fx = fixture();
        let writer = fx.writer(RunState::Running, false);
        let loaded = fx.file("user.cfg", b"old");
        let err = writer
            .apply(
                &fx.scope,
                &fx.channel,
                &[change(&loaded, Some(b"new"))],
                "x",
            )
            .unwrap_err();
        assert!(matches!(err, Error::GameRunning));
        assert_eq!(fs::read(&loaded.path).unwrap(), b"old");
        assert_eq!(fx.snapshots(&writer), 0);
    }

    #[test]
    fn apply_refuses_when_game_state_unknown() {
        let fx = fixture();
        let writer = fx.writer(RunState::Unknown, false);
        let loaded = fx.file("user.cfg", b"old");
        let err = writer
            .apply(
                &fx.scope,
                &fx.channel,
                &[change(&loaded, Some(b"new"))],
                "x",
            )
            .unwrap_err();
        assert!(matches!(err, Error::GameRunning));
        assert_eq!(fs::read(&loaded.path).unwrap(), b"old");
        assert_eq!(fx.snapshots(&writer), 0);
    }

    #[test]
    fn apply_allowed_when_unlocked() {
        let fx = fixture();
        let writer = fx.writer(RunState::Running, true);
        let loaded = fx.file("user.cfg", b"old");
        writer
            .apply(
                &fx.scope,
                &fx.channel,
                &[change(&loaded, Some(b"new"))],
                "x",
            )
            .unwrap();
        assert_eq!(fs::read(&loaded.path).unwrap(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn apply_refuses_ambiguous_case() {
        let fx = fixture();
        let writer = fx.writer(RunState::NotRunning, false);
        let loaded = fx.file("user.cfg", b"old");
        fx.file("USER.cfg", b"other");
        let err = writer
            .apply(
                &fx.scope,
                &fx.channel,
                &[change(&loaded, Some(b"new"))],
                "x",
            )
            .unwrap_err();
        assert!(matches!(err, Error::AmbiguousCase { .. }));
        assert_eq!(fs::read(&loaded.path).unwrap(), b"old");
        assert_eq!(fx.snapshots(&writer), 0);
    }

    #[test]
    fn apply_refuses_when_file_changed_on_disk() {
        let fx = fixture();
        let writer = fx.writer(RunState::NotRunning, false);
        let loaded = fx.file("user.cfg", b"old");
        fs::write(&loaded.path, b"outside").unwrap();
        let err = writer
            .apply(
                &fx.scope,
                &fx.channel,
                &[change(&loaded, Some(b"new"))],
                "x",
            )
            .unwrap_err();
        assert!(matches!(&err, Error::ChangedOnDisk { path } if *path == loaded.path));
        assert_eq!(fs::read(&loaded.path).unwrap(), b"outside");
        assert_eq!(fx.snapshots(&writer), 0);
    }

    #[test]
    fn apply_snapshots_then_writes_and_finishes() {
        let fx = fixture();
        let writer = fx.writer(RunState::NotRunning, false);
        let loaded = fx.file("user.cfg", b"old");
        let info = writer
            .apply(
                &fx.scope,
                &fx.channel,
                &[change(&loaded, Some(b"new"))],
                "edit",
            )
            .unwrap();
        assert_eq!(fs::read(&loaded.path).unwrap(), b"new");
        let listed = writer.store().list(&fx.scope).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].state, SnapshotState::Done);
        assert_eq!(
            writer
                .store()
                .read_file(&fx.scope, &info.id, "user.cfg")
                .unwrap(),
            Some(b"old".to_vec())
        );
    }

    #[test]
    fn apply_none_deletes_file_and_undo_restores_it() {
        let fx = fixture();
        let writer = fx.writer(RunState::NotRunning, false);
        let loaded = fx.file("user.cfg", b"old");
        let info = writer
            .apply(&fx.scope, &fx.channel, &[change(&loaded, None)], "delete")
            .unwrap();
        assert!(!loaded.path.exists());
        writer
            .store()
            .restore(&fx.scope, &fx.channel, &info.id)
            .unwrap();
        assert_eq!(fs::read(&loaded.path).unwrap(), b"old");
    }

    #[test]
    fn apply_two_files_is_all_or_nothing_on_stale_hash() {
        let fx = fixture();
        let writer = fx.writer(RunState::NotRunning, false);
        let first = fx.file("a.cfg", b"a");
        let second = fx.file("b.cfg", b"b");
        fs::write(&second.path, b"outside").unwrap();
        let err = writer
            .apply(
                &fx.scope,
                &fx.channel,
                &[change(&first, Some(b"A")), change(&second, Some(b"B"))],
                "x",
            )
            .unwrap_err();
        assert!(matches!(err, Error::ChangedOnDisk { .. }));
        assert_eq!(fs::read(&first.path).unwrap(), b"a");
        assert_eq!(fs::read(&second.path).unwrap(), b"outside");
    }

    #[cfg(unix)]
    #[test]
    fn apply_rolls_back_first_file_when_second_write_fails() {
        use crate::safe_write::testing::ReadOnly;
        let fx = fixture();
        let writer = fx.writer(RunState::NotRunning, false);
        let first = fx.file("a.cfg", b"a");
        let second = fx.file("locked/b.cfg", b"b");
        let _guard = ReadOnly::new(&fx.channel.dir.join("locked"));
        let err = writer
            .apply(
                &fx.scope,
                &fx.channel,
                &[change(&first, Some(b"A")), change(&second, Some(b"B"))],
                "x",
            )
            .unwrap_err();
        assert!(matches!(&err, Error::Io { path, .. } if *path == second.path));
        assert_eq!(fs::read(&first.path).unwrap(), b"a");
        assert_eq!(fs::read(&second.path).unwrap(), b"b");
        assert!(writer.store().interrupted().unwrap().is_empty());
        let leftovers: Vec<_> = fs::read_dir(&fx.channel.dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(leftovers.len(), 2);
    }

    struct Failing<'a> {
        inner: &'a BackupStore,
        finish: bool,
        roll_back: bool,
    }

    impl Snapshots for Failing<'_> {
        fn begin(
            &self,
            scope: &Scope,
            channel: &Channel,
            files: &[PathBuf],
            label: &str,
        ) -> Result<SnapshotInfo> {
            self.inner.begin(scope, channel, files, label)
        }

        fn finish(&self, scope: &Scope, id: &SnapshotId) -> Result<()> {
            if self.finish {
                return Err(Error::Invalid("injected finish failure".into()));
            }
            self.inner.finish(scope, id)
        }

        fn roll_back(&self, scope: &Scope, channel: &Channel, id: &SnapshotId) -> Result<()> {
            if self.roll_back {
                return Err(Error::Invalid("injected restore failure".into()));
            }
            self.inner.roll_back(scope, channel, id)
        }

        fn delete(&self, scope: &Scope, id: &SnapshotId) -> Result<()> {
            self.inner.delete(scope, id)
        }
    }

    #[test]
    fn apply_returns_a_done_snapshot() {
        let fx = fixture();
        let writer = fx.writer(RunState::NotRunning, false);
        let loaded = fx.file("user.cfg", b"old");
        let info = writer
            .apply(
                &fx.scope,
                &fx.channel,
                &[change(&loaded, Some(b"new"))],
                "x",
            )
            .unwrap();
        assert_eq!(info.state, SnapshotState::Done);
    }

    #[cfg(unix)]
    #[test]
    fn apply_changes_no_game_file_when_the_snapshot_cannot_be_written() {
        use crate::safe_write::testing::ReadOnly;
        let fx = fixture();
        let writer = fx.writer(RunState::NotRunning, false);
        let loaded = fx.file("user.cfg", b"old");
        let backups = fx._root.path().join("backups");
        fs::create_dir_all(&backups).unwrap();
        let _guard = ReadOnly::new(&backups);
        let err = writer
            .apply(
                &fx.scope,
                &fx.channel,
                &[change(&loaded, Some(b"new"))],
                "x",
            )
            .unwrap_err();
        assert!(matches!(err, Error::Io { .. }));
        assert_eq!(fs::read(&loaded.path).unwrap(), b"old");
    }

    #[cfg(unix)]
    #[test]
    fn failed_rollback_is_reported_and_the_snapshot_stays_applying() {
        use crate::safe_write::testing::ReadOnly;
        let fx = fixture();
        let writer = fx.writer(RunState::NotRunning, false);
        let first = fx.file("a.cfg", b"a");
        let second = fx.file("locked/b.cfg", b"b");
        let _guard = ReadOnly::new(&fx.channel.dir.join("locked"));
        let store = Failing {
            inner: writer.store(),
            finish: false,
            roll_back: true,
        };
        let changes = [change(&first, Some(b"A")), change(&second, Some(b"B"))];
        let target = Target {
            scope: &fx.scope,
            channel: &fx.channel,
            changes: &changes,
            label: "x",
        };
        let err = writer.apply_with(&store, &[target]).unwrap_err();
        let Error::RollbackFailed {
            write, snapshot, ..
        } = &err
        else {
            panic!("{err:?}");
        };
        assert!(matches!(&**write, Error::Io { path, .. } if *path == second.path));
        assert!(err.to_string().contains(&snapshot.0));
        assert_eq!(fs::read(&first.path).unwrap(), b"A");
        let interrupted = writer.store().interrupted().unwrap();
        assert_eq!(interrupted.len(), 1);
        assert_eq!(&interrupted[0].1.id, snapshot);
    }

    #[test]
    fn failed_finish_after_good_writes_is_not_a_failed_apply() {
        let fx = fixture();
        let writer = fx.writer(RunState::NotRunning, false);
        let loaded = fx.file("user.cfg", b"old");
        let store = Failing {
            inner: writer.store(),
            finish: true,
            roll_back: false,
        };
        let changes = [change(&loaded, Some(b"new"))];
        let target = Target {
            scope: &fx.scope,
            channel: &fx.channel,
            changes: &changes,
            label: "x",
        };
        let done = writer.apply_with(&store, &[target]).unwrap();
        assert_eq!(done[0].state, SnapshotState::Applying);
        assert_eq!(fs::read(&loaded.path).unwrap(), b"new");
    }

    #[test]
    fn restore_refuses_while_the_game_runs() {
        let fx = fixture();
        let loaded = fx.file("user.cfg", b"old");
        let idle = fx.writer(RunState::NotRunning, false);
        let info = idle
            .apply(
                &fx.scope,
                &fx.channel,
                &[change(&loaded, Some(b"new"))],
                "x",
            )
            .unwrap();
        for game in [RunState::Running, RunState::Unknown] {
            let busy = fx.writer(game, false);
            let err = busy.restore(&fx.scope, &fx.channel, &info.id).unwrap_err();
            assert!(matches!(err, Error::GameRunning));
            assert_eq!(fs::read(&loaded.path).unwrap(), b"new");
        }
        idle.restore(&fx.scope, &fx.channel, &info.id).unwrap();
        assert_eq!(fs::read(&loaded.path).unwrap(), b"old");
    }
}
