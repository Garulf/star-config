//! Snapshots of the files star-config is about to change.

use crate::error::{Error, Result};
use crate::install::Channel;
use crate::safe_write::{self, hash, write_atomic};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use time::OffsetDateTime;
use time::macros::format_description;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SnapshotId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub install_key: String,
    pub channel: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapshotState {
    Applying,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotFile {
    pub rel: String,
    pub existed: bool,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    pub id: SnapshotId,
    pub created_unix: i64,
    pub label: String,
    pub state: SnapshotState,
    pub files: Vec<SnapshotFile>,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    created_unix: i64,
    label: String,
    state: SnapshotState,
    files: Vec<SnapshotFile>,
}

#[derive(Debug, Clone)]
pub struct BackupStore {
    root: PathBuf,
}

fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\'])
}

pub(crate) fn relative_to(channel: &Channel, file: &Path) -> Result<String> {
    let outside = || {
        Error::Invalid(format!(
            "{} is outside {}",
            file.display(),
            channel.dir.display()
        ))
    };
    let rel = file.strip_prefix(&channel.dir).map_err(|_| outside())?;
    let parts: Option<Vec<String>> = rel
        .components()
        .map(|part| match part {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    match parts {
        Some(parts) if !parts.is_empty() => Ok(parts.join("/")),
        _ => Err(outside()),
    }
}

fn is_plain_rel(rel: &str) -> bool {
    rel.split('/').all(|part| {
        is_plain_name(part)
            && matches!(
                Path::new(part).components().collect::<Vec<_>>().as_slice(),
                [Component::Normal(_)]
            )
    })
}

fn sequence(id: &str) -> u32 {
    id.split_once("Z-")
        .and_then(|(_, number)| number.parse().ok())
        .unwrap_or(1)
}

fn read_manifest(dir: &Path) -> Result<Option<SnapshotInfo>> {
    let path = dir.join("manifest.toml");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Error::io(&path, err)),
    };
    let Ok(manifest) = toml::from_str::<Manifest>(&text) else {
        return Ok(None);
    };
    if !manifest.files.iter().all(|file| is_plain_rel(&file.rel)) {
        return Ok(None);
    }
    let id = dir.file_name().unwrap_or_default().to_string_lossy();
    Ok(Some(SnapshotInfo {
        id: SnapshotId(id.into_owned()),
        created_unix: manifest.created_unix,
        label: manifest.label,
        state: manifest.state,
        files: manifest.files,
    }))
}

