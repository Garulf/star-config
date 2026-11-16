//! Atomic file writes and change detection.

use crate::error::{Error, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

pub type Hash = [u8; 32];

pub fn hash(bytes: &[u8]) -> Hash {
    Sha256::digest(bytes).into()
}

#[derive(Debug, Clone)]
pub struct LoadedFile {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub hash: Option<Hash>,
}

pub fn load(path: &Path) -> Result<LoadedFile> {
    match fs::read(path) {
        Ok(bytes) => Ok(LoadedFile {
            path: path.to_path_buf(),
            hash: Some(hash(&bytes)),
            bytes,
        }),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(LoadedFile {
            path: path.to_path_buf(),
            bytes: Vec::new(),
            hash: None,
        }),
        Err(err) => Err(Error::io(path, err)),
    }
}

fn link_destination(path: &Path) -> PathBuf {
    if let Ok(real) = fs::canonicalize(path) {
        return real;
    }
    match fs::read_link(path) {
        Ok(target) => path
            .parent()
            .map_or(target.clone(), |dir| dir.join(&target)),
        Err(_) => path.to_path_buf(),
    }
}

fn temp_path(target: &Path) -> PathBuf {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    target.with_file_name(format!(".{name}.star-config.tmp"))
}

fn write_temp(temp: &Path, target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = fs::File::create(temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    if let Ok(meta) = fs::metadata(target) {
        let _ = fs::set_permissions(temp, meta.permissions());
    }
    Ok(())
}

#[cfg(unix)]
fn sync_dir(dir: &Path) {
    if let Ok(handle) = fs::File::open(dir) {
        let _ = handle.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) {}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let target = link_destination(path);
    let parent = target.parent().filter(|dir| !dir.as_os_str().is_empty());
    if let Some(dir) = parent {
        fs::create_dir_all(dir).map_err(|err| Error::io(path, err))?;
    }
    let temp = temp_path(&target);
    let written = write_temp(&temp, &target, bytes).and_then(|()| fs::rename(&temp, &target));
    if let Err(err) = written {
        let _ = fs::remove_file(&temp);
        return Err(Error::io(path, err));
    }
    if let Some(dir) = parent {
        sync_dir(dir);
    }
    Ok(())
}

pub fn remove(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(err) if err.kind() != ErrorKind::NotFound => Err(Error::io(path, err)),
        _ => Ok(()),
    }
}

pub fn check_writable(dir: &Path) -> Result<()> {
    let probe = dir.join(".star-config-probe.tmp");
    let renamed = dir.join(".star-config-probe.renamed");
    let outcome = fs::File::create(&probe).and_then(|_| fs::rename(&probe, &renamed));
    let _ = fs::remove_file(&probe);
    let _ = fs::remove_file(&renamed);
    outcome.map_err(|err| Error::io(dir, err))
}

#[cfg(test)]
pub(crate) mod testing {
    use std::path::{Path, PathBuf};

    #[cfg(unix)]
    pub struct ReadOnly {
        path: PathBuf,
        original: std::fs::Permissions,
    }

    #[cfg(unix)]
    impl ReadOnly {
        pub fn new(path: &Path) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let original = std::fs::metadata(path).unwrap().permissions();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o555)).unwrap();
            Self {
                path: path.to_path_buf(),
                original,
            }
        }
    }

    #[cfg(unix)]
    impl Drop for ReadOnly {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.path, self.original.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn load_missing_file_has_no_hash() {
        let dir = tempdir().unwrap();
        let loaded = load(&dir.path().join("user.cfg")).unwrap();
        assert_eq!(loaded.hash, None);
        assert!(loaded.bytes.is_empty());
    }

    #[test]
    fn write_atomic_creates_parents_and_preserves_bytes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a/b/user.cfg");
        let bytes = b"a\r\n\xEF\xBB\xBFb";
        write_atomic(&path, bytes).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.bytes, bytes);
        assert_eq!(loaded.hash, Some(hash(bytes)));
    }

    #[test]
    fn write_atomic_replaces_existing_and_leaves_no_temp() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("user.cfg");
        fs::write(&path, b"old").unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(names(dir.path()), ["user.cfg"]);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_into_read_only_dir_fails_and_keeps_original() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("user.cfg");
        fs::write(&path, b"old").unwrap();
        let _guard = testing::ReadOnly::new(dir.path());
        let err = write_atomic(&path, b"new").unwrap_err();
        assert!(matches!(&err, Error::Io { path: failed, .. } if *failed == path));
        assert_eq!(fs::read(&path).unwrap(), b"old");
        assert_eq!(names(dir.path()), ["user.cfg"]);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_keeps_a_symlinked_target_as_a_link() {
        let dir = tempdir().unwrap();
        let real = dir.path().join("elsewhere/real.cfg");
        write_atomic(&real, b"old").unwrap();
        let link = dir.path().join("user.cfg");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_atomic(&link, b"new").unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(fs::read(&real).unwrap(), b"new");
        assert_eq!(names(dir.path()), ["elsewhere", "user.cfg"]);
    }

    #[test]
    fn remove_is_ok_when_absent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("user.cfg");
        remove(&path).unwrap();
        fs::write(&path, b"x").unwrap();
        remove(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn check_writable_ok_in_tempdir_and_err_in_read_only_dir() {
        let dir = tempdir().unwrap();
        check_writable(dir.path()).unwrap();
        assert!(names(dir.path()).is_empty());
        #[cfg(unix)]
        {
            let _guard = testing::ReadOnly::new(dir.path());
            assert!(matches!(check_writable(dir.path()), Err(Error::Io { .. })));
            assert!(names(dir.path()).is_empty());
        }
    }
}
