//! Staged edits that keep showing while a background write runs, and the guarded write
//! that replays them once when another writer changed the file first.

use super::words::{GAME_RUNNING, ROLLBACK_FAILED, words};
use crate::screen::{Origin, Toast};
use sc_core::backup::{Scope, SnapshotInfo};
use sc_core::error::{Error, Result};
use sc_core::guard::{Change, Writer};
use sc_core::install::Channel;
use sc_core::safe_write::LoadedFile;
use sc_core::staging::{Document, Staged};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};

pub const CHANGED_ON_DISK: &str =
    "The file changed on disk. Your edits were kept. Check them and apply again.";
pub const FORMAT_CHANGED: &str = "The file changed on disk and its format is no longer recognised, so star-config did not change it. Your edits were kept.";
const RELOADED: &str = "The changes were not applied, and the files were loaded again meanwhile, so your edits were dropped.";

/// Moves a value that is not `Clone` through a message.
pub struct Carry<T>(Arc<Mutex<Option<T>>>);

impl<T> Carry<T> {
    pub fn new(value: T) -> Self {
        Self(Arc::new(Mutex::new(Some(value))))
    }

    pub fn take(&self) -> Option<T> {
        self.0.lock().ok()?.take()
    }
}

impl<T> Clone for Carry<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> fmt::Debug for Carry<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Carry(..)")
    }
}

/// How many settings differ between the file and the staged document.
pub type Counter<D> = fn(&D, &D) -> usize;

/// Counts keys added, removed or changed between two key to value maps.
pub fn changed_keys(base: &BTreeMap<String, String>, current: &BTreeMap<String, String>) -> usize {
    let changed = base
        .iter()
        .filter(|(key, value)| current.get(*key) != Some(*value))
        .count();
    let added = current
        .keys()
        .filter(|key| !base.contains_key(*key))
        .count();
    changed + added
}

enum Slot<D: Document> {
    Here(Staged<D>),
    Away { base: D, current: D, missing: bool },
}

/// A [`Staged`] document that can be lent to a background write while the screen keeps
/// showing a copy of it.
pub struct Edits<D: Document> {
    slot: Slot<D>,
    keys: Vec<Option<String>>,
    count: usize,
    counter: Counter<D>,
}

impl<D: Document> Edits<D> {
    pub fn new(staged: Staged<D>, counter: Counter<D>) -> Self {
        let mut edits = Self {
            slot: Slot::Here(staged),
            keys: Vec::new(),
            count: 0,
            counter,
        };
        edits.recount();
        edits
    }

    pub fn base(&self) -> &D {
        match &self.slot {
            Slot::Here(staged) => staged.base(),
            Slot::Away { base, .. } => base,
        }
    }

    pub fn current(&self) -> &D {
        match &self.slot {
            Slot::Here(staged) => staged.current(),
            Slot::Away { current, .. } => current,
        }
    }

    /// True when the file did not exist when it was loaded.
    pub fn missing(&self) -> bool {
        match &self.slot {
            Slot::Here(staged) => staged.loaded().hash.is_none(),
            Slot::Away { missing, .. } => *missing,
        }
    }

    pub fn is_away(&self) -> bool {
        matches!(self.slot, Slot::Away { .. })
    }

    pub fn pending(&self) -> usize {
        self.count
    }

    /// Stages `op`. Consecutive edits with the same `key` (typing in one field) replace each
    /// other, so one undo takes back the whole edit.
    pub fn stage(&mut self, op: D::Op, key: Option<&str>) -> Result<()> {
        let Slot::Here(staged) = &mut self.slot else {
            return Err(Error::Invalid("changes are being applied".into()));
        };
        let merge = key.is_some() && self.keys.last().map(Option::as_deref) == Some(key);
        if merge {
            staged.current().clone().apply(&op)?;
            staged.undo_last();
            self.keys.pop();
        }
        staged.stage(op)?;
        self.keys.push(key.map(str::to_string));
        self.recount();
        Ok(())
    }

    pub fn undo(&mut self) -> bool {
        let Slot::Here(staged) = &mut self.slot else {
            return false;
        };
        if !staged.undo_last() {
            return false;
        }
        self.keys.pop();
        self.recount();
        true
    }

    pub fn discard(&mut self) {
        if let Slot::Here(staged) = &mut self.slot {
            staged.discard();
            self.keys.clear();
            self.recount();
        }
    }

    /// Takes the staged document for a background write, leaving a copy to show.
    pub fn lend(&mut self) -> Option<Staged<D>> {
        if self.is_away() {
            return None;
        }
        let copy = Slot::Away {
            base: self.base().clone(),
            current: self.current().clone(),
            missing: self.missing(),
        };
        match std::mem::replace(&mut self.slot, copy) {
            Slot::Here(staged) => Some(staged),
            Slot::Away { .. } => None,
        }
    }

