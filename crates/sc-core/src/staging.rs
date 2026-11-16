//! Staged edits over a parsed document.

use crate::backup::{Scope, SnapshotInfo};
use crate::error::{Error, Result};
use crate::guard::{Change, Writer};
use crate::install::Channel;
use crate::safe_write::{self, LoadedFile};
use std::path::Path;

pub trait Document: Sized + Clone {
    type Op: Clone;
    fn parse(path: &Path, bytes: &[u8]) -> Result<Self>;
    fn to_bytes(&self) -> Vec<u8>;
    fn apply(&mut self, op: &Self::Op) -> Result<()>;
}

pub struct Staged<D: Document> {
    loaded: LoadedFile,
    base: D,
    current: D,
    ops: Vec<D::Op>,
}

impl<D: Document> Staged<D> {
    pub fn open(path: &Path) -> Result<Self> {
        let loaded = safe_write::load(path)?;
        let base = D::parse(path, &loaded.bytes)?;
        Ok(Self {
            loaded,
            current: base.clone(),
            base,
            ops: Vec::new(),
        })
    }

    pub fn loaded(&self) -> &LoadedFile {
        &self.loaded
    }

    pub fn base(&self) -> &D {
        &self.base
    }

    pub fn current(&self) -> &D {
        &self.current
    }

    pub fn pending(&self) -> usize {
        self.ops.len()
    }

    pub fn stage(&mut self, op: D::Op) -> Result<()> {
        let mut next = self.current.clone();
        next.apply(&op)?;
        self.current = next;
        self.ops.push(op);
        Ok(())
    }

    pub fn undo_last(&mut self) -> bool {
        if self.ops.pop().is_none() {
            return false;
        }
        let (replayed, _) = replay(&self.base, std::mem::take(&mut self.ops));
        (self.current, self.ops) = replayed;
        true
    }

    pub fn discard(&mut self) {
        self.ops.clear();
        self.current = self.base.clone();
    }

    pub fn reload_and_replay(&mut self) -> Result<usize> {
        let loaded = safe_write::load(&self.loaded.path)?;
        let base = D::parse(&loaded.path, &loaded.bytes)?;
        let (replayed, dropped) = replay(&base, std::mem::take(&mut self.ops));
        self.loaded = loaded;
        self.base = base;
        (self.current, self.ops) = replayed;
        Ok(dropped)
    }

    pub fn commit(
        &mut self,
        writer: &Writer,
        scope: &Scope,
        channel: &Channel,
        label: &str,
    ) -> Result<SnapshotInfo> {
        if self.ops.is_empty() {
            return Err(Error::Invalid("nothing to apply".into()));
        }
        let bytes = self.current.to_bytes();
        let change = Change {
            loaded: &self.loaded,
            new_bytes: Some(&bytes),
        };
        let info = writer.apply(scope, channel, &[change], label)?;
        self.loaded = safe_write::load(&self.loaded.path).unwrap_or_else(|_| LoadedFile {
            path: self.loaded.path.clone(),
            hash: Some(safe_write::hash(&bytes)),
            bytes,
        });
        self.base = D::parse(&self.loaded.path, &self.loaded.bytes)
            .unwrap_or_else(|_| self.current.clone());
        self.current = self.base.clone();
        self.ops.clear();
        Ok(info)
    }
}

type Replayed<D> = (D, Vec<<D as Document>::Op>);