fn write_manifest(dir: &Path, info: &SnapshotInfo) -> Result<()> {
    let manifest = Manifest {
        created_unix: info.created_unix,
        label: info.label.clone(),
        state: info.state,
        files: info.files.clone(),
    };
    let text = toml::to_string(&manifest).map_err(|err| Error::Invalid(err.to_string()))?;
    write_atomic(&dir.join("manifest.toml"), text.as_bytes())
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

impl BackupStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn scope_dir(&self, scope: &Scope) -> Result<PathBuf> {
        if !is_plain_name(&scope.install_key) || !is_plain_name(&scope.channel) {
            return Err(Error::Invalid(format!(
                "bad backup scope {}/{}",
                scope.install_key, scope.channel
            )));
        }
        Ok(self.root.join(&scope.install_key).join(&scope.channel))
    }

    fn snapshot_dir(&self, scope: &Scope, id: &SnapshotId) -> Result<PathBuf> {
        if !is_plain_name(&id.0) {
            return Err(Error::Invalid(format!("bad snapshot id {}", id.0)));
        }
        Ok(self.scope_dir(scope)?.join(&id.0))
    }

    fn info(&self, scope: &Scope, id: &SnapshotId) -> Result<(PathBuf, SnapshotInfo)> {
        let dir = self.snapshot_dir(scope, id)?;
        match read_manifest(&dir)? {
            Some(info) => Ok((dir, info)),
            None => Err(Error::NotFound(format!("snapshot {}", id.0))),
        }
    }

    fn create_snapshot_dir(
        &self,
        scope: &Scope,
        now: OffsetDateTime,
    ) -> Result<(SnapshotId, PathBuf)> {
        let parent = self.scope_dir(scope)?;
        fs::create_dir_all(&parent).map_err(|err| Error::io(&parent, err))?;
        let stamp = now
            .format(format_description!(
                "[year][month][day]T[hour][minute][second]Z"
            ))
            .map_err(|err| Error::Invalid(err.to_string()))?;
        for attempt in 1u32.. {
            let id = if attempt == 1 {
                stamp.clone()
            } else {
                format!("{stamp}-{attempt}")
            };
            let dir = parent.join(&id);
            match fs::create_dir(&dir) {
                Ok(()) => return Ok((SnapshotId(id), dir)),
                Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(Error::io(&dir, err)),
            }
        }
        unreachable!("the attempt counter does not run out")
    }

    fn copy_files(
        &self,
        dir: &Path,
        channel: &Channel,
        rels: &[String],
    ) -> Result<Vec<SnapshotFile>> {
        rels.iter()
            .map(|rel| {
                let loaded = safe_write::load(&channel.dir.join(rel))?;
                let existed = loaded.hash.is_some();
                if existed {
                    write_atomic(&dir.join("files").join(rel), &loaded.bytes)?;
                }
                Ok(SnapshotFile {
                    rel: rel.clone(),
                    existed,
                    size: loaded.bytes.len() as u64,
                })
            })
            .collect()
    }

    pub fn begin(
        &self,
        scope: &Scope,
        channel: &Channel,
        files: &[PathBuf],
        label: &str,
    ) -> Result<SnapshotInfo> {
        let mut rels: Vec<String> = Vec::new();
        for file in files {
            let rel = relative_to(channel, file)?;
            if !rels.contains(&rel) {
                rels.push(rel);
            }
        }
        let now = OffsetDateTime::now_utc();
        let (id, dir) = self.create_snapshot_dir(scope, now)?;
        let built = self.copy_files(&dir, channel, &rels).and_then(|files| {
            let info = SnapshotInfo {
                id,
                created_unix: now.unix_timestamp(),
                label: label.to_string(),
                state: SnapshotState::Applying,
                files,
            };
            write_manifest(&dir, &info)?;
            Ok(info)
        });
        if built.is_err() {
            let _ = fs::remove_dir_all(&dir);
        }
        built
    }

    pub fn finish(&self, scope: &Scope, id: &SnapshotId) -> Result<()> {
        let (dir, mut info) = self.info(scope, id)?;
        info.state = SnapshotState::Done;
        write_manifest(&dir, &info)
    }

    pub fn snapshot(
        &self,
        scope: &Scope,
        channel: &Channel,
        files: &[PathBuf],
        label: &str,
    ) -> Result<SnapshotInfo> {
        let mut info = self.begin(scope, channel, files, label)?;
        self.finish(scope, &info.id)?;
        info.state = SnapshotState::Done;
        Ok(info)
    }

    pub fn list(&self, scope: &Scope) -> Result<Vec<SnapshotInfo>> {
        let mut found = Vec::new();
        for dir in subdirs(&self.scope_dir(scope)?) {
            if let Some(info) = read_manifest(&dir)? {
                found.push(info);
            }
        }
        found.sort_by_key(|info| std::cmp::Reverse((info.created_unix, sequence(&info.id.0))));
        Ok(found)
    }

    pub fn interrupted(&self) -> Result<Vec<(Scope, SnapshotInfo)>> {
        let mut found = Vec::new();
        for install in subdirs(&self.root) {
            for channel in subdirs(&install) {
                let scope = Scope {
                    install_key: install
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    channel: channel
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                };
                for info in self.list(&scope)? {
                    if info.state == SnapshotState::Applying {
                        found.push((scope.clone(), info));
                    }
                }
            }
        }
        Ok(found)
    }

    pub fn read_file(&self, scope: &Scope, id: &SnapshotId, rel: &str) -> Result<Option<Vec<u8>>> {
        let (dir, info) = self.info(scope, id)?;
        let file = info
            .files
            .iter()
            .find(|file| file.rel == rel)
            .ok_or_else(|| Error::NotFound(format!("{rel} in snapshot {}", id.0)))?;
        if !file.existed {
            return Ok(None);
        }
        let path = dir.join("files").join(rel);
        fs::read(&path)
            .map(Some)
            .map_err(|err| Error::io(path, err))
    }

    /// Restores a snapshot without checking whether the game runs. App code must call
    /// `Writer::restore` instead.
    pub fn restore(
        &self,
        scope: &Scope,
        channel: &Channel,
        id: &SnapshotId,
    ) -> Result<SnapshotInfo> {
        let (dir, info) = self.info(scope, id)?;
        let targets: Vec<PathBuf> = info
            .files
            .iter()
            .map(|file| channel.dir.join(&file.rel))
            .collect();
        let mut before = self.begin(
            scope,
            channel,
            &targets,
            &format!("Before restore of {}", id.0),
        )?;
        put_back(&dir, &info, channel)?;
        self.finish(scope, &before.id)?;
        before.state = SnapshotState::Done;
        if info.state == SnapshotState::Applying {
            self.finish(scope, id)?;
        }
        Ok(before)
    }

    /// Puts back the files of a snapshot whose own apply just failed, without a snapshot first.
    pub(crate) fn roll_back(
        &self,
        scope: &Scope,
        channel: &Channel,
        id: &SnapshotId,
    ) -> Result<()> {
        let (dir, info) = self.info(scope, id)?;
        put_back(&dir, &info, channel)?;
        if info.state == SnapshotState::Applying {
            self.finish(scope, id)?;
        }
        Ok(())
    }

    pub fn delete(&self, scope: &Scope, id: &SnapshotId) -> Result<()> {
        let dir = self.snapshot_dir(scope, id)?;
        match fs::remove_dir_all(&dir) {
            Err(err) if err.kind() == ErrorKind::NotFound => {
                Err(Error::NotFound(format!("snapshot {}", id.0)))
            }
            Err(err) => Err(Error::io(dir, err)),
            Ok(()) => Ok(()),
        }
    }

    pub fn prune(&self, scope: &Scope, keep: usize) -> Result<usize> {
        let stale: Vec<SnapshotInfo> = self
            .list(scope)?
            .into_iter()
            .filter(|info| info.state == SnapshotState::Done)
            .skip(keep)
            .collect();
        for info in &stale {
            self.delete(scope, &info.id)?;
        }
        Ok(stale.len())
    }
}