    /// True while an edit staged under `key` is still pending.
    pub fn has_key(&self, key: &str) -> bool {
        self.keys
            .iter()
            .any(|staged| staged.as_deref() == Some(key))
    }

    pub fn put_back(&mut self, staged: Staged<D>) {
        let kept = staged.pending();
        if kept == 0 {
            self.keys.clear();
        } else if kept != self.keys.len() {
            self.keys = vec![None; kept];
        }
        self.slot = Slot::Here(staged);
        self.recount();
    }

    fn recount(&mut self) {
        self.count = (self.counter)(self.base(), self.current());
    }
}

/// A staged file as the guarded write sees it.
pub trait Pending {
    fn loaded(&self) -> &LoadedFile;
    /// The bytes to write, or `None` when they equal the file.
    fn bytes(&self) -> Option<Vec<u8>>;
    /// Loads the file again and replays the staged edits on it. Returns how many edits no
    /// longer applied and were dropped.
    fn replay(&mut self) -> Result<usize>;
    /// Whether writing the parsed file back unchanged gives the bytes on disk.
    fn round_trips(&self) -> bool;
    /// Loads the file again after it was written, dropping the applied edits.
    fn reopen(&mut self);
}

impl<D: Document> Pending for Staged<D> {
    fn loaded(&self) -> &LoadedFile {
        Staged::loaded(self)
    }

    fn bytes(&self) -> Option<Vec<u8>> {
        let bytes = self.current().to_bytes();
        (bytes != Staged::loaded(self).bytes).then_some(bytes)
    }

    fn replay(&mut self) -> Result<usize> {
        self.reload_and_replay()
    }

    fn round_trips(&self) -> bool {
        self.base().to_bytes() == Staged::loaded(self).bytes
    }

    fn reopen(&mut self) {
        let path = Staged::loaded(self).path.clone();
        match Staged::open(&path) {
            Ok(fresh) => *self = fresh,
            Err(_) => self.discard(),
        }
    }
}

/// Where a guarded write goes and the label of its snapshot.
pub struct Target {
    pub writer: Writer,
    pub scope: Scope,
    pub channel: Channel,
    pub label: &'static str,
    /// False when a staged edit replaces the whole file, so replaying it on a file someone
    /// else changed would throw their change away.
    pub replay: bool,
}

/// What a guarded write did. `info` is `None` when there was nothing to write.
pub struct Written {
    pub info: Option<SnapshotInfo>,
    /// Staged edits that no longer applied to the changed file and were dropped.
    pub dropped: usize,
}

impl Target {
    pub fn origin(&self) -> Origin {
        Origin {
            scope: self.scope.clone(),
            channel: self.channel.clone(),
        }
    }
}

/// Writes every file that has changes in one snapshot. When a file changed on disk since it
/// was loaded, reloads every file and replays the staged edits. Then it tries once more,
/// unless `target.replay` is false: the edits stay staged on the new file for the user to
/// check.
pub fn write(target: &Target, files: &mut [&mut dyn Pending]) -> Result<Written> {
    let changed = match write_once(target, files) {
        Err(changed @ Error::ChangedOnDisk { .. }) => changed,
        other => return other.map(|info| Written { info, dropped: 0 }),
    };
    let mut dropped = 0;
    for file in files.iter_mut() {
        dropped += file.replay()?;
        if !file.round_trips() {
            return Err(Error::Invalid(FORMAT_CHANGED.into()));
        }
    }
    if !target.replay {
        return Err(changed);
    }
    let info = write_once(target, files)?;
    Ok(Written { info, dropped })
}

fn write_once(target: &Target, files: &mut [&mut dyn Pending]) -> Result<Option<SnapshotInfo>> {
    let bytes: Vec<Option<Vec<u8>>> = files.iter().map(|file| file.bytes()).collect();
    let info = {
        let changes: Vec<Change<'_>> = files
            .iter()
            .zip(&bytes)
            .filter_map(|(file, bytes)| {
                bytes.as_deref().map(|new_bytes| Change {
                    loaded: file.loaded(),
                    new_bytes: Some(new_bytes),
                })
            })
            .collect();
        if changes.is_empty() {
            return Ok(None);
        }
        target
            .writer
            .apply(&target.scope, &target.channel, &changes, target.label)?
    };
    for (file, bytes) in files.iter_mut().zip(&bytes) {
        if bytes.is_some() {
            file.reopen();
        }
    }
    Ok(Some(info))
}

