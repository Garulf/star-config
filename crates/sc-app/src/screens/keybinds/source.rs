//! The files the screen reads and writes. Everything here runs off the UI thread.

use crate::screens::backups::words;
use sc_core::actionmaps::ActionMaps;
use sc_core::backup::{Scope, SnapshotId};
use sc_core::error::Error;
use sc_core::guard::{Change, Writer};
use sc_core::install::Channel;
use sc_core::safe_write;
use sc_core::staging::{Document, Staged};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const CURRENT_LABEL: &str = "Current binds";
pub const UNRECOGNISED: &str =
    "This file's format is not recognised, so star-config will not change it.";
const LABEL: &str = "Keybind changes";
const NOT_RECOGNISED_PREFIX: &str = "format not recognised: ";
const LAYOUT_PREFIX: &str = "layout_";
const LAYOUT_SUFFIX: &str = "_exported.xml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Current,
    Layout(PathBuf),
}

impl Source {
    pub fn path(&self, channel: &Channel) -> PathBuf {
        match self {
            Source::Current => channel.actionmaps_xml(),
            Source::Layout(path) => path.clone(),
        }
    }

    pub fn is_layout(&self) -> bool {
        matches!(self, Source::Layout(_))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceChoice {
    pub source: Source,
    pub label: String,
}

impl fmt::Display for SourceChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// Carries a staged document through a message, which must be `Clone`, with how many
/// edits were sent to be written.
#[derive(Clone)]
pub struct Handoff(Arc<Mutex<Option<Staged<ActionMaps>>>>, usize);

impl Handoff {
    fn new(staged: Staged<ActionMaps>, count: usize) -> Self {
        Self(Arc::new(Mutex::new(Some(staged))), count)
    }

    pub fn count(&self) -> usize {
        self.1
    }

    pub fn take(&self) -> Option<Staged<ActionMaps>> {
        self.0.lock().ok()?.take()
    }
}

impl fmt::Debug for Handoff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Handoff")
    }
}

#[derive(Debug, Clone)]
pub struct Failure {
    pub banner: String,
    pub unrecognised: bool,
    pub raw: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Opened {
    /// `writable` is false when writing the file back unchanged would not give the same bytes.
    Ready {
        doc: Handoff,
        writable: bool,
    },
    Failed(Failure),
}

#[derive(Debug, Clone)]
pub struct Loaded {
    pub sources: Vec<SourceChoice>,
    pub source: Option<Source>,
    pub opened: Option<Opened>,
}

/// The layout's name from its file name: `layout_hotas-nov_exported.xml` gives `hotas-nov`.
pub fn layout_name(file_name: &str) -> String {
    let stem = file_name
        .strip_prefix(LAYOUT_PREFIX)
        .and_then(|rest| rest.strip_suffix(LAYOUT_SUFFIX))
        .filter(|name| !name.is_empty());
    match stem {
        Some(name) => name.to_string(),
        None => file_name
            .strip_suffix(".xml")
            .unwrap_or(file_name)
            .to_string(),
    }
}

fn is_xml(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("xml"))
}

pub fn list(channel: &Channel) -> Vec<SourceChoice> {
    let mut sources = Vec::new();
    if channel.actionmaps_xml().is_file() {
        sources.push(SourceChoice {
            source: Source::Current,
            label: CURRENT_LABEL.to_string(),
        });
    }
    let mut layouts: Vec<PathBuf> = fs::read_dir(channel.mappings_dir())
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| is_xml(path))
                .collect()
        })
        .unwrap_or_default();
    layouts.sort();
    sources.extend(layouts.into_iter().map(|path| SourceChoice {
        label: layout_name(&path.file_name().unwrap_or_default().to_string_lossy()),
        source: Source::Layout(path),
    }));
    sources
}

pub fn load(channel: &Channel, wanted: Option<Source>) -> Loaded {
    let sources = list(channel);
    let source = wanted
        .filter(|wanted| sources.iter().any(|choice| &choice.source == wanted))
        .or_else(|| sources.first().map(|choice| choice.source.clone()));
    let opened = source.as_ref().map(|source| open(&source.path(channel)));
    Loaded {
        sources,
        source,
        opened,
    }
}