fn put_back(snapshot_dir: &Path, info: &SnapshotInfo, channel: &Channel) -> Result<()> {
    let mut first_error = None;
    for file in &info.files {
        let target = channel.dir.join(&file.rel);
        if let Err(err) = restore_one(snapshot_dir, file, &target) {
            first_error.get_or_insert(err);
        }
    }
    first_error.map_or(Ok(()), Err)
}

fn restore_one(snapshot_dir: &Path, file: &SnapshotFile, target: &Path) -> Result<()> {
    if !file.existed {
        return safe_write::remove(target);
    }
    let current = safe_write::load(target)?;
    let source = snapshot_dir.join("files").join(&file.rel);
    let saved = fs::read(&source).map_err(|err| Error::io(&source, err))?;
    if current.hash == Some(hash(&saved)) {
        return Ok(());
    }
    write_atomic(target, &saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{TempDir, tempdir};

    struct Fixture {
        _root: TempDir,
        store: BackupStore,
        scope: Scope,
        channel: Channel,
    }

    fn fixture() -> Fixture {
        let root = tempdir().unwrap();
        let channel = Channel {
            name: "LIVE".into(),
            dir: root.path().join("LIVE"),
        };
        fs::create_dir_all(&channel.dir).unwrap();
        Fixture {
            store: BackupStore::new(root.path().join("backups")),
            scope: Scope {
                install_key: "abc".into(),
                channel: "LIVE".into(),
            },
            channel,
            _root: root,
        }
    }

    #[test]
    fn snapshot_copies_files_and_records_missing_ones() {
        let fx = fixture();
        let cfg = fx.channel.dir.join("user.cfg");
        fs::write(&cfg, b"a").unwrap();
        let missing = fx.channel.dir.join("USER/x.xml");
        let info = fx
            .store
            .snapshot(&fx.scope, &fx.channel, &[cfg, missing], "test")
            .unwrap();
        assert_eq!(info.state, SnapshotState::Done);
        assert_eq!(info.label, "test");
        let files: Vec<(&str, bool, u64)> = info
            .files
            .iter()
            .map(|file| (file.rel.as_str(), file.existed, file.size))
            .collect();
        assert_eq!(files, [("user.cfg", true, 1), ("USER/x.xml", false, 0)]);
        assert_eq!(
            fx.store.read_file(&fx.scope, &info.id, "user.cfg").unwrap(),
            Some(b"a".to_vec())
        );
    }

    #[test]
    fn snapshot_rejects_path_outside_channel() {
        let fx = fixture();
        let outside = fx.channel.dir.parent().unwrap().join("other.cfg");
        let err = fx
            .store
            .snapshot(&fx.scope, &fx.channel, &[outside], "x")
            .unwrap_err();
        assert!(matches!(err, Error::Invalid(_)));
        assert!(fx.store.list(&fx.scope).unwrap().is_empty());
    }

    #[test]
    fn list_is_newest_first_and_ids_are_unique_within_one_second() {
        let fx = fixture();
        let ids: Vec<SnapshotId> = (0..3)
            .map(|n| {
                fx.store
                    .snapshot(&fx.scope, &fx.channel, &[], &format!("s{n}"))
                    .unwrap()
                    .id
            })
            .collect();
        let mut unique = ids.clone();
        unique.dedup();
        assert_eq!(unique.len(), 3);
        let listed = fx.store.list(&fx.scope).unwrap();
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].id, ids[2]);
        assert_eq!(listed[2].id, ids[0]);
    }

    #[test]
    fn list_skips_folder_without_manifest() {
        let fx = fixture();
        fx.store
            .snapshot(&fx.scope, &fx.channel, &[], "ok")
            .unwrap();
        let broken = fx.store.root.join("abc/LIVE/20200101T000000Z/files");
        fs::create_dir_all(broken).unwrap();
        assert_eq!(fx.store.list(&fx.scope).unwrap().len(), 1);
    }

    #[test]
    fn begin_is_applying_until_finish_and_shows_in_interrupted() {
        let fx = fixture();
        let info = fx
            .store
            .begin(&fx.scope, &fx.channel, &[], "pending")
            .unwrap();
        assert_eq!(info.state, SnapshotState::Applying);
        let interrupted = fx.store.interrupted().unwrap();
        assert_eq!(interrupted.len(), 1);
        assert_eq!(interrupted[0].0, fx.scope);
        assert_eq!(interrupted[0].1.id, info.id);
        fx.store.finish(&fx.scope, &info.id).unwrap();
        assert!(fx.store.interrupted().unwrap().is_empty());
    }

    #[test]
    fn restore_puts_back_content_and_deletes_files_that_did_not_exist() {
        let fx = fixture();
        let cfg = fx.channel.dir.join("user.cfg");
        let added = fx.channel.dir.join("USER/x.xml");
        fs::write(&cfg, b"old").unwrap();
        let info = fx
            .store
            .snapshot(&fx.scope, &fx.channel, &[cfg.clone(), added.clone()], "s")
            .unwrap();
        fs::write(&cfg, b"changed").unwrap();
        fs::create_dir_all(added.parent().unwrap()).unwrap();
        fs::write(&added, b"new").unwrap();
        fx.store.restore(&fx.scope, &fx.channel, &info.id).unwrap();
        assert_eq!(fs::read(&cfg).unwrap(), b"old");
        assert!(!added.exists());
    }

    #[test]
    fn restore_returns_a_snapshot_that_undoes_it() {
        let fx = fixture();
        let cfg = fx.channel.dir.join("user.cfg");
        fs::write(&cfg, b"one").unwrap();
        let a = fx
            .store
            .snapshot(&fx.scope, &fx.channel, std::slice::from_ref(&cfg), "a")
            .unwrap();
        fs::write(&cfg, b"two").unwrap();
        let b = fx.store.restore(&fx.scope, &fx.channel, &a.id).unwrap();
        assert_eq!(b.label, format!("Before restore of {}", a.id.0));
        assert_eq!(fs::read(&cfg).unwrap(), b"one");
        fx.store.restore(&fx.scope, &fx.channel, &b.id).unwrap();
        assert_eq!(fs::read(&cfg).unwrap(), b"two");
    }

    #[test]
    fn restore_of_applying_snapshot_marks_it_done() {
        let fx = fixture();
        let cfg = fx.channel.dir.join("user.cfg");
        fs::write(&cfg, b"one").unwrap();
        let begun = fx.store.begin(&fx.scope, &fx.channel, &[cfg], "w").unwrap();
        fx.store.restore(&fx.scope, &fx.channel, &begun.id).unwrap();
        assert!(fx.store.interrupted().unwrap().is_empty());
        let listed = fx.store.list(&fx.scope).unwrap();
        assert!(listed.iter().all(|info| info.state == SnapshotState::Done));
    }

    #[test]
    fn manifest_with_escaping_rel_is_unreadable_and_restores_nothing() {
        let root = tempdir().unwrap();
        let channel = Channel {
            name: "LIVE".into(),
            dir: root.path().join("a/b/LIVE"),
        };
        fs::create_dir_all(&channel.dir).unwrap();
        let store = BackupStore::new(root.path().join("backups"));
        let scope = Scope {
            install_key: "abc".into(),
            channel: "LIVE".into(),
        };
        let cfg = channel.dir.join("user.cfg");
        let info = store
            .snapshot(&scope, &channel, std::slice::from_ref(&cfg), "s")
            .unwrap();
        let manifest = store
            .snapshot_dir(&scope, &info.id)
            .unwrap()
            .join("manifest.toml");
        let text = fs::read_to_string(&manifest).unwrap();
        fs::write(&manifest, text.replace("\"user.cfg\"", "\"../../x\"")).unwrap();
        let victim = root.path().join("a/x");
        fs::write(&victim, b"keep").unwrap();
        assert!(store.restore(&scope, &channel, &info.id).is_err());
        assert_eq!(fs::read(&victim).unwrap(), b"keep");
        assert!(store.list(&scope).unwrap().iter().all(|s| s.id != info.id));
    }

    #[cfg(unix)]
    #[test]
    fn failed_restore_leaves_its_own_snapshot_applying_for_recovery() {
        use crate::safe_write::testing::ReadOnly;
        let fx = fixture();
        let cfg = fx.channel.dir.join("user.cfg");
        let locked = fx.channel.dir.join("locked/a.xml");
        fs::create_dir_all(locked.parent().unwrap()).unwrap();
        fs::write(&cfg, b"one").unwrap();
        fs::write(&locked, b"x").unwrap();
        let info = fx
            .store
            .snapshot(&fx.scope, &fx.channel, &[cfg.clone(), locked.clone()], "s")
            .unwrap();
        fs::write(&cfg, b"two").unwrap();
        fs::write(&locked, b"y").unwrap();
        let guard = ReadOnly::new(locked.parent().unwrap());
        assert!(fx.store.restore(&fx.scope, &fx.channel, &info.id).is_err());
        drop(guard);
        let interrupted = fx.store.interrupted().unwrap();
        assert_eq!(interrupted.len(), 1);
        let before = &interrupted[0].1;
        assert_eq!(before.label, format!("Before restore of {}", info.id.0));
        fx.store
            .restore(&fx.scope, &fx.channel, &before.id)
            .unwrap();
        assert_eq!(fs::read(&cfg).unwrap(), b"two");
        assert_eq!(fs::read(&locked).unwrap(), b"y");
    }

    #[test]
    fn roll_back_takes_no_snapshot_and_marks_done() {
        let fx = fixture();
        let cfg = fx.channel.dir.join("user.cfg");
        fs::write(&cfg, b"one").unwrap();
        let begun = fx
            .store
            .begin(&fx.scope, &fx.channel, std::slice::from_ref(&cfg), "w")
            .unwrap();
        fs::write(&cfg, b"two").unwrap();
        fx.store
            .roll_back(&fx.scope, &fx.channel, &begun.id)
            .unwrap();
        assert_eq!(fs::read(&cfg).unwrap(), b"one");
        let listed = fx.store.list(&fx.scope).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].state, SnapshotState::Done);
    }

    #[test]
    fn read_file_returns_none_for_file_that_did_not_exist() {
        let fx = fixture();
        let info = fx
            .store
            .snapshot(
                &fx.scope,
                &fx.channel,
                &[fx.channel.dir.join("user.cfg")],
                "s",
            )
            .unwrap();
        assert_eq!(
            fx.store.read_file(&fx.scope, &info.id, "user.cfg").unwrap(),
            None
        );
    }

    #[test]
    fn prune_keeps_newest() {
        let fx = fixture();
        let ids: Vec<SnapshotId> = (0..5)
            .map(|_| {
                fx.store
                    .snapshot(&fx.scope, &fx.channel, &[], "s")
                    .unwrap()
                    .id
            })
            .collect();
        assert_eq!(fx.store.prune(&fx.scope, 2).unwrap(), 3);
        let listed: Vec<SnapshotId> = fx
            .store
            .list(&fx.scope)
            .unwrap()
            .into_iter()
            .map(|info| info.id)
            .collect();
        assert_eq!(listed, [ids[4].clone(), ids[3].clone()]);
    }

    #[test]
    fn delete_removes_the_snapshot() {
        let fx = fixture();
        let info = fx.store.snapshot(&fx.scope, &fx.channel, &[], "s").unwrap();
        fx.store.delete(&fx.scope, &info.id).unwrap();
        assert!(fx.store.list(&fx.scope).unwrap().is_empty());
    }
}