/// The toast that reports a guarded write of `count` staged changes into `origin`.
pub fn toast_for(result: &Result<Written>, count: usize, origin: &Origin) -> Option<Toast> {
    match result {
        Ok(Written {
            info: None,
            dropped: 0,
        }) => None,
        Ok(Written {
            info: None,
            dropped,
        }) => Some(Toast::info(format!(
            "{} Nothing was written.",
            dropped_text(*dropped)
        ))),
        Ok(Written {
            info: Some(info),
            dropped,
        }) => {
            let applied = applied_text(count.saturating_sub(*dropped).max(1));
            let text = match dropped {
                0 => applied,
                _ => format!("{applied}. {}", dropped_text(*dropped)),
            };
            Some(Toast::success(text).with_undo_in(vec![origin.undo(info.id.clone())]))
        }
        Err(err) => Some(failure_toast(err)),
    }
}

/// The toast for a write whose result came back after the screen loaded its files again, so
/// the edits it carried are gone.
pub fn reloaded_toast(result: &Result<Written>, count: usize, origin: &Origin) -> Option<Toast> {
    match result {
        Err(Error::RollbackFailed { .. }) | Ok(_) => toast_for(result, count, origin),
        Err(_) => Some(Toast::error(RELOADED)),
    }
}

fn dropped_text(dropped: usize) -> String {
    if dropped == 1 {
        "1 edit no longer fits the file and was dropped.".to_string()
    } else {
        format!("{dropped} edits no longer fit the file and were dropped.")
    }
}

fn applied_text(count: usize) -> String {
    if count == 1 {
        "Applied 1 change".to_string()
    } else {
        format!("Applied {count} changes")
    }
}