fn replay<D: Document>(base: &D, ops: Vec<D::Op>) -> (Replayed<D>, usize) {
    let mut current = base.clone();
    let total = ops.len();
    let kept: Vec<D::Op> = ops
        .into_iter()
        .filter(|op| {
            let mut next = current.clone();
            let applied = next.apply(op).is_ok();
            if applied {
                current = next;
            }
            applied
        })
        .collect();
    let dropped = total - kept.len();
    ((current, kept), dropped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::{BackupStore, Scope};
    use crate::install::Channel;
    use crate::process::{FixedProbe, RunState};
    use crate::safe_write::write_atomic;
    use std::fs;
    use std::sync::Arc;
    use tempfile::{TempDir, tempdir};

    #[derive(Debug, Clone, PartialEq)]
    struct Lines(Vec<(String, String)>);

    #[derive(Debug, Clone)]
    struct Set {
        key: String,
        value: String,
    }

    impl Document for Lines {
        type Op = Set;

        fn parse(_path: &Path, bytes: &[u8]) -> Result<Self> {
            let text = String::from_utf8_lossy(bytes);
            Ok(Lines(
                text.lines()
                    .filter_map(|line| line.split_once('='))
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect(),
            ))
        }

        fn to_bytes(&self) -> Vec<u8> {
            self.0
                .iter()
                .map(|(key, value)| format!("{key}={value}\n"))
                .collect::<String>()
                .into_bytes()
        }

        fn apply(&mut self, op: &Set) -> Result<()> {
            match self.0.iter_mut().find(|(key, _)| *key == op.key) {
                Some(entry) => {
                    entry.1 = op.value.clone();
                    Ok(())
                }
                None => Err(Error::Invalid(format!("no key {}", op.key))),
            }
        }
    }

    fn set(key: &str, value: &str) -> Set {
        Set {
            key: key.into(),
            value: value.into(),
        }
    }

    struct Fixture {
        _root: TempDir,
        channel: Channel,
        scope: Scope,
        writer: Writer,
        path: std::path::PathBuf,
    }

    fn fixture(content: &str) -> Fixture {
        let root = tempdir().unwrap();
        let channel = Channel {
            name: "LIVE".into(),
            dir: root.path().join("LIVE"),
        };
        let path = channel.dir.join("user.cfg");
        write_atomic(&path, content.as_bytes()).unwrap();
        let writer = Writer::new(
            BackupStore::new(root.path().join("backups")),
            Arc::new(FixedProbe {
                game: RunState::NotRunning,
                launcher: RunState::NotRunning,
            }),
            false,
        );
        Fixture {
            channel,
            scope: Scope {
                install_key: "k".into(),
                channel: "LIVE".into(),
            },
            writer,
            path,
            _root: root,
        }
    }

    #[test]
    fn stage_changes_current_not_base_and_counts() {
        let fx = fixture("a=1\nb=2\n");
        let mut staged = Staged::<Lines>::open(&fx.path).unwrap();
        staged.stage(set("a", "9")).unwrap();
        assert_eq!(staged.pending(), 1);
        assert_eq!(staged.current().0[0].1, "9");
        assert_eq!(staged.base().0[0].1, "1");
        assert!(staged.stage(set("zzz", "1")).is_err());
        assert_eq!(staged.pending(), 1);
    }

    #[test]
    fn undo_last_and_discard_restore_base() {
        let fx = fixture("a=1\nb=2\n");
        let mut staged = Staged::<Lines>::open(&fx.path).unwrap();
        staged.stage(set("a", "9")).unwrap();
        staged.stage(set("b", "8")).unwrap();
        assert!(staged.undo_last());
        assert_eq!(staged.current().0[1].1, "2");
        assert_eq!(staged.current().0[0].1, "9");
        staged.discard();
        assert_eq!(staged.current(), staged.base());
        assert_eq!(staged.pending(), 0);
        assert!(!staged.undo_last());
    }

    #[test]
    fn commit_writes_current_and_resets_pending() {
        let fx = fixture("a=1\nb=2\n");
        let mut staged = Staged::<Lines>::open(&fx.path).unwrap();
        staged.stage(set("a", "9")).unwrap();
        staged
            .commit(&fx.writer, &fx.scope, &fx.channel, "edit a")
            .unwrap();
        assert_eq!(fs::read(&fx.path).unwrap(), b"a=9\nb=2\n");
        assert_eq!(staged.pending(), 0);
        assert_eq!(staged.base(), staged.current());
        assert_eq!(staged.loaded().bytes, b"a=9\nb=2\n");
        staged.stage(set("b", "7")).unwrap();
        staged
            .commit(&fx.writer, &fx.scope, &fx.channel, "edit b")
            .unwrap();
        assert_eq!(fs::read(&fx.path).unwrap(), b"a=9\nb=7\n");
    }

    #[test]
    fn commit_with_nothing_pending_is_invalid() {
        let fx = fixture("a=1\n");
        let mut staged = Staged::<Lines>::open(&fx.path).unwrap();
        let err = staged
            .commit(&fx.writer, &fx.scope, &fx.channel, "x")
            .unwrap_err();
        assert!(matches!(err, Error::Invalid(_)));
    }

    #[test]
    fn reload_and_replay_keeps_ops_on_top_of_outside_change() {
        let fx = fixture("a=1\n");
        let mut staged = Staged::<Lines>::open(&fx.path).unwrap();
        staged.stage(set("a", "9")).unwrap();
        fs::write(&fx.path, "a=1\nc=3\n").unwrap();
        assert_eq!(staged.reload_and_replay().unwrap(), 0);
        assert_eq!(staged.pending(), 1);
        assert_eq!(staged.current().0.len(), 2);
        assert_eq!(staged.current().0[0].1, "9");
        staged
            .commit(&fx.writer, &fx.scope, &fx.channel, "edit")
            .unwrap();
        assert_eq!(fs::read(&fx.path).unwrap(), b"a=9\nc=3\n");
    }

    #[test]
    fn reload_and_replay_drops_ops_that_no_longer_apply() {
        let fx = fixture("a=1\nb=2\n");
        let mut staged = Staged::<Lines>::open(&fx.path).unwrap();
        staged.stage(set("a", "9")).unwrap();
        fs::write(&fx.path, "b=2\n").unwrap();
        assert_eq!(staged.reload_and_replay().unwrap(), 1);
        assert_eq!(staged.pending(), 0);
    }
}
