//! Staged edits that keep showing while a background write runs, and the guarded write
//! that replays them once when another writer changed the file first.

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
    fn replay(&mut self) -> Result<()>;
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

    fn replay(&mut self) -> Result<()> {
        self.reload_and_replay().map(|_| ())
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
/// was loaded, reloads every file, replays the staged edits and tries once more.
/// `Ok(None)` means there was nothing to write.
pub fn write(target: &Target, files: &mut [&mut dyn Pending]) -> Result<Option<SnapshotInfo>> {
    match write_once(target, files) {
        Err(Error::ChangedOnDisk { .. }) => {
            for file in files.iter_mut() {
                file.replay()?;
            }
            write_once(target, files)
        }
        other => other,
    }
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
pub fn toast_for(
    result: &Result<Option<SnapshotInfo>>,
    count: usize,
    origin: &Origin,
) -> Option<Toast> {
    match result {
        Ok(None) => None,
        Ok(Some(info)) => Some(
            Toast::success(applied_text(count)).with_undo_in(vec![origin.undo(info.id.clone())]),
        ),
        Err(err) => Some(failure_toast(err)),
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
        Error::GameRunning => {
            Toast::error("Star Citizen is running, so nothing was written. Your edits were kept.")
        }
        Error::RollbackFailed { .. } => Toast::error(
            "Writing failed, and putting the earlier files back failed too. Some files may \
             have changed. The snapshot on the Backups screen holds the originals.",
        ),
        other => Toast::error(format!(
            "Could not apply the changes: {other}. Your edits were kept."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
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
