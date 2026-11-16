//! Single instance lock.

use crate::dirs::AppDirs;
use crate::error::{Error, Result};
use std::fs::{self, File, TryLockError};

pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    pub fn acquire(dirs: &AppDirs) -> Result<Option<InstanceLock>> {
        fs::create_dir_all(&dirs.data).map_err(|err| Error::io(&dirs.data, err))?;
        let path = dirs.data.join("star-config.lock");
        let file = File::options()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|err| Error::io(&path, err))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(InstanceLock { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(err)) => Err(Error::io(&path, err)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn second_acquire_returns_none_until_first_is_dropped() {
        let root = tempdir().unwrap();
        let dirs = AppDirs::under(root.path());
        let first = InstanceLock::acquire(&dirs).unwrap();
        assert!(first.is_some());
        assert!(InstanceLock::acquire(&dirs).unwrap().is_none());
        drop(first);
        assert!(InstanceLock::acquire(&dirs).unwrap().is_some());
    }
}
