//! The single guarded write path for files inside a game folder.

use crate::backup::{BackupStore, Scope, SnapshotInfo, relative_to};
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

    pub fn apply(
        &self,
        scope: &Scope,
        channel: &Channel,
        changes: &[Change],
        label: &str,
    ) -> Result<SnapshotInfo> {
        if self.blocked() {
            return Err(Error::GameRunning);
        }
        check_cases(channel, changes)?;
        check_unchanged(changes)?;
        let paths: Vec<PathBuf> = changes
            .iter()
            .map(|change| change.loaded.path.clone())
            .collect();
        let snapshot = self.store.begin(scope, channel, &paths, label)?;
        if let Err(err) = write_all(changes) {
            let _ = self.store.restore(scope, channel, &snapshot.id);
            return Err(err);
        }
        self.store.finish(scope, &snapshot.id)?;
        Ok(snapshot)
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
    use crate::backup::SnapshotState;
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
}
