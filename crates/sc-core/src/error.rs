use crate::backup::SnapshotId;
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}", path = path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Star Citizen is running")]
    GameRunning,
    #[error("{path} changed on disk since it was loaded", path = path.display())]
    ChangedOnDisk { path: PathBuf },
    #[error("{path}, line {line}: {message}", path = path.display())]
    Parse {
        path: PathBuf,
        line: usize,
        message: String,
    },
    #[error("{0} not found")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("network: {0}")]
    Network(String),
    #[error("{first} and {second} differ only by case", first = first.display(), second = second.display())]
    AmbiguousCase { first: PathBuf, second: PathBuf },
    #[error(
        "{write}. Putting the earlier versions back also failed: {restore}. The files may now be a mix of old and new versions; snapshot {id} holds the originals.",
        id = snapshot.0
    )]
    RollbackFailed {
        write: Box<Error>,
        restore: Box<Error>,
        snapshot: SnapshotId,
    },
}

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    #[test]
    fn io_error_names_the_path_and_keeps_the_source() {
        let err = Error::io(
            "/tmp/user.cfg",
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );
        assert!(err.to_string().starts_with("/tmp/user.cfg: "));
        assert!(err.source().is_some());
    }

    #[test]
    fn parse_error_reports_line() {
        let err = Error::Parse {
            path: "a.xml".into(),
            line: 7,
            message: "bad".into(),
        };
        assert_eq!(err.to_string(), "a.xml, line 7: bad");
    }

    #[test]
    fn rollback_failure_names_both_errors_and_the_snapshot() {
        let err = Error::RollbackFailed {
            write: Box::new(Error::Invalid("disk full".into())),
            restore: Box::new(Error::Invalid("still full".into())),
            snapshot: SnapshotId("20261115T120000Z".into()),
        };
        let text = err.to_string();
        assert!(text.starts_with("disk full. "));
        assert!(text.contains("still full"));
        assert!(text.contains("mix of old and new"));
        assert!(text.contains("snapshot 20261115T120000Z holds the originals"));
    }
}