pub fn failure_toast(err: &Error) -> Toast {
    match err {
        Error::ChangedOnDisk { .. } => Toast::info(CHANGED_ON_DISK),
        Error::Invalid(message) if message == FORMAT_CHANGED => Toast::error(FORMAT_CHANGED),
        Error::GameRunning => Toast::error(GAME_RUNNING),
        Error::RollbackFailed { .. } => Toast::error(ROLLBACK_FAILED),
        other => Toast::error(format!(
            "Could not apply the changes: {}. Your edits were kept.",
            words(other)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::backup::BackupStore;
    use sc_core::process::{GameProbe, RunState};
    use sc_core::usercfg::{Op, UserCfg};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Rewrites the file the second time the writer asks whether the game runs, which is
    /// between the replay and the second try.
    struct Rewrites {
        path: PathBuf,
        calls: AtomicUsize,
    }

    impl GameProbe for Rewrites {
        fn game(&self) -> RunState {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
                fs::write(&self.path, "cl_fov = 70\n").unwrap();
            }
            RunState::NotRunning
        }

        fn launcher(&self) -> RunState {
            RunState::NotRunning
        }
    }

    fn target(temp: &Path, probe: Arc<dyn GameProbe>, replay: bool) -> Target {
        let dir = temp.join("LIVE");
        fs::create_dir_all(&dir).unwrap();
        Target {
            writer: Writer::new(BackupStore::new(temp.join("snapshots")), probe, false),
            scope: Scope {
                install_key: "test".into(),
                channel: "LIVE".into(),
            },
            channel: Channel {
                name: "LIVE".into(),
                dir,
            },
            label: "test",
            replay,
        }
    }

    fn idle() -> Arc<dyn GameProbe> {
        Arc::new(sc_core::process::FixedProbe {
            game: RunState::NotRunning,
            launcher: RunState::NotRunning,
        })
    }

    fn staged(target: &Target, text: &str, op: Op) -> Staged<UserCfg> {
        let path = target.channel.user_cfg();
        fs::write(&path, text).unwrap();
        let mut staged = Staged::<UserCfg>::open(&path).unwrap();
        staged.stage(op).unwrap();
        staged
    }

    fn set(key: &str, value: &str) -> Op {
        Op::Set {
            key: key.into(),
            value: value.into(),
        }
    }

    #[test]
    fn a_second_outside_change_keeps_the_edits_and_says_so() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("LIVE").join("user.cfg");
        let probe = Arc::new(Rewrites {
            path: path.clone(),
            calls: AtomicUsize::new(0),
        });
        let target = target(temp.path(), probe, true);
        let mut file = staged(&target, "cl_fov = 90\n", set("r_DisplayInfo", "1"));
        fs::write(&path, "cl_fov = 80\n").unwrap();
        let result = write(&target, &mut [&mut file as &mut dyn Pending]);
        assert!(matches!(result, Err(Error::ChangedOnDisk { .. })));
        assert_eq!(file.pending(), 1);
        assert_eq!(fs::read_to_string(&path).unwrap(), "cl_fov = 70\n");
        let toast = toast_for(&result, 1, &target.origin()).unwrap();
        assert_eq!(toast.text, CHANGED_ON_DISK);
    }

    #[test]
    fn a_whole_file_edit_is_not_replayed_over_an_outside_change() {
        let temp = tempfile::tempdir().unwrap();
        let target = target(temp.path(), idle(), false);
        let path = target.channel.user_cfg();
        let mut file = staged(
            &target,
            "cl_fov = 90\n",
            Op::ReplaceText("cl_fov = 100\n".into()),
        );
        fs::write(&path, "cl_fov = 90\ng_language = english\n").unwrap();
        let result = write(&target, &mut [&mut file as &mut dyn Pending]);
        assert!(matches!(result, Err(Error::ChangedOnDisk { .. })));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "cl_fov = 90\ng_language = english\n"
        );
        assert_eq!(file.pending(), 1);
        let again = write(&target, &mut [&mut file as &mut dyn Pending]);
        assert!(again.is_ok_and(|written| written.info.is_some()));
    }

    #[test]
    fn edits_that_no_longer_apply_are_counted_in_the_toast() {
        let temp = tempfile::tempdir().unwrap();
        let target = target(temp.path(), idle(), true);
        let path = target.channel.user_cfg();
        let mut file = staged(&target, "a = 1\nb = 2\n", set("a", "5"));
        file.stage(Op::Remove { key: "b".into() }).unwrap();
        fs::write(&path, "a = 1\n").unwrap();
        let result = write(&target, &mut [&mut file as &mut dyn Pending]);
        assert_eq!(fs::read_to_string(&path).unwrap(), "a = 5\n");
        assert_eq!(result.as_ref().map(|written| written.dropped).ok(), Some(1));
        let toast = toast_for(&result, 3, &target.origin()).unwrap();
        assert_eq!(
            toast.text,
            "Applied 2 changes. 1 edit no longer fits the file and was dropped."
        );
    }

    /// A staged file whose format stops passing the round trip once it is loaded again.
    struct Drifts(Staged<UserCfg>, bool);

    impl Pending for Drifts {
        fn loaded(&self) -> &LoadedFile {
            Pending::loaded(&self.0)
        }

        fn bytes(&self) -> Option<Vec<u8>> {
            Pending::bytes(&self.0)
        }

        fn replay(&mut self) -> Result<usize> {
            self.1 = true;
            Pending::replay(&mut self.0)
        }

        fn round_trips(&self) -> bool {
            !self.1
        }

        fn reopen(&mut self) {
            Pending::reopen(&mut self.0)
        }
    }

    #[test]
    fn a_file_that_no_longer_round_trips_after_the_replay_is_not_written() {
        let temp = tempfile::tempdir().unwrap();
        let target = target(temp.path(), idle(), true);
        let path = target.channel.user_cfg();
        let mut file = Drifts(staged(&target, "a = 1\n", set("a", "5")), false);
        fs::write(&path, "a = 2\n").unwrap();
        let result = write(&target, &mut [&mut file as &mut dyn Pending]);
        assert_eq!(fs::read_to_string(&path).unwrap(), "a = 2\n");
        let toast = toast_for(&result, 1, &target.origin()).unwrap();
        assert_eq!(toast.text, FORMAT_CHANGED);
    }

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn apply_errors_name_the_file_without_its_path_or_code() {
        let err = Error::Io {
            path: PathBuf::from("/home/someone/Games/StarCitizen/LIVE/user.cfg"),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        };
        let toast = failure_toast(&err);
        assert_eq!(
            toast.text,
            "Could not apply the changes: user.cfg: permission denied. Your edits were kept."
        );
    }

    #[test]
    fn changed_keys_counts_added_removed_and_changed() {
        let base = map(&[("a", "1"), ("b", "2"), ("c", "3")]);
        let current = map(&[("a", "1"), ("b", "9"), ("d", "4")]);
        assert_eq!(changed_keys(&base, &current), 3);
        assert_eq!(changed_keys(&base, &base), 0);
    }

    #[test]
    fn carry_hands_its_value_over_once() {
        let carry = Carry::new(String::from("x"));
        let copy = carry.clone();
        assert_eq!(copy.take().as_deref(), Some("x"));
        assert_eq!(carry.take(), None);
    }

    #[test]
    fn applied_text_counts() {
        assert_eq!(applied_text(1), "Applied 1 change");
        assert_eq!(applied_text(3), "Applied 3 changes");
    }
}