/// Whether writing the parsed file back unchanged gives the bytes on disk.
pub fn round_trips(staged: &Staged<ActionMaps>) -> bool {
    staged.base().to_bytes() == staged.loaded().bytes
}

pub fn open(path: &Path) -> Opened {
    match Staged::<ActionMaps>::open(path) {
        Ok(staged) => {
            let writable = round_trips(&staged);
            Opened::Ready {
                doc: Handoff::new(staged, 0),
                writable,
            }
        }
        Err(err) => Opened::Failed(failure(path, &err)),
    }
}

fn failure(path: &Path, err: &Error) -> Failure {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Error::Parse { line, message, .. } = err else {
        return Failure {
            banner: format!("Could not read {name}: {}", words(err)),
            unrecognised: false,
            raw: Vec::new(),
        };
    };
    let raw = fs::read(path)
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .lines()
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    match message.strip_prefix(NOT_RECOGNISED_PREFIX) {
        Some(reason) => Failure {
            banner: format!("{UNRECOGNISED} Line {line}: {reason}."),
            unrecognised: true,
            raw,
        },
        None => Failure {
            banner: format!("{name} could not be read. Line {line}: {message}."),
            unrecognised: false,
            raw,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteFailure {
    ChangedAgain,
    NoLongerApply(usize),
    Unrecognised,
    Exists,
    RollbackFailed,
    GameRunning,
    Failed(String),
}

impl From<Error> for WriteFailure {
    fn from(err: Error) -> Self {
        match err {
            Error::ChangedOnDisk { .. } => WriteFailure::ChangedAgain,
            Error::RollbackFailed { .. } => WriteFailure::RollbackFailed,
            Error::GameRunning => WriteFailure::GameRunning,
            other => WriteFailure::Failed(words(&other)),
        }
    }
}

/// Writes the staged edits. When the file changed on disk since it was read, the edits
/// are replayed on the new file and the write is tried once more.
pub fn apply(
    mut staged: Staged<ActionMaps>,
    count: usize,
    writer: Writer,
    scope: Scope,
    channel: Channel,
) -> (Handoff, Result<SnapshotId, WriteFailure>) {
    let result = match staged.commit(&writer, &scope, &channel, LABEL) {
        Err(Error::ChangedOnDisk { .. }) => match staged.reload_and_replay() {
            Ok(_) if !round_trips(&staged) => {
                return (Handoff::new(staged, count), Err(WriteFailure::Unrecognised));
            }
            Ok(0) => staged.commit(&writer, &scope, &channel, LABEL),
            Ok(dropped) => {
                return (
                    Handoff::new(staged, count),
                    Err(WriteFailure::NoLongerApply(dropped)),
                );
            }
            Err(err) => Err(err),
        },
        other => other,
    };
    let result = result.map(|info| info.id).map_err(WriteFailure::from);
    (Handoff::new(staged, count), result)
}

/// Writes `bytes` as the layout file `target`, refusing to replace one unless `overwrite`.
pub fn save_layout(
    bytes: Vec<u8>,
    target: PathBuf,
    overwrite: bool,
    writer: Writer,
    scope: Scope,
    channel: Channel,
) -> Result<SnapshotId, WriteFailure> {
    let loaded = safe_write::load(&target).map_err(WriteFailure::from)?;
    if loaded.hash.is_some() && !overwrite {
        return Err(WriteFailure::Exists);
    }
    let change = Change {
        loaded: &loaded,
        new_bytes: Some(&bytes),
    };
    writer
        .apply(&scope, &channel, &[change], LABEL)
        .map(|info| info.id)
        .map_err(WriteFailure::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_names_come_from_file_names() {
        assert_eq!(layout_name("layout_hotas-nov_exported.xml"), "hotas-nov");
        assert_eq!(layout_name("my_binds.xml"), "my_binds");
        assert_eq!(layout_name("layout__exported.xml"), "layout__exported");
    }
}
