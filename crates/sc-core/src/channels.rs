//! Channel management.

use crate::backup::{Scope, SnapshotInfo};
use crate::error::{Error, Result};
use crate::guard::{Change, Target, Writer};
use crate::install::{Channel, Install, canonical_or_raw, resolve_ci, resolve_ci_or_default};
use crate::process::{GameProbe, RunState};
use crate::safe_write::{self, LoadedFile, check_writable, write_atomic};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;
use sysinfo::{DiskRefreshKind, Disks};
use walkdir::WalkDir;

pub const KNOWN: [&str; 5] = ["LIVE", "PTU", "EPTU", "HOTFIX", "TECH-PREVIEW"];

const P4K: &str = "Data.p4k";
const CHUNK: usize = 4 * 1024 * 1024;
const PART_SUFFIX: &str = ".star-config.part";
const SWAP_DIR: &str = ".star-config-swap";
const SWAP_SUFFIX: &str = ".star-config-swap";
const DATA: &str = "data";
const LOCALIZATION: &str = "Localization";
const SESSION_FILES: [&str; 5] = [
    "Game.log",
    "logbackups",
    "logs",
    "Crashes",
    "loginData.json",
];

// classification and validation

fn normal_parts(rel: &Path) -> Option<Vec<&OsStr>> {
    rel.components()
        .map(|part| match part {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect()
}

fn is_named(name: &OsStr, expected: &str) -> bool {
    name.to_str()
        .is_some_and(|name| name.eq_ignore_ascii_case(expected))
}

pub fn is_user_path(rel: &Path) -> bool {
    let Some(parts) = normal_parts(rel) else {
        return false;
    };
    match parts.as_slice() {
        [first, ..]
            if ["user.cfg", "USER", "ScreenShots"]
                .iter()
                .any(|n| is_named(first, n)) =>
        {
            true
        }
        [first, second, ..] => is_named(first, DATA) && is_named(second, LOCALIZATION),
        _ => false,
    }
}

/// Logs, crash dumps and the login session belong to the channel that wrote them, so a game file
/// copy leaves the target's own. A swap renames whole folders, so they travel with their folder.
fn is_session_path(rel: &Path) -> bool {
    normal_parts(rel)
        .and_then(|parts| parts.first().copied())
        .is_some_and(|first| SESSION_FILES.iter().any(|name| is_named(first, name)))
}

fn is_game_file_path(rel: &Path) -> bool {
    !is_user_path(rel) && !is_session_path(rel)
}

fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

fn check_channel_name(name: &str) -> Result<()> {
    let plain = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\'])
        && matches!(
            Path::new(name).components().collect::<Vec<_>>().as_slice(),
            [Component::Normal(_)]
        );
    if plain {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "{name:?} is not a channel folder name"
        )))
    }
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file())
}

fn exists_no_follow(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn existing_channel_dir(install: &Install, name: &str) -> Result<Option<PathBuf>> {
    let Some(dir) = resolve_ci(&install.root, name) else {
        return Ok(None);
    };
    if is_symlink(&dir) {
        return Err(Error::Invalid(format!(
            "{} is a link; channel folders must be real folders",
            dir.display()
        )));
    }
    if !is_real_dir(&dir) {
        return Err(Error::Invalid(format!("{} is not a folder", dir.display())));
    }
    Ok(Some(dir))
}

fn has_p4k(dir: &Path) -> bool {
    resolve_ci(dir, P4K).is_some_and(|p4k| p4k.is_file())
}

fn canonical_dir(install: &Install, dir: &Path) -> Result<PathBuf> {
    if dir.exists() {
        return fs::canonicalize(dir).map_err(|err| Error::io(dir, err));
    }
    let root = fs::canonicalize(&install.root).map_err(|err| Error::io(&install.root, err))?;
    Ok(root.join(dir.file_name().unwrap_or_default()))
}

pub fn validate_pair(install: &Install, source: &str, target: &str) -> Result<(PathBuf, PathBuf)> {
    check_channel_name(source)?;
    check_channel_name(target)?;
    if same_name(source, target) {
        return Err(Error::Invalid(format!(
            "{source} and {target} are the same channel"
        )));
    }
    let source_dir = existing_channel_dir(install, source)?
        .ok_or_else(|| Error::Invalid(format!("channel folder {source} not found")))?;
    if !has_p4k(&source_dir) {
        return Err(Error::Invalid(format!(
            "{} holds no {P4K}",
            source_dir.display()
        )));
    }
    let target_dir = match existing_channel_dir(install, target)? {
        Some(dir) => dir,
        None => {
            let known = KNOWN
                .iter()
                .find(|known| same_name(known, target))
                .ok_or_else(|| {
                    Error::Invalid(format!(
                        "channel folder {target} not found and not a known channel name"
                    ))
                })?;
            install.root.join(known)
        }
    };
    let (canon_source, canon_target) = (
        canonical_dir(install, &source_dir)?,
        canonical_dir(install, &target_dir)?,
    );
    if canon_source.starts_with(&canon_target) || canon_target.starts_with(&canon_source) {
        return Err(Error::Invalid(format!(
            "{} and {} overlap",
            source_dir.display(),
            target_dir.display()
        )));
    }
    Ok((source_dir, target_dir))
}

pub fn require_closed(probe: &dyn GameProbe) -> Result<()> {
    if probe.game() == RunState::NotRunning && probe.launcher() == RunState::NotRunning {
        Ok(())
    } else {
        Err(Error::GameRunning)
    }
}

// copy

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CopyOptions {
    pub compare_contents: bool,
    pub in_place: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    pub rel: PathBuf,
    pub size: u64,
    pub copy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyPlan {
    pub source: PathBuf,
    pub target: PathBuf,
    pub files: Vec<PlannedFile>,
    pub bytes_to_copy: u64,
    pub largest_copy: u64,
    pub free_bytes: Option<u64>,
    pub needs_in_place: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyOutcome {
    pub files_copied: usize,
    pub bytes_copied: u64,
    pub cancelled: bool,
}

fn walk_error(root: &Path, err: walkdir::Error) -> Error {
    let path = err.path().unwrap_or(root).to_path_buf();
    Error::io(path, err.into())
}

fn rel_text(rel: &Path) -> String {
    let parts: Vec<String> = rel
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.join("/")
}

fn source_files(source: &Path) -> Result<Vec<(PathBuf, fs::Metadata)>> {
    let walk = WalkDir::new(source)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            let rel = entry.path().strip_prefix(source).unwrap_or(Path::new(""));
            rel.as_os_str().is_empty() || is_game_file_path(rel)
        });
    let mut files = Vec::new();
    for entry in walk {
        let entry = entry.map_err(|err| walk_error(source, err))?;
        let is_part = entry.file_name().to_string_lossy().ends_with(PART_SUFFIX);
        if !entry.file_type().is_file() || is_part {
            continue;
        }
        let meta = entry.metadata().map_err(|err| walk_error(source, err))?;
        let rel = entry.path().strip_prefix(source).unwrap_or(Path::new(""));
        files.push((rel.to_path_buf(), meta));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

fn file_digest(path: &Path) -> Result<[u8; 32]> {
    let mut file = File::open(path).map_err(|err| Error::io(path, err))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = file.read(&mut buffer).map_err(|err| Error::io(path, err))?;
        if read == 0 {
            return Ok(hasher.finalize().into());
        }
        hasher.update(&buffer[..read]);
    }
}

fn target_matches(
    source_file: &Path,
    source_meta: &fs::Metadata,
    target_file: &Path,
    options: CopyOptions,
) -> Result<bool> {
    let Ok(target_meta) = fs::symlink_metadata(target_file) else {
        return Ok(false);
    };
    if !target_meta.is_file() || target_meta.len() != source_meta.len() {
        return Ok(false);
    }
    if options.compare_contents {
        return Ok(file_digest(source_file)? == file_digest(target_file)?);
    }
    let modified = source_meta.modified().ok();
    Ok(modified.is_some() && modified == target_meta.modified().ok())
}

fn existing_size(path: &Path) -> u64 {
    fs::symlink_metadata(path)
        .ok()
        .filter(fs::Metadata::is_file)
        .map_or(0, |meta| meta.len())
}

fn free_space(path: &Path) -> Option<u64> {
    let existing = path.ancestors().find(|dir| dir.exists())?;
    let real = fs::canonicalize(existing).ok()?;
    let disks =
        Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing().with_storage()).ok()?;
    let mounts: Vec<(String, u64)> = disks
        .iter()
        .map(|disk| {
            let mount = disk.mount_point().to_string_lossy().into_owned();
            (mount, disk.available_space())
        })
        .collect();
    free_on_mount(&real.to_string_lossy(), &mounts, cfg!(windows))
}

/// Path parts for mount matching. On Windows, `canonicalize` returns verbatim paths
/// (`\\?\C:\x`, `\\?\UNC\server\share`) while mount points are plain (`C:\`).
fn mount_parts(path: &str, windows: bool) -> Vec<String> {
    if !windows {
        return path
            .split('/')
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect();
    }
    let plain = match path.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => path.strip_prefix(r"\\?\").unwrap_or(path).to_string(),
    };
    plain
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn free_on_mount(path: &str, mounts: &[(String, u64)], windows: bool) -> Option<u64> {
    let parts = mount_parts(path, windows);
    mounts
        .iter()
        .map(|(mount, free)| (mount_parts(mount, windows), *free))
        .filter(|(mount, _)| parts.starts_with(mount))
        .max_by_key(|(mount, _)| mount.len())
        .map(|(_, free)| free)
}

fn space_needs_in_place(bytes_to_copy: u64, replaced: u64, free: Option<u64>) -> Result<bool> {
    let Some(free) = free else {
        return Ok(false);
    };
    let growth = bytes_to_copy.saturating_sub(replaced);
    if free < growth {
        return Err(Error::Invalid(format!(
            "the copy needs {growth} more bytes but only {free} bytes are free on the target disk"
        )));
    }
    Ok(free < bytes_to_copy)
}

pub fn plan_copy(
    install: &Install,
    source: &str,
    target: &str,
    options: CopyOptions,
) -> Result<CopyPlan> {
    let (source, target) = validate_pair(install, source, target)?;
    let mut files = Vec::new();
    let (mut bytes_to_copy, mut largest_copy, mut replaced) = (0u64, 0u64, 0u64);
    for (rel, meta) in source_files(&source)? {
        let destination = resolve_ci_or_default(&target, &rel_text(&rel));
        let copy = !target_matches(&source.join(&rel), &meta, &destination, options)?;
        if copy {
            bytes_to_copy += meta.len();
            largest_copy = largest_copy.max(meta.len());
            replaced += existing_size(&destination);
        }
        files.push(PlannedFile {
            rel,
            size: meta.len(),
            copy,
        });
    }
    let free_bytes = free_space(&target);
    let needs_in_place = space_needs_in_place(bytes_to_copy, replaced, free_bytes)?;
    Ok(CopyPlan {
        source,
        target,
        files,
        bytes_to_copy,
        largest_copy,
        free_bytes,
        needs_in_place,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reflink {
    Try,
    #[cfg_attr(not(test), allow(dead_code))]
    Skip,
}

struct Job {
    source: PathBuf,
    destination: PathBuf,
    meta: fs::Metadata,
}

struct Copier<'a> {
    cancel: &'a AtomicBool,
    progress: &'a mut dyn FnMut(Progress),
    reflink: Reflink,
    state: Progress,
}

enum FileResult {
    Copied(u64),
    Cancelled,
}

fn invalid_plan(plan: &CopyPlan, why: &str) -> Error {
    Error::Invalid(format!(
        "copy plan from {} to {}: {why}",
        plan.source.display(),
        plan.target.display()
    ))
}

fn check_no_links_between(base: &Path, path: &Path) -> Result<()> {
    let rel = path.strip_prefix(base).unwrap_or(Path::new(""));
    let mut current = base.to_path_buf();
    for part in rel.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(Error::Invalid(format!(
                    "{} is a link; star-config does not write through links",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    Ok(())
}

fn check_destination(target: &Path, destination: &Path) -> Result<()> {
    check_no_links_between(target, destination)?;
    match fs::symlink_metadata(destination) {
        Ok(meta) if !meta.is_file() => Err(Error::Invalid(format!(
            "{} is in the way and is not a file",
            destination.display()
        ))),
        _ => Ok(()),
    }
}

fn prepare_jobs(plan: &CopyPlan) -> Result<Vec<Job>> {
    if is_symlink(&plan.source) || !is_real_dir(&plan.source) {
        return Err(invalid_plan(plan, "the source is not a real folder"));
    }
    if exists_no_follow(&plan.target) && !is_real_dir(&plan.target) {
        return Err(invalid_plan(plan, "the target is not a real folder"));
    }
    let mut jobs = Vec::new();
    for planned in plan.files.iter().filter(|file| file.copy) {
        let plain = normal_parts(&planned.rel).is_some_and(|parts| !parts.is_empty());
        if !plain || !is_game_file_path(&planned.rel) {
            return Err(invalid_plan(
                plan,
                &format!("{} is not a game file path", planned.rel.display()),
            ));
        }
        let source = plan.source.join(&planned.rel);
        let meta = fs::symlink_metadata(&source).map_err(|err| Error::io(&source, err))?;
        if !meta.is_file() {
            return Err(Error::Invalid(format!(
                "{} is no longer a regular file",
                source.display()
            )));
        }
        let destination = resolve_ci_or_default(&plan.target, &rel_text(&planned.rel));
        check_destination(&plan.target, &destination)?;
        jobs.push(Job {
            source,
            destination,
            meta,
        });
    }
    Ok(jobs)
}

fn open_target_folder(plan: &CopyPlan) -> Result<()> {
    if !exists_no_follow(&plan.target) {
        fs::create_dir(&plan.target).map_err(|err| Error::io(&plan.target, err))?;
    }
    check_writable(&plan.target)
}

pub fn run_copy(
    plan: &CopyPlan,
    options: CopyOptions,
    probe: &dyn GameProbe,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
) -> Result<CopyOutcome> {
    run_copy_with(plan, options, probe, cancel, progress, Reflink::Try)
}

fn run_copy_with(
    plan: &CopyPlan,
    options: CopyOptions,
    probe: &dyn GameProbe,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
    reflink: Reflink,
) -> Result<CopyOutcome> {
    require_closed(probe)?;
    if plan.needs_in_place && !options.in_place {
        return Err(invalid_plan(
            plan,
            "there is not enough space to stage files; allow in-place copying",
        ));
    }
    let jobs = prepare_jobs(plan)?;
    let mut outcome = CopyOutcome {
        files_copied: 0,
        bytes_copied: 0,
        cancelled: false,
    };
    if cancel.load(Ordering::SeqCst) {
        outcome.cancelled = true;
        return Ok(outcome);
    }
    require_closed(probe)?;
    open_target_folder(plan)?;
    let mut copier = Copier {
        cancel,
        progress,
        reflink,
        state: Progress {
            files_done: 0,
            files_total: jobs.len(),
            bytes_done: 0,
            bytes_total: plan.bytes_to_copy,
        },
    };
    for job in &jobs {
        if cancel.load(Ordering::SeqCst) {
            outcome.cancelled = true;
            break;
        }
        let started_at = copier.state.bytes_done;
        let result = if options.in_place {
            copier.copy_in_place(plan, job)?
        } else {
            copier.copy_staged(plan, job)?
        };
        match result {
            FileResult::Copied(bytes) => {
                outcome.files_copied += 1;
                outcome.bytes_copied += bytes;
                copier.state.files_done += 1;
                copier.state.bytes_done = started_at + bytes;
                copier.report();
            }
            FileResult::Cancelled => {
                outcome.cancelled = true;
                break;
            }
        }
    }
    Ok(outcome)
}

fn part_path(destination: &Path) -> PathBuf {
    let name = destination
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    destination.with_file_name(format!("{name}{PART_SUFFIX}"))
}

fn remove_stale_part(part: &Path) -> Result<()> {
    match fs::symlink_metadata(part) {
        Ok(meta) if meta.is_dir() => Err(Error::Invalid(format!(
            "{} is in the way and is a folder",
            part.display()
        ))),
        Ok(_) => fs::remove_file(part).map_err(|err| Error::io(part, err)),
        Err(_) => Ok(()),
    }
}

fn create_parents(plan: &CopyPlan, destination: &Path) -> Result<()> {
    let parent = destination.parent().unwrap_or(&plan.target);
    check_no_links_between(&plan.target, parent)?;
    fs::create_dir_all(parent).map_err(|err| Error::io(parent, err))?;
    check_no_links_between(&plan.target, parent)
}

fn finish_file(file: &File, path: &Path, modified: Option<SystemTime>) -> Result<()> {
    if let Some(time) = modified {
        file.set_modified(time)
            .map_err(|err| Error::io(path, err))?;
    }
    file.sync_all().map_err(|err| Error::io(path, err))
}

impl Copier<'_> {
    fn report(&mut self) {
        (self.progress)(self.state);
    }

    fn try_reflink(&self, source: &Path, destination: &Path) -> bool {
        if self.reflink == Reflink::Skip {
            return false;
        }
        match reflink_copy::reflink(source, destination) {
            Ok(()) => true,
            Err(err) => {
                if err.kind() != ErrorKind::AlreadyExists && is_regular_file(destination) {
                    let _ = fs::remove_file(destination);
                }
                false
            }
        }
    }

    fn clone_into(
        &self,
        source: &Path,
        destination: &Path,
        modified: Option<SystemTime>,
    ) -> Result<bool> {
        if !self.try_reflink(source, destination) {
            return Ok(false);
        }
        match File::options().write(true).open(destination) {
            Ok(file) => finish_file(&file, destination, modified).map(|()| true),
            Err(_) => fs::remove_file(destination)
                .map(|()| false)
                .map_err(|err| Error::io(destination, err)),
        }
    }

    fn stream(&mut self, source: &Path, into: &mut File, into_path: &Path) -> Result<FileResult> {
        let mut reader = File::open(source).map_err(|err| Error::io(source, err))?;
        let mut buffer = vec![0u8; CHUNK];
        let mut copied = 0u64;
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                return Ok(FileResult::Cancelled);
            }
            let read = reader
                .read(&mut buffer)
                .map_err(|err| Error::io(source, err))?;
            if read == 0 {
                return Ok(FileResult::Copied(copied));
            }
            into.write_all(&buffer[..read])
                .map_err(|err| Error::io(into_path, err))?;
            copied += read as u64;
            self.state.bytes_done += read as u64;
            self.report();
        }
    }

    fn copy_staged(&mut self, plan: &CopyPlan, job: &Job) -> Result<FileResult> {
        create_parents(plan, &job.destination)?;
        let part = part_path(&job.destination);
        remove_stale_part(&part)?;
        let result = self.fill_part(job, &part);
        if !matches!(result, Ok(FileResult::Copied(_))) {
            let _ = fs::remove_file(&part);
            return result;
        }
        check_destination(&plan.target, &job.destination)
            .and_then(|()| {
                fs::rename(&part, &job.destination).map_err(|err| Error::io(&job.destination, err))
            })
            .inspect_err(|_| {
                let _ = fs::remove_file(&part);
            })?;
        result
    }

    fn fill_part(&mut self, job: &Job, part: &Path) -> Result<FileResult> {
        let modified = job.meta.modified().ok();
        if self.clone_into(&job.source, part, modified)? {
            return Ok(FileResult::Copied(job.meta.len()));
        }
        let mut file = File::options()
            .write(true)
            .create_new(true)
            .open(part)
            .map_err(|err| Error::io(part, err))?;
        let result = self.stream(&job.source, &mut file, part)?;
        if matches!(result, FileResult::Copied(_)) {
            finish_file(&file, part, modified)?;
        }
        Ok(result)
    }

    fn copy_in_place(&mut self, plan: &CopyPlan, job: &Job) -> Result<FileResult> {
        create_parents(plan, &job.destination)?;
        check_destination(&plan.target, &job.destination)?;
        let modified = job.meta.modified().ok();
        let destination = &job.destination;
        if !exists_no_follow(destination) && self.clone_into(&job.source, destination, modified)? {
            return Ok(FileResult::Copied(job.meta.len()));
        }
        let mut file = File::options()
            .write(true)
            .create(true)
            .truncate(true)
            .open(destination)
            .map_err(|err| Error::io(destination, err))?;
        let result = self.stream(&job.source, &mut file, destination)?;
        if matches!(result, FileResult::Copied(_)) {
            finish_file(&file, destination, modified)?;
        }
        Ok(result)
    }
}

// swap

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Step {
    Rename { from: PathBuf, to: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwapJournal {
    pub install_root: PathBuf,
    pub a: String,
    pub b: String,
    pub steps: Vec<Step>,
    pub done: usize,
    /// The identity of the entry each step moves, taken before the first rename.
    #[serde(default)]
    pub ids: Vec<Option<EntryId>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resume {
    Finish,
    Undo,
}

pub fn journal_path(journal_dir: &Path, install: &Install) -> PathBuf {
    journal_dir.join(format!("swap-{}.json", install.key()))
}

/// `[name]` for a top-level user entry, `[data, Localization]` for the localization folder.
type Entry = Vec<OsString>;

fn child_names(dir: &Path) -> Result<Vec<OsString>> {
    let entries = fs::read_dir(dir).map_err(|err| Error::io(dir, err))?;
    let mut names = Vec::new();
    for entry in entries {
        names.push(entry.map_err(|err| Error::io(dir, err))?.file_name());
    }
    names.sort();
    Ok(names)
}

fn only_child(dir: &Path, name: &str) -> Result<Option<OsString>> {
    let matches: Vec<OsString> = child_names(dir)?
        .into_iter()
        .filter(|child| is_named(child, name))
        .collect();
    match matches.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(one.clone())),
        [first, second, ..] => Err(Error::AmbiguousCase {
            first: dir.join(first),
            second: dir.join(second),
        }),
    }
}

fn entry_key(entry: &Entry) -> String {
    let parts: Vec<String> = entry
        .iter()
        .map(|part| part.to_string_lossy().to_lowercase())
        .collect();
    parts.join("/")
}

fn user_entries(dir: &Path) -> Result<BTreeMap<String, Entry>> {
    let mut found: Vec<Entry> = Vec::new();
    for name in child_names(dir)? {
        if is_user_path(Path::new(&name)) {
            found.push(vec![name]);
        }
    }
    if let Some(data) = only_child(dir, DATA)?
        && is_real_dir(&dir.join(&data))
        && let Some(localization) = only_child(&dir.join(&data), LOCALIZATION)?
    {
        found.push(vec![data, localization]);
    }
    let mut entries = BTreeMap::new();
    for entry in found {
        let key = entry_key(&entry);
        if let Some(earlier) = entries.insert(key, entry.clone()) {
            return Err(Error::AmbiguousCase {
                first: earlier.iter().fold(dir.to_path_buf(), |p, n| p.join(n)),
                second: entry.iter().fold(dir.to_path_buf(), |p, n| p.join(n)),
            });
        }
    }
    Ok(entries)
}

fn under(dir: &Path, parts: &[&OsString]) -> PathBuf {
    parts
        .iter()
        .fold(dir.to_path_buf(), |path, part| path.join(part))
}

fn parked(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{name}{SWAP_SUFFIX}"))
}

fn rename(from: PathBuf, to: PathBuf) -> Step {
    Step::Rename { from, to }
}

/// After the folder renames, `owner`'s entry sits under `other`'s path and must move back.
/// The listings are read before anything moves, so `other` still shows the other channel's tree.
fn one_sided_steps(owner: &Path, other: &Path, entry: &Entry) -> Result<Vec<Step>> {
    match entry.as_slice() {
        [name] => Ok(vec![rename(other.join(name), owner.join(name))]),
        [data, localization] => match only_child(other, DATA)? {
            Some(other_data) if is_real_dir(&other.join(&other_data)) => Ok(vec![rename(
                under(other, &[data, localization]),
                under(owner, &[&other_data, localization]),
            )]),
            Some(other_data) => Err(Error::Invalid(format!(
                "{} is not a folder, so {} cannot stay with its channel",
                other.join(other_data).display(),
                under(owner, &[data, localization]).display()
            ))),
            None if child_names(&owner.join(data))?.len() == 1 => {
                Ok(vec![rename(other.join(data), owner.join(data))])
            }
            None => Err(Error::Invalid(format!(
                "{} has no {DATA} folder and {} holds more than {LOCALIZATION}; swap with user files travelling or move it first",
                other.display(),
                owner.join(data).display()
            ))),
        },
        _ => Ok(Vec::new()),
    }
}

/// Exchanges an entry both channels have: park B's copy, bring A's home, then send B's home.
fn exchange_steps(a: &Path, b: &Path, entry_a: &Entry, entry_b: &Entry) -> Vec<Step> {
    let (b_parent, a_parent): (Vec<&OsString>, Vec<&OsString>) = (
        entry_b[..entry_b.len() - 1].iter().collect(),
        entry_a[..entry_a.len() - 1].iter().collect(),
    );
    let (name_a, name_b) = (&entry_a[entry_a.len() - 1], &entry_b[entry_b.len() - 1]);
    let b_in_a = under(a, &b_parent).join(name_b);
    let a_in_b = under(b, &a_parent).join(name_a);
    vec![
        rename(b_in_a.clone(), parked(&b_in_a)),
        rename(a_in_b, under(a, &b_parent).join(name_a)),
        rename(parked(&b_in_a), under(b, &a_parent).join(name_b)),
    ]
}

fn stay_home_steps(a: &Path, b: &Path) -> Result<Vec<Step>> {
    let (entries_a, entries_b) = (user_entries(a)?, user_entries(b)?);
    let keys: BTreeSet<&String> = entries_a.keys().chain(entries_b.keys()).collect();
    let mut steps = Vec::new();
    for key in keys {
        match (entries_a.get(key), entries_b.get(key)) {
            (Some(entry_a), Some(entry_b)) => steps.extend(exchange_steps(a, b, entry_a, entry_b)),
            (Some(entry_a), None) => steps.extend(one_sided_steps(a, b, entry_a)?),
            (None, Some(entry_b)) => steps.extend(one_sided_steps(b, a, entry_b)?),
            (None, None) => {}
        }
    }
    Ok(steps)
}

pub fn plan_swap(
    install: &Install,
    a: &str,
    b: &str,
    user_files_travel: bool,
) -> Result<SwapJournal> {
    let (dir_a, dir_b) = validate_pair(install, a, b)?;
    validate_pair(install, b, a)?;
    let temp = install.root.join(SWAP_DIR);
    if exists_no_follow(&temp) {
        return Err(Error::Invalid(format!(
            "{} exists; remove it or resume the swap it belongs to",
            temp.display()
        )));
    }
    let mut steps = vec![
        rename(dir_a.clone(), temp.clone()),
        rename(dir_b.clone(), dir_a.clone()),
        rename(temp, dir_b.clone()),
    ];
    if !user_files_travel {
        steps.extend(stay_home_steps(&dir_a, &dir_b)?);
    }
    let name = |dir: &Path| {
        dir.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    Ok(SwapJournal {
        install_root: install.root.clone(),
        a: name(&dir_a),
        b: name(&dir_b),
        steps,
        done: 0,
        ids: Vec::new(),
    })
}

fn inside_root(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .ok()
        .and_then(normal_parts)
        .is_some_and(|parts| !parts.is_empty())
}

fn check_journal(journal: &SwapJournal) -> Result<()> {
    let root = &journal.install_root;
    let steps_inside = journal
        .steps
        .iter()
        .all(|Step::Rename { from, to }| inside_root(root, from) && inside_root(root, to));
    let ids_fit = journal.ids.is_empty() || journal.ids.len() == journal.steps.len();
    if !root.is_absolute() || !steps_inside || !ids_fit || journal.done > journal.steps.len() {
        return Err(Error::Invalid(format!(
            "the swap journal for {} is not valid",
            root.display()
        )));
    }
    Ok(())
}

fn move_entry(from: &Path, to: &Path) -> Result<()> {
    if exists_no_follow(to) {
        return Err(Error::io(
            to,
            io::Error::new(ErrorKind::AlreadyExists, "already exists"),
        ));
    }
    fs::rename(from, to).map_err(|err| Error::io(from, err))?;
    for dir in [from.parent(), to.parent()].into_iter().flatten() {
        safe_write::sync_dir(dir);
    }
    Ok(())
}

fn save(journal: &SwapJournal, file: &Path) -> Result<()> {
    let text = serde_json::to_vec_pretty(journal).map_err(|err| Error::Invalid(err.to_string()))?;
    write_atomic(file, &text)
}

fn discard(file: &Path) -> Result<()> {
    safe_write::remove(file)
}

type Mover<'a> = dyn FnMut(&Path, &Path) -> Result<()> + 'a;

fn step_paths(step: &Step) -> (&Path, &Path) {
    let Step::Rename { from, to } = step;
    (from, to)
}

fn redo(journal: &mut SwapJournal, file: &Path, mover: &mut Mover) -> Result<()> {
    while journal.done < journal.steps.len() {
        let (from, to) = step_paths(&journal.steps[journal.done]);
        mover(from, to)?;
        journal.done += 1;
        save(journal, file)?;
    }
    Ok(())
}

/// The journal is lowered before each reverse rename, so after a crash it is at most one step
/// behind the disk, which `catch_up` detects. A failed journal write does not stop the undo,
/// because getting back to the starting state matters more than recording the way there.
fn undo(journal: &mut SwapJournal, file: &Path, mover: &mut Mover) -> Result<()> {
    while journal.done > 0 {
        let index = journal.done - 1;
        journal.done = index;
        let _ = save(journal, file);
        let (from, to) = step_paths(&journal.steps[index]);
        if let Err(err) = mover(to, from) {
            journal.done = index + 1;
            let _ = save(journal, file);
            return Err(err);
        }
    }
    Ok(())
}

/// Tells one moved entry from another across renames: device and inode on Unix, volume and file
/// index on Windows. Creation time does not work there, because NTFS gives a name that was
/// just freed and taken again the creation time of the entry that left it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryId(u64, u64);

#[cfg(unix)]
fn entry_id(path: &Path) -> Option<EntryId> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::symlink_metadata(path).ok()?;
    Some(EntryId(meta.dev(), meta.ino()))
}

#[cfg(windows)]
fn entry_id(path: &Path) -> Option<EntryId> {
    let handle = winapi_util::Handle::from_path_any(path).ok()?;
    let info = winapi_util::file::information(&handle).ok()?;
    Some(EntryId(info.volume_serial_number(), info.file_index()))
}

#[cfg(not(any(unix, windows)))]
fn entry_id(_path: &Path) -> Option<EntryId> {
    None
}

fn moved_by(path: &Path, from: &Path, to: &Path) -> PathBuf {
    match path.strip_prefix(from) {
        Ok(rest) if rest.as_os_str().is_empty() => to.to_path_buf(),
        Ok(rest) => to.join(rest),
        Err(_) => path.to_path_buf(),
    }
}

/// Where the entry at `path` after `steps[..from_state]` ran sits after `steps[..to_state]` ran.
fn follow(steps: &[Step], path: &Path, from_state: usize, to_state: usize) -> PathBuf {
    if from_state <= to_state {
        steps[from_state..to_state]
            .iter()
            .fold(path.to_path_buf(), |path, step| {
                let (from, to) = step_paths(step);
                moved_by(&path, from, to)
            })
    } else {
        steps[to_state..from_state]
            .iter()
            .rev()
            .fold(path.to_path_buf(), |path, step| {
                let (from, to) = step_paths(step);
                moved_by(&path, to, from)
            })
    }
}

/// Where the entry that step `index` moves sits once `state` steps have run.
fn location_of(steps: &[Step], index: usize, state: usize) -> PathBuf {
    let (from, to) = step_paths(&steps[index]);
    if index < state {
        follow(steps, to, index + 1, state)
    } else {
        follow(steps, from, index, state)
    }
}

fn record_ids(journal: &mut SwapJournal) {
    journal.ids = (0..journal.steps.len())
        .map(|index| entry_id(&location_of(&journal.steps, index, 0)))
        .collect();
}

fn disk_is_at(journal: &SwapJournal, state: usize) -> bool {
    let steps = &journal.steps;
    let next_free = steps
        .get(state)
        .is_none_or(|step| !exists_no_follow(step_paths(step).1));
    next_free
        && (0..steps.len()).all(|index| {
            let at = location_of(steps, index, state);
            match journal.ids.get(index).copied().flatten() {
                Some(id) => entry_id(&at) == Some(id),
                None => exists_no_follow(&at),
            }
        })
}

/// Sets `done` to the number of steps the disk shows, which can be more or fewer than recorded.
fn catch_up(journal: &mut SwapJournal, journal_file: &Path) -> Result<()> {
    let recorded = journal.done;
    let mut states: Vec<usize> = (0..=journal.steps.len()).collect();
    states.sort_by_key(|state| state.abs_diff(recorded));
    match states.into_iter().find(|state| disk_is_at(journal, *state)) {
        Some(state) => {
            journal.done = state;
            Ok(())
        }
        None => Err(Error::Invalid(format!(
            "the folders in {} match no point of the swap recorded in {}; put them back by hand",
            journal.install_root.display(),
            journal_file.display()
        ))),
    }
}

pub fn run_swap(journal: SwapJournal, journal_file: &Path, probe: &dyn GameProbe) -> Result<()> {
    run_swap_with(journal, journal_file, probe, &mut move_entry)
}

fn run_swap_with(
    mut journal: SwapJournal,
    journal_file: &Path,
    probe: &dyn GameProbe,
    mover: &mut Mover,
) -> Result<()> {
    require_closed(probe)?;
    check_journal(&journal)?;
    if journal.done != 0 {
        return Err(Error::Invalid(
            "the swap has already started; resume it instead".into(),
        ));
    }
    if exists_no_follow(journal_file) {
        return Err(Error::Invalid(format!(
            "an earlier swap is pending in {}; resume it first",
            journal_file.display()
        )));
    }
    require_closed(probe)?;
    check_writable(&journal.install_root)?;
    record_ids(&mut journal);
    save(&journal, journal_file)?;
    if let Err(first) = redo(&mut journal, journal_file, mover) {
        if undo(&mut journal, journal_file, mover).is_ok() {
            let _ = discard(journal_file);
        }
        return Err(first);
    }
    discard(journal_file)
}

pub fn pending_swap(journal_file: &Path) -> Result<Option<SwapJournal>> {
    let bytes = match fs::read(journal_file) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Error::io(journal_file, err)),
    };
    let journal: SwapJournal = serde_json::from_slice(&bytes).map_err(|err| {
        Error::Invalid(format!(
            "the swap journal {} is unreadable: {err}",
            journal_file.display()
        ))
    })?;
    check_journal(&journal)?;
    Ok(Some(journal))
}

/// Finds a pending swap for `install` in `journal_dir`, also one written under another spelling
/// of the install's path. Returns the journal file and its contents.
pub fn find_pending_swap(
    journal_dir: &Path,
    install: &Install,
) -> Result<Option<(PathBuf, SwapJournal)>> {
    let own = journal_path(journal_dir, install);
    if let Some(journal) = pending_swap(&own)? {
        return Ok(Some((own, journal)));
    }
    let entries = match fs::read_dir(journal_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Error::io(journal_dir, err)),
    };
    let root = canonical_or_raw(&install.root);
    let mut names: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.starts_with("swap-") && name.ends_with(".json"))
        })
        .collect();
    names.sort();
    for file in names {
        if let Ok(Some(journal)) = pending_swap(&file)
            && canonical_or_raw(&journal.install_root) == root
        {
            return Ok(Some((file, journal)));
        }
    }
    Ok(None)
}

pub fn resume_swap(
    mut journal: SwapJournal,
    journal_file: &Path,
    how: Resume,
    probe: &dyn GameProbe,
) -> Result<()> {
    require_closed(probe)?;
    check_journal(&journal)?;
    catch_up(&mut journal, journal_file)?;
    let mover: &mut Mover = &mut move_entry;
    match how {
        Resume::Finish => redo(&mut journal, journal_file, mover)?,
        Resume::Undo => undo(&mut journal, journal_file, mover)?,
    }
    discard(journal_file)
}

// settings

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    UserCfg,
    Keybinds,
    Layouts,
    GameSettings,
    StarStrings,
}

fn layout_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(names) = child_names(dir) else {
        return Vec::new();
    };
    names
        .into_iter()
        .map(|name| dir.join(name))
        .filter(|path| {
            is_regular_file(path)
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("xml"))
        })
        .collect()
}

pub fn part_files(channel: &Channel, part: Part) -> Vec<PathBuf> {
    match part {
        Part::UserCfg => vec![channel.user_cfg()],
        Part::Keybinds => vec![channel.actionmaps_xml()],
        Part::Layouts => layout_files(&channel.mappings_dir()),
        Part::GameSettings => vec![channel.attributes_xml()],
        Part::StarStrings => vec![channel.global_ini()],
    }
}

fn channel_rel(channel: &Channel, path: &Path) -> Result<String> {
    let rel = path
        .strip_prefix(&channel.dir)
        .ok()
        .filter(|rel| normal_parts(rel).is_some_and(|parts| !parts.is_empty()))
        .ok_or_else(|| {
            Error::Invalid(format!(
                "{} is outside {}",
                path.display(),
                channel.dir.display()
            ))
        })?;
    Ok(rel_text(rel))
}

/// The settings files of `parts` in `channel`, keyed by lower-case relative path, in part order.
fn settings_rels(channel: &Channel, parts: &[Part]) -> Result<Vec<(String, String)>> {
    let mut rels: Vec<(String, String)> = Vec::new();
    for part in parts {
        for path in part_files(channel, *part) {
            let rel = channel_rel(channel, &path)?;
            let key = rel.to_lowercase();
            if !rels.iter().any(|(known, _)| *known == key) {
                rels.push((key, rel));
            }
        }
    }
    Ok(rels)
}

fn exists(loaded: &LoadedFile) -> bool {
    loaded.hash.is_some()
}

pub fn copy_settings(
    source: &Channel,
    target: &Channel,
    parts: &[Part],
    writer: &Writer,
    target_scope: &Scope,
) -> Result<SnapshotInfo> {
    if writer.blocked() {
        return Err(Error::GameRunning);
    }
    let mut pairs: Vec<(LoadedFile, LoadedFile)> = Vec::new();
    for (_, rel) in settings_rels(source, parts)? {
        let from = safe_write::load(&source.dir.join(&rel))?;
        if exists(&from) {
            let into = safe_write::load(&resolve_ci_or_default(&target.dir, &rel))?;
            pairs.push((from, into));
        }
    }
    if pairs.is_empty() {
        return Err(Error::Invalid(format!(
            "{} has none of the chosen settings files",
            source.name
        )));
    }
    let changes: Vec<Change> = pairs
        .iter()
        .map(|(from, into)| Change {
            loaded: into,
            new_bytes: Some(&from.bytes),
        })
        .collect();
    writer.apply(
        target_scope,
        target,
        &changes,
        &format!("Copy settings from {}", source.name),
    )
}

fn other_side(from: &LoadedFile) -> Option<&[u8]> {
    exists(from).then_some(from.bytes.as_slice())
}

pub fn swap_settings(
    a: &Channel,
    b: &Channel,
    parts: &[Part],
    writer: &Writer,
    scope_a: &Scope,
    scope_b: &Scope,
) -> Result<(SnapshotInfo, SnapshotInfo)> {
    if writer.blocked() {
        return Err(Error::GameRunning);
    }
    let mut rels = settings_rels(a, parts)?;
    for (key, rel) in settings_rels(b, parts)? {
        if !rels.iter().any(|(known, _)| *known == key) {
            rels.push((key, rel));
        }
    }
    let mut pairs: Vec<(LoadedFile, LoadedFile)> = Vec::new();
    for (_, rel) in rels {
        let in_a = safe_write::load(&resolve_ci_or_default(&a.dir, &rel))?;
        let in_b = safe_write::load(&resolve_ci_or_default(&b.dir, &rel))?;
        if exists(&in_a) || exists(&in_b) {
            pairs.push((in_a, in_b));
        }
    }
    if pairs.is_empty() {
        return Err(Error::Invalid(format!(
            "neither {} nor {} has the chosen settings files",
            a.name, b.name
        )));
    }
    let changes_a: Vec<Change> = pairs
        .iter()
        .map(|(in_a, in_b)| Change {
            loaded: in_a,
            new_bytes: other_side(in_b),
        })
        .collect();
    let changes_b: Vec<Change> = pairs
        .iter()
        .map(|(in_a, in_b)| Change {
            loaded: in_b,
            new_bytes: other_side(in_a),
        })
        .collect();
    let label_a = format!("Swap settings with {}", b.name);
    let label_b = format!("Swap settings with {}", a.name);
    let done = writer.apply_all(&[
        Target {
            scope: scope_a,
            channel: a,
            changes: &changes_a,
            label: &label_a,
        },
        Target {
            scope: scope_b,
            channel: b,
            changes: &changes_b,
            label: &label_b,
        },
    ])?;
    let mut done = done.into_iter();
    match (done.next(), done.next()) {
        (Some(first), Some(second)) => Ok((first, second)),
        _ => Err(Error::Invalid(
            "the settings swap returned too few snapshots".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::BackupStore;
    use crate::install::InstallKind;
    use crate::process::FixedProbe;
    use std::sync::Arc;
    use tempfile::{TempDir, tempdir};

    const CLOSED: FixedProbe = FixedProbe {
        game: RunState::NotRunning,
        launcher: RunState::NotRunning,
    };
    const GAME_FILES: [&str; 2] = ["Data.p4k", "Bin64/StarCitizen.exe"];
    const USER_FILES: [&str; 4] = [
        "user.cfg",
        "USER/Client/0/Profiles/default/actionmaps.xml",
        "ScreenShots/a.jpg",
        "data/Localization/english/global.ini",
    ];
    const MIXED_CASE_USER_FILES: [&str; 4] = [
        "USER.CFG",
        "user/Client/0/Profiles/default/actionmaps.xml",
        "screenshots/a.jpg",
        "Data/localization/english/global.ini",
    ];

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Node {
        Dir,
        File(Vec<u8>),
        Link(PathBuf),
    }

    type Tree = BTreeMap<String, Node>;

    struct Fx {
        tmp: TempDir,
        install: Install,
    }

    impl Fx {
        fn dir(&self, name: &str) -> PathBuf {
            self.install.root.join(name)
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.install.root.join(rel)
        }

        fn write(&self, rel: &str, bytes: &[u8]) {
            let path = self.path(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }

        fn read(&self, rel: &str) -> Vec<u8> {
            fs::read(self.path(rel)).unwrap()
        }

        fn journal(&self) -> PathBuf {
            journal_path(&self.tmp.path().join("journals"), &self.install)
        }

        fn tree(&self) -> Tree {
            tree_of(&self.install.root)
        }

        fn channel(&self, name: &str) -> Channel {
            Channel {
                name: name.to_string(),
                dir: self.dir(name),
            }
        }

        fn writer(&self, game: RunState) -> Writer {
            Writer::new(
                BackupStore::new(self.tmp.path().join("backups")),
                Arc::new(FixedProbe {
                    game,
                    launcher: RunState::NotRunning,
                }),
                false,
            )
        }
    }

    fn tree_of(root: &Path) -> Tree {
        WalkDir::new(root)
            .follow_links(false)
            .min_depth(1)
            .into_iter()
            .map(|entry| {
                let entry = entry.unwrap();
                let rel = entry.path().strip_prefix(root).unwrap();
                let rel = rel.to_string_lossy().replace('\\', "/");
                let kind = entry.file_type();
                let node = if kind.is_symlink() {
                    Node::Link(fs::read_link(entry.path()).unwrap())
                } else if kind.is_dir() {
                    Node::Dir
                } else {
                    Node::File(fs::read(entry.path()).unwrap())
                };
                (rel, node)
            })
            .collect()
    }

    fn scope(channel: &str) -> Scope {
        Scope {
            install_key: "k".into(),
            channel: channel.into(),
        }
    }

    fn content(channel: &str, rel: &str) -> Vec<u8> {
        format!("{channel} {rel}\n").into_bytes()
    }

    fn empty() -> Fx {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("StarCitizen");
        fs::create_dir_all(&root).unwrap();
        Fx {
            install: Install {
                root,
                kind: InstallKind::Native,
            },
            tmp,
        }
    }

    fn add_channel(fx: &Fx, name: &str, user_files: &[&str]) {
        for rel in GAME_FILES.iter().chain(user_files) {
            fx.write(&format!("{name}/{rel}"), &content(name, rel));
        }
    }

    fn fixture() -> Fx {
        let fx = empty();
        add_channel(&fx, "LIVE", &USER_FILES);
        add_channel(&fx, "PTU", &USER_FILES);
        fx
    }

    fn lopsided() -> Fx {
        let fx = empty();
        add_channel(&fx, "LIVE", &USER_FILES);
        add_channel(&fx, "PTU", &[]);
        fx
    }

    fn mixed_case() -> Fx {
        let fx = empty();
        add_channel(&fx, "LIVE", &USER_FILES);
        fx.write("LIVE/data/other.txt", b"live game data\n");
        add_channel(&fx, "PTU", &MIXED_CASE_USER_FILES);
        fx
    }

    fn closed_with(game: RunState, launcher: RunState) -> FixedProbe {
        FixedProbe { game, launcher }
    }

    fn rel_text(rel: &Path) -> String {
        rel.to_string_lossy().replace('\\', "/")
    }

    fn set_mtime(path: &Path, time: SystemTime) {
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(time)
            .unwrap();
    }

    fn mtime(path: &Path) -> SystemTime {
        fs::metadata(path).unwrap().modified().unwrap()
    }

    fn no_part_files(tree: &Tree) -> bool {
        tree.keys().all(|rel| !rel.ends_with(".star-config.part"))
    }

    fn user_entries_of(tree: &Tree, channel: &str) -> Tree {
        let prefix = format!("{channel}/");
        tree.iter()
            .filter(|(rel, _)| {
                rel.strip_prefix(&prefix)
                    .is_some_and(|rest| is_user_path(Path::new(rest)))
            })
            .map(|(rel, node)| (rel.clone(), node.clone()))
            .collect()
    }

    fn copy(fx: &Fx, source: &str, target: &str, options: CopyOptions) -> CopyOutcome {
        let plan = plan_copy(&fx.install, source, target, options).unwrap();
        run_copy(
            &plan,
            options,
            &CLOSED,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap()
    }

    // classification and validation

    #[test]
    fn is_user_path_classifies_ignoring_case() {
        for rel in [
            "USER/x",
            "user.cfg",
            "User.CFG",
            "screenshots/a.jpg",
            "Data/Localization/english/global.ini",
            "data/localization",
        ] {
            assert!(is_user_path(Path::new(rel)), "{rel}");
        }
        for rel in ["Data.p4k", "Bin64/x", "data/other", "data", "", "x/USER"] {
            assert!(!is_user_path(Path::new(rel)), "{rel}");
        }
    }

    #[test]
    fn validate_pair_accepts_existing_and_known_new_target() {
        let fx = fixture();
        fs::create_dir_all(fx.dir("MyChannel")).unwrap();
        assert_eq!(
            validate_pair(&fx.install, "LIVE", "PTU").unwrap(),
            (fx.dir("LIVE"), fx.dir("PTU"))
        );
        assert_eq!(
            validate_pair(&fx.install, "live", "ptu").unwrap(),
            (fx.dir("LIVE"), fx.dir("PTU"))
        );
        assert_eq!(
            validate_pair(&fx.install, "LIVE", "eptu").unwrap(),
            (fx.dir("LIVE"), fx.dir("EPTU"))
        );
        assert_eq!(
            validate_pair(&fx.install, "LIVE", "mychannel").unwrap(),
            (fx.dir("LIVE"), fx.dir("MyChannel"))
        );
        assert!(!fx.dir("EPTU").exists());
    }

    #[test]
    fn validate_pair_rejects_same_unknown_new_traversal_and_missing_p4k() {
        let fx = fixture();
        fs::create_dir_all(fx.dir("HOTFIX/USER")).unwrap();
        fx.write("notes.txt", b"x\n");
        for (source, target) in [
            ("LIVE", "live"),
            ("LIVE", "MYSTUFF"),
            ("../x", "PTU"),
            ("LIVE", "../x"),
            ("LIVE", ".."),
            (".", "PTU"),
            ("LIVE", "a/b"),
            ("LIVE", "a\\b"),
            ("LIVE", ""),
            ("HOTFIX", "PTU"),
            ("EPTU", "PTU"),
            ("LIVE", "notes.txt"),
        ] {
            let result = validate_pair(&fx.install, source, target);
            assert!(
                matches!(result, Err(Error::Invalid(_))),
                "{source} -> {target}: {result:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn validate_pair_rejects_symlinked_channel() {
        let fx = fixture();
        let elsewhere = fx.tmp.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(elsewhere.join("Data.p4k"), b"p4k\n").unwrap();
        std::os::unix::fs::symlink(&elsewhere, fx.dir("EPTU")).unwrap();
        for (source, target) in [("EPTU", "LIVE"), ("LIVE", "EPTU")] {
            let result = validate_pair(&fx.install, source, target);
            assert!(matches!(&result, Err(Error::Invalid(message)) if message.contains("EPTU")));
        }
    }

    #[test]
    fn require_closed_needs_game_and_launcher_not_running() {
        use RunState::{NotRunning, Running, Unknown};
        require_closed(&CLOSED).unwrap();
        for (game, launcher) in [
            (Running, NotRunning),
            (Unknown, NotRunning),
            (NotRunning, Running),
            (NotRunning, Unknown),
            (Running, Running),
        ] {
            assert!(matches!(
                require_closed(&closed_with(game, launcher)),
                Err(Error::GameRunning)
            ));
        }
    }

    // copy

    #[test]
    fn plan_lists_game_files_only_and_skips_matching_ones() {
        let fx = fixture();
        fx.write("LIVE/data/other", b"game data\n");
        let live_exe = fx.path("LIVE/Bin64/StarCitizen.exe");
        let ptu_exe = fx.path("PTU/Bin64/StarCitizen.exe");
        fs::copy(&live_exe, &ptu_exe).unwrap();
        set_mtime(&ptu_exe, mtime(&live_exe));
        let plan = plan_copy(&fx.install, "LIVE", "PTU", CopyOptions::default()).unwrap();
        let listed: Vec<(String, bool)> = plan
            .files
            .iter()
            .map(|file| (rel_text(&file.rel), file.copy))
            .collect();
        assert_eq!(
            listed,
            [
                ("Bin64/StarCitizen.exe".to_string(), false),
                ("Data.p4k".to_string(), true),
                ("data/other".to_string(), true),
            ]
        );
        let p4k = fx.read("LIVE/Data.p4k").len() as u64;
        assert_eq!(plan.bytes_to_copy, p4k + 10);
        assert_eq!(plan.largest_copy, p4k.max(10));
        assert_eq!(plan.source, fx.dir("LIVE"));
        assert_eq!(plan.target, fx.dir("PTU"));
        assert!(!plan.needs_in_place);
    }

    #[test]
    fn plan_with_compare_contents_detects_same_size_different_bytes() {
        let fx = fixture();
        fx.write("LIVE/Data.p4k", b"aaaa");
        fx.write("PTU/Data.p4k", b"bbbb");
        set_mtime(&fx.path("PTU/Data.p4k"), mtime(&fx.path("LIVE/Data.p4k")));
        let copies = |options: CopyOptions| -> Vec<(String, bool)> {
            plan_copy(&fx.install, "LIVE", "PTU", options)
                .unwrap()
                .files
                .iter()
                .map(|file| (rel_text(&file.rel), file.copy))
                .collect()
        };
        assert!(copies(CopyOptions::default()).contains(&("Data.p4k".to_string(), false)));
        let compared = copies(CopyOptions {
            compare_contents: true,
            in_place: false,
        });
        assert!(compared.contains(&("Data.p4k".to_string(), true)));
    }

    #[test]
    fn free_space_matches_verbatim_and_unc_paths_to_plain_mounts() {
        let windows = [
            (r"C:\".to_string(), 10),
            (r"D:\".to_string(), 20),
            (r"\\server\share\".to_string(), 30),
            (r"D:\mnt\games\".to_string(), 40),
        ];
        let free = |path: &str| free_on_mount(path, &windows, true);
        assert_eq!(free(r"\\?\C:\Program Files\x"), Some(10));
        assert_eq!(free(r"\\?\c:\x"), Some(10));
        assert_eq!(free(r"\\?\UNC\server\share\games\sc"), Some(30));
        assert_eq!(free(r"\\?\D:\mnt\games\sc"), Some(40));
        assert_eq!(free(r"\\?\D:\mnt\gamesx"), Some(20));
        assert_eq!(free(r"\\?\E:\x"), None);
        let unix = [
            ("/".to_string(), 1),
            ("/mnt/games".to_string(), 2),
            ("/mnt/g".to_string(), 3),
        ];
        assert_eq!(free_on_mount("/mnt/games/sc", &unix, false), Some(2));
        assert_eq!(free_on_mount("/home/x", &unix, false), Some(1));
    }

    #[test]
    fn space_check_flags_in_place_and_refuses_copies_that_cannot_fit() {
        assert!(!space_needs_in_place(100, 0, None).unwrap());
        assert!(!space_needs_in_place(100, 0, Some(100)).unwrap());
        assert!(space_needs_in_place(100, 50, Some(80)).unwrap());
        let err = space_needs_in_place(100, 30, Some(40)).unwrap_err();
        assert!(
            matches!(&err, Error::Invalid(message) if message.contains("70") && message.contains("40"))
        );
    }

    #[test]
    fn copy_fills_new_channel_and_leaves_no_part_files() {
        let fx = fixture();
        let plan = plan_copy(&fx.install, "LIVE", "hotfix", CopyOptions::default()).unwrap();
        assert_eq!(plan.target, fx.dir("HOTFIX"));
        let mut seen = Vec::new();
        let outcome = run_copy(
            &plan,
            CopyOptions::default(),
            &CLOSED,
            &AtomicBool::new(false),
            &mut |progress| seen.push(progress),
        )
        .unwrap();
        assert_eq!(
            outcome,
            CopyOutcome {
                files_copied: 2,
                bytes_copied: plan.bytes_to_copy,
                cancelled: false,
            }
        );
        let last = *seen.last().unwrap();
        assert_eq!((last.files_done, last.files_total), (2, 2));
        assert_eq!(last.bytes_done, last.bytes_total);
        let hotfix = tree_of(&fx.dir("HOTFIX"));
        let expected: Tree = [
            ("Bin64".to_string(), Node::Dir),
            (
                "Bin64/StarCitizen.exe".to_string(),
                Node::File(content("LIVE", "Bin64/StarCitizen.exe")),
            ),
            (
                "Data.p4k".to_string(),
                Node::File(content("LIVE", "Data.p4k")),
            ),
        ]
        .into_iter()
        .collect();
        assert_eq!(hotfix, expected);
        assert!(no_part_files(&fx.tree()));
    }

    #[test]
    fn copy_overwrites_differing_game_files_and_keeps_target_user_files() {
        let fx = fixture();
        let before = user_entries_of(&fx.tree(), "PTU");
        assert_eq!(before.len(), 12);
        copy(&fx, "LIVE", "PTU", CopyOptions::default());
        for rel in GAME_FILES {
            assert_eq!(fx.read(&format!("PTU/{rel}")), content("LIVE", rel));
        }
        let after = fx.tree();
        assert_eq!(user_entries_of(&after, "PTU"), before);
        assert!(no_part_files(&after));
    }

    #[test]
    fn copy_keeps_the_target_session_files() {
        let fx = fixture();
        let session = [
            "Game.log",
            "logbackups/Game-old.log",
            "Crashes/dump.dmp",
            "logs/x.log",
            "loginData.json",
        ];
        for rel in session {
            fx.write(&format!("LIVE/{rel}"), &content("LIVE", rel));
            fx.write(&format!("PTU/{rel}"), &content("PTU", rel));
        }
        let plan = plan_copy(&fx.install, "LIVE", "PTU", CopyOptions::default()).unwrap();
        let planned: Vec<String> = plan.files.iter().map(|f| rel_text(&f.rel)).collect();
        assert_eq!(planned, ["Bin64/StarCitizen.exe", "Data.p4k"]);
        copy(&fx, "LIVE", "PTU", CopyOptions::default());
        for rel in session {
            assert_eq!(fx.read(&format!("PTU/{rel}")), content("PTU", rel), "{rel}");
        }
    }

    #[test]
    fn copy_never_deletes_extra_target_files() {
        let fx = fixture();
        fx.write("PTU/Bin64/extra.dll", b"extra\n");
        fx.write("PTU/old.txt", b"old\n");
        copy(&fx, "LIVE", "PTU", CopyOptions::default());
        assert_eq!(fx.read("PTU/Bin64/extra.dll"), b"extra\n");
        assert_eq!(fx.read("PTU/old.txt"), b"old\n");
    }

    #[test]
    fn copy_preserves_modified_time_so_a_second_plan_copies_nothing() {
        let fx = fixture();
        let old = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        set_mtime(&fx.path("LIVE/Data.p4k"), old);
        copy(&fx, "LIVE", "PTU", CopyOptions::default());
        assert_eq!(mtime(&fx.path("PTU/Data.p4k")), old);
        let again = plan_copy(&fx.install, "LIVE", "PTU", CopyOptions::default()).unwrap();
        assert!(again.files.iter().all(|file| !file.copy));
        assert_eq!(again.bytes_to_copy, 0);
    }

    #[test]
    fn cancel_before_start_copies_nothing() {
        let fx = fixture();
        let before = fx.tree();
        for target in ["PTU", "HOTFIX"] {
            let plan = plan_copy(&fx.install, "LIVE", target, CopyOptions::default()).unwrap();
            let outcome = run_copy(
                &plan,
                CopyOptions::default(),
                &CLOSED,
                &AtomicBool::new(true),
                &mut |_| {},
            )
            .unwrap();
            assert_eq!(
                outcome,
                CopyOutcome {
                    files_copied: 0,
                    bytes_copied: 0,
                    cancelled: true,
                }
            );
        }
        assert_eq!(fx.tree(), before);
    }

    #[test]
    fn cancel_mid_file_keeps_old_target_file_and_removes_part() {
        let fx = fixture();
        let big: Vec<u8> = (0..9 * 1024 * 1024 + 7).map(|n| (n % 251) as u8).collect();
        fx.write("LIVE/Data.p4k", &big);
        let plan = plan_copy(&fx.install, "LIVE", "PTU", CopyOptions::default()).unwrap();
        let cancel = AtomicBool::new(false);
        let mut cancel_in_big_file = |progress: Progress| {
            if progress.bytes_done >= CHUNK as u64 {
                cancel.store(true, Ordering::SeqCst);
            }
        };
        let outcome = run_copy_with(
            &plan,
            CopyOptions::default(),
            &CLOSED,
            &cancel,
            &mut cancel_in_big_file,
            Reflink::Skip,
        )
        .unwrap();
        assert!(outcome.cancelled);
        assert_eq!(outcome.files_copied, 1);
        assert_eq!(
            fx.read("PTU/Bin64/StarCitizen.exe"),
            content("LIVE", "Bin64/StarCitizen.exe")
        );
        assert_eq!(fx.read("PTU/Data.p4k"), content("PTU", "Data.p4k"));
        assert!(no_part_files(&fx.tree()));
    }

    #[cfg(unix)]
    #[test]
    fn copy_error_removes_part_and_keeps_old_target_file() {
        use std::os::unix::fs::PermissionsExt;
        let fx = fixture();
        let plan = plan_copy(&fx.install, "LIVE", "PTU", CopyOptions::default()).unwrap();
        let p4k = fx.path("LIVE/Data.p4k");
        fs::set_permissions(&p4k, fs::Permissions::from_mode(0o000)).unwrap();
        let result = run_copy(
            &plan,
            CopyOptions::default(),
            &CLOSED,
            &AtomicBool::new(false),
            &mut |_| {},
        );
        fs::set_permissions(&p4k, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(&result, Err(Error::Io { path, .. }) if *path == p4k));
        assert_eq!(fx.read("PTU/Data.p4k"), content("PTU", "Data.p4k"));
        assert!(no_part_files(&fx.tree()));
    }

    #[test]
    fn copy_refuses_when_game_or_launcher_running() {
        use RunState::{NotRunning, Running, Unknown};
        let fx = fixture();
        let before = fx.tree();
        let plan = plan_copy(&fx.install, "LIVE", "PTU", CopyOptions::default()).unwrap();
        for (game, launcher) in [
            (Running, NotRunning),
            (NotRunning, Running),
            (Unknown, NotRunning),
        ] {
            let result = run_copy(
                &plan,
                CopyOptions::default(),
                &closed_with(game, launcher),
                &AtomicBool::new(false),
                &mut |_| {},
            );
            assert!(matches!(result, Err(Error::GameRunning)));
        }
        assert_eq!(fx.tree(), before);
    }

    #[test]
    fn run_refuses_plan_needing_in_place_without_option() {
        let fx = fixture();
        let before = fx.tree();
        let mut plan = plan_copy(&fx.install, "LIVE", "PTU", CopyOptions::default()).unwrap();
        plan.needs_in_place = true;
        let result = run_copy(
            &plan,
            CopyOptions::default(),
            &CLOSED,
            &AtomicBool::new(false),
            &mut |_| {},
        );
        assert!(matches!(result, Err(Error::Invalid(_))));
        assert_eq!(fx.tree(), before);

        let in_place = CopyOptions {
            compare_contents: false,
            in_place: true,
        };
        let outcome = run_copy(
            &plan,
            in_place,
            &CLOSED,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(outcome.files_copied, 2);
        assert_eq!(fx.read("PTU/Data.p4k"), content("LIVE", "Data.p4k"));
        assert_eq!(
            mtime(&fx.path("PTU/Data.p4k")),
            mtime(&fx.path("LIVE/Data.p4k"))
        );
        assert!(no_part_files(&fx.tree()));
    }

    #[test]
    fn run_refuses_user_paths_and_escaping_paths_in_a_hand_built_plan() {
        let fx = fixture();
        let before = fx.tree();
        for rel in ["user.cfg", "../escape", "USER/x.xml"] {
            let plan = CopyPlan {
                source: fx.dir("LIVE"),
                target: fx.dir("PTU"),
                files: vec![PlannedFile {
                    rel: PathBuf::from(rel),
                    size: 1,
                    copy: true,
                }],
                bytes_to_copy: 1,
                largest_copy: 1,
                free_bytes: None,
                needs_in_place: false,
            };
            let result = run_copy(
                &plan,
                CopyOptions::default(),
                &CLOSED,
                &AtomicBool::new(false),
                &mut |_| {},
            );
            assert!(matches!(result, Err(Error::Invalid(_))), "{rel}");
        }
        assert_eq!(fx.tree(), before);
    }

    #[cfg(unix)]
    #[test]
    fn copy_skips_symlinks_in_source() {
        use std::os::unix::fs::symlink;
        let fx = fixture();
        let outside = fx.tmp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), b"secret\n").unwrap();
        symlink(fx.path("LIVE/Data.p4k"), fx.path("LIVE/Bin64/link.dll")).unwrap();
        symlink(&outside, fx.path("LIVE/linked")).unwrap();
        let plan = plan_copy(&fx.install, "LIVE", "PTU", CopyOptions::default()).unwrap();
        let rels: Vec<String> = plan.files.iter().map(|file| rel_text(&file.rel)).collect();
        assert_eq!(rels, ["Bin64/StarCitizen.exe", "Data.p4k"]);
        run_copy(
            &plan,
            CopyOptions::default(),
            &CLOSED,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        assert!(fs::symlink_metadata(fx.path("PTU/Bin64/link.dll")).is_err());
        assert!(fs::symlink_metadata(fx.path("PTU/linked")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn copy_refuses_to_write_through_a_symlinked_target_folder() {
        use std::os::unix::fs::symlink;
        let fx = fixture();
        let outside = fx.tmp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::remove_dir_all(fx.path("PTU/Bin64")).unwrap();
        symlink(&outside, fx.path("PTU/Bin64")).unwrap();
        let plan = plan_copy(&fx.install, "LIVE", "PTU", CopyOptions::default()).unwrap();
        let result = run_copy(
            &plan,
            CopyOptions::default(),
            &CLOSED,
            &AtomicBool::new(false),
            &mut |_| {},
        );
        assert!(matches!(result, Err(Error::Invalid(_))));
        assert!(tree_of(&outside).is_empty());
    }

    // swap

    #[derive(Clone, Copy)]
    struct Layout {
        build: fn() -> Fx,
        a: &'static str,
        b: &'static str,
        travel: bool,
    }

    const LAYOUTS: [Layout; 5] = [
        Layout {
            build: fixture,
            a: "LIVE",
            b: "PTU",
            travel: false,
        },
        Layout {
            build: fixture,
            a: "LIVE",
            b: "PTU",
            travel: true,
        },
        Layout {
            build: lopsided,
            a: "LIVE",
            b: "PTU",
            travel: false,
        },
        Layout {
            build: lopsided,
            a: "PTU",
            b: "LIVE",
            travel: false,
        },
        Layout {
            build: mixed_case,
            a: "LIVE",
            b: "PTU",
            travel: false,
        },
    ];

    fn plan(fx: &Fx, layout: Layout) -> SwapJournal {
        plan_swap(&fx.install, layout.a, layout.b, layout.travel).unwrap()
    }

    fn clean_swap(layout: Layout) -> Tree {
        let fx = (layout.build)();
        run_swap(plan(&fx, layout), &fx.journal(), &CLOSED).unwrap();
        fx.tree()
    }

    fn assert_finished(fx: &Fx) {
        assert!(!fx.journal().exists());
        assert!(
            fx.tree()
                .keys()
                .all(|rel| !rel.contains(".star-config-swap"))
        );
    }

    fn interrupt(fx: &Fx, layout: Layout, applied: usize, recorded: usize) -> SwapJournal {
        let mut journal = plan(fx, layout);
        record_ids(&mut journal);
        for Step::Rename { from, to } in &journal.steps[..applied] {
            fs::rename(from, to).unwrap();
        }
        journal.done = recorded;
        fs::create_dir_all(fx.journal().parent().unwrap()).unwrap();
        fs::write(fx.journal(), serde_json::to_vec(&journal).unwrap()).unwrap();
        pending_swap(&fx.journal()).unwrap().unwrap()
    }

    fn mover_failing_at(calls_to_fail: Vec<usize>) -> impl FnMut(&Path, &Path) -> Result<()> {
        let mut calls = 0;
        move |from, to| {
            let call = calls;
            calls += 1;
            if calls_to_fail.contains(&call) {
                Err(Error::io(from, io::Error::other("injected failure")))
            } else {
                move_entry(from, to)
            }
        }
    }

    fn assert_channel(fx: &Fx, dir: &str, game_of: &str, user_of: Option<&str>) {
        for rel in GAME_FILES {
            assert_eq!(
                fx.read(&format!("{dir}/{rel}")),
                content(game_of, rel),
                "{dir}/{rel}"
            );
        }
        for rel in USER_FILES {
            let path = fx.path(&format!("{dir}/{rel}"));
            match user_of {
                Some(owner) => assert_eq!(fs::read(&path).unwrap(), content(owner, rel)),
                None => assert!(!path.exists(), "{}", path.display()),
            }
        }
    }

    #[test]
    fn swap_exchanges_game_files_and_keeps_user_files_by_default() {
        let fx = fixture();
        run_swap(
            plan_swap(&fx.install, "LIVE", "PTU", false).unwrap(),
            &fx.journal(),
            &CLOSED,
        )
        .unwrap();
        assert_channel(&fx, "LIVE", "PTU", Some("LIVE"));
        assert_channel(&fx, "PTU", "LIVE", Some("PTU"));

        let fx = lopsided();
        run_swap(
            plan_swap(&fx.install, "LIVE", "PTU", false).unwrap(),
            &fx.journal(),
            &CLOSED,
        )
        .unwrap();
        assert_channel(&fx, "LIVE", "PTU", Some("LIVE"));
        assert_channel(&fx, "PTU", "LIVE", None);
        assert!(!fx.path("PTU/data").exists());
    }

    #[test]
    fn swap_keeps_differently_cased_user_entries_home() {
        let fx = mixed_case();
        run_swap(
            plan_swap(&fx.install, "LIVE", "PTU", false).unwrap(),
            &fx.journal(),
            &CLOSED,
        )
        .unwrap();
        for rel in USER_FILES {
            let live = resolve_ci(&fx.dir("LIVE"), rel).unwrap();
            assert_eq!(fs::read(live).unwrap(), content("LIVE", rel));
        }
        for rel in MIXED_CASE_USER_FILES {
            let ptu = resolve_ci(&fx.dir("PTU"), rel).unwrap();
            assert_eq!(fs::read(ptu).unwrap(), content("PTU", rel));
        }
        let other = resolve_ci(&fx.dir("PTU"), "data/other.txt").unwrap();
        assert_eq!(fs::read(other).unwrap(), b"live game data\n");
        assert_eq!(fx.read("LIVE/Data.p4k"), content("PTU", "Data.p4k"));
        assert_finished(&fx);
    }

    #[test]
    fn swap_with_travel_moves_everything() {
        let fx = fixture();
        let journal = plan_swap(&fx.install, "LIVE", "PTU", true).unwrap();
        assert_eq!(journal.steps.len(), 3);
        run_swap(journal, &fx.journal(), &CLOSED).unwrap();
        assert_channel(&fx, "LIVE", "PTU", Some("PTU"));
        assert_channel(&fx, "PTU", "LIVE", Some("LIVE"));
    }

    #[test]
    fn swap_leaves_no_temp_folder_and_no_journal() {
        for layout in LAYOUTS {
            let fx = (layout.build)();
            run_swap(plan(&fx, layout), &fx.journal(), &CLOSED).unwrap();
            assert!(!fx.dir(".star-config-swap").exists());
            assert_finished(&fx);
        }
    }

    #[test]
    fn swap_refuses_to_strand_localization_beside_other_data() {
        let fx = lopsided();
        fx.write("LIVE/data/other.txt", b"live game data\n");
        assert!(matches!(
            plan_swap(&fx.install, "LIVE", "PTU", false),
            Err(Error::Invalid(_))
        ));
        assert_eq!(
            plan_swap(&fx.install, "LIVE", "PTU", true)
                .unwrap()
                .steps
                .len(),
            3
        );
    }

    #[test]
    fn swap_refuses_when_running_and_changes_nothing() {
        use RunState::{NotRunning, Running, Unknown};
        let fx = fixture();
        let before = fx.tree();
        for (game, launcher) in [
            (Running, NotRunning),
            (NotRunning, Running),
            (Unknown, NotRunning),
        ] {
            let journal = plan_swap(&fx.install, "LIVE", "PTU", false).unwrap();
            let result = run_swap(journal, &fx.journal(), &closed_with(game, launcher));
            assert!(matches!(result, Err(Error::GameRunning)));
            assert_eq!(fx.tree(), before);
            assert!(!fx.journal().exists());
        }
    }

    #[test]
    fn swap_refuses_when_temp_folder_exists() {
        let fx = fixture();
        let temp = fx.dir(".star-config-swap");
        fs::create_dir(&temp).unwrap();
        assert!(matches!(
            plan_swap(&fx.install, "LIVE", "PTU", false),
            Err(Error::Invalid(_))
        ));
        fs::remove_dir(&temp).unwrap();

        let before = fx.tree();
        let journal = plan_swap(&fx.install, "LIVE", "PTU", false).unwrap();
        fs::create_dir(&temp).unwrap();
        let result = run_swap(journal, &fx.journal(), &CLOSED);
        assert!(matches!(result, Err(Error::Io { .. })));
        fs::remove_dir(&temp).unwrap();
        assert_eq!(fx.tree(), before);
        assert!(!fx.journal().exists());
    }

    #[test]
    fn swap_refuses_while_another_swap_is_pending() {
        let fx = fixture();
        interrupt(&fx, LAYOUTS[0], 2, 2);
        let during = fx.tree();
        let fresh = SwapJournal {
            done: 0,
            ..pending_swap(&fx.journal()).unwrap().unwrap()
        };
        assert!(matches!(
            run_swap(fresh, &fx.journal(), &CLOSED),
            Err(Error::Invalid(_))
        ));
        assert_eq!(fx.tree(), during);
        assert_eq!(pending_swap(&fx.journal()).unwrap().unwrap().done, 2);
    }

    #[test]
    fn failed_step_is_undone_and_state_matches_start() {
        let fx = lopsided();
        let journal = plan_swap(&fx.install, "LIVE", "PTU", false).unwrap();
        let blocked = fx.path("PTU/user.cfg");
        assert!(
            journal
                .steps
                .iter()
                .any(|Step::Rename { to, .. }| *to == fx.path("LIVE/user.cfg"))
        );
        fs::write(&blocked, b"planted\n").unwrap();
        let before = fx.tree();
        let result = run_swap(journal, &fx.journal(), &CLOSED);
        assert!(matches!(result, Err(Error::Io { .. })));
        assert_eq!(fx.tree(), before);
        assert!(!fx.journal().exists());
    }

    #[test]
    fn every_failed_step_is_undone_and_state_matches_start() {
        for layout in LAYOUTS {
            let steps = plan(&(layout.build)(), layout).steps.len();
            for fail_at in 0..steps {
                let fx = (layout.build)();
                let before = fx.tree();
                let mut mover = mover_failing_at(vec![fail_at]);
                let result = run_swap_with(plan(&fx, layout), &fx.journal(), &CLOSED, &mut mover);
                assert!(matches!(result, Err(Error::Io { .. })), "step {fail_at}");
                assert_eq!(fx.tree(), before, "step {fail_at}");
                assert!(!fx.journal().exists());
            }
        }
    }

    #[test]
    fn failed_undo_leaves_journal_with_true_count_and_resume_recovers() {
        let layout = LAYOUTS[0];
        let start = (layout.build)().tree();
        let swapped = clean_swap(layout);
        for (how, expected) in [(Resume::Undo, &start), (Resume::Finish, &swapped)] {
            let fx = (layout.build)();
            let mut mover = mover_failing_at(vec![5, 7]);
            let result = run_swap_with(plan(&fx, layout), &fx.journal(), &CLOSED, &mut mover);
            assert!(
                matches!(&result, Err(Error::Io { source, .. }) if source.to_string() == "injected failure")
            );
            let pending = pending_swap(&fx.journal()).unwrap().unwrap();
            assert_eq!(pending.done, 4);
            resume_swap(pending, &fx.journal(), how, &CLOSED).unwrap();
            assert_eq!(&fx.tree(), expected);
            assert_finished(&fx);
        }
    }

    #[test]
    fn resume_finish_after_each_possible_interruption() {
        for layout in LAYOUTS {
            let expected = clean_swap(layout);
            let steps = plan(&(layout.build)(), layout).steps.len();
            for k in 0..=steps {
                let fx = (layout.build)();
                let journal = interrupt(&fx, layout, k, k);
                resume_swap(journal, &fx.journal(), Resume::Finish, &CLOSED).unwrap();
                assert_eq!(fx.tree(), expected, "after {k} steps");
                assert_finished(&fx);
            }
        }
    }

    #[test]
    fn resume_undo_after_each_possible_interruption() {
        for layout in LAYOUTS {
            let steps = plan(&(layout.build)(), layout).steps.len();
            for k in 0..=steps {
                let fx = (layout.build)();
                let start = fx.tree();
                let journal = interrupt(&fx, layout, k, k);
                resume_swap(journal, &fx.journal(), Resume::Undo, &CLOSED).unwrap();
                assert_eq!(fx.tree(), start, "after {k} steps");
                assert_finished(&fx);
            }
        }
    }

    #[test]
    fn resume_detects_step_completed_but_not_recorded() {
        for layout in LAYOUTS {
            let expected = clean_swap(layout);
            let steps = plan(&(layout.build)(), layout).steps.len();
            for k in 0..steps {
                let fx = (layout.build)();
                let journal = interrupt(&fx, layout, k + 1, k);
                resume_swap(journal, &fx.journal(), Resume::Finish, &CLOSED).unwrap();
                assert_eq!(fx.tree(), expected, "finish, step {k} unrecorded");
                assert_finished(&fx);

                let fx = (layout.build)();
                let start = fx.tree();
                let journal = interrupt(&fx, layout, k + 1, k);
                resume_swap(journal, &fx.journal(), Resume::Undo, &CLOSED).unwrap();
                assert_eq!(fx.tree(), start, "undo, step {k} unrecorded");
                assert_finished(&fx);
            }
        }
    }

    #[test]
    fn resume_when_journal_is_ahead_of_the_disk() {
        for layout in LAYOUTS {
            let expected = clean_swap(layout);
            let steps = plan(&(layout.build)(), layout).steps.len();
            for ahead in [1, 2] {
                for recorded in ahead..=steps {
                    let applied = recorded - ahead;
                    let fx = (layout.build)();
                    let journal = interrupt(&fx, layout, applied, recorded);
                    resume_swap(journal, &fx.journal(), Resume::Finish, &CLOSED).unwrap();
                    assert_eq!(fx.tree(), expected, "finish, {applied} of {recorded}");
                    assert_finished(&fx);

                    let fx = (layout.build)();
                    let start = fx.tree();
                    let journal = interrupt(&fx, layout, applied, recorded);
                    resume_swap(journal, &fx.journal(), Resume::Undo, &CLOSED).unwrap();
                    assert_eq!(fx.tree(), start, "undo, {applied} of {recorded}");
                    assert_finished(&fx);
                }
            }
        }
    }

    #[test]
    fn resume_refuses_folders_that_match_no_point_of_the_swap() {
        let fx = fixture();
        let journal = interrupt(&fx, LAYOUTS[0], 1, 1);
        fs::rename(fx.dir("PTU"), fx.dir("elsewhere")).unwrap();
        let during = fx.tree();
        assert!(matches!(
            resume_swap(journal, &fx.journal(), Resume::Undo, &CLOSED),
            Err(Error::Invalid(_))
        ));
        assert_eq!(fx.tree(), during);
        assert!(fx.journal().exists());
    }

    #[cfg(unix)]
    #[test]
    fn swap_checks_the_install_folder_is_writable_before_the_journal() {
        use crate::safe_write::testing::ReadOnly;
        let fx = fixture();
        let before = fx.tree();
        let journal = plan_swap(&fx.install, "LIVE", "PTU", false).unwrap();
        let result = {
            let _guard = ReadOnly::new(&fx.install.root);
            run_swap(journal, &fx.journal(), &CLOSED)
        };
        assert!(matches!(&result, Err(Error::Io { path, .. }) if *path == fx.install.root));
        assert_eq!(fx.tree(), before);
        assert!(!fx.journal().exists());
    }

    #[cfg(unix)]
    #[test]
    fn find_pending_swap_matches_another_spelling_of_the_install() {
        let fx = fixture();
        interrupt(&fx, LAYOUTS[0], 2, 2);
        let dir = fx.tmp.path().join("journals");
        assert_eq!(
            find_pending_swap(&dir, &fx.install).unwrap().unwrap().0,
            fx.journal()
        );
        let link = fx.tmp.path().join("Games");
        std::os::unix::fs::symlink(fx.tmp.path(), &link).unwrap();
        let linked = Install {
            root: link.join("StarCitizen"),
            kind: InstallKind::Native,
        };
        let old_file = dir.join("swap-0123456789abcdef.json");
        fs::rename(fx.journal(), &old_file).unwrap();
        let (file, found) = find_pending_swap(&dir, &linked).unwrap().unwrap();
        assert_eq!(file, old_file);
        assert_eq!(found.done, 2);
        let other = empty();
        assert!(find_pending_swap(&dir, &other.install).unwrap().is_none());
        assert!(
            find_pending_swap(&fx.tmp.path().join("missing"), &fx.install)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn resume_refuses_when_running() {
        let fx = fixture();
        let journal = interrupt(&fx, LAYOUTS[0], 1, 1);
        let during = fx.tree();
        let busy = closed_with(RunState::NotRunning, RunState::Running);
        assert!(matches!(
            resume_swap(journal, &fx.journal(), Resume::Undo, &busy),
            Err(Error::GameRunning)
        ));
        assert_eq!(fx.tree(), during);
        assert!(fx.journal().exists());
    }

    #[test]
    fn pending_swap_none_without_journal_and_invalid_when_corrupt() {
        let fx = fixture();
        assert!(pending_swap(&fx.journal()).unwrap().is_none());
        fs::create_dir_all(fx.journal().parent().unwrap()).unwrap();
        fs::write(fx.journal(), b"not json\n").unwrap();
        assert!(matches!(
            pending_swap(&fx.journal()),
            Err(Error::Invalid(_))
        ));
        assert_eq!(fs::read(fx.journal()).unwrap(), b"not json\n");

        let mut outside = plan_swap(&fx.install, "LIVE", "PTU", false).unwrap();
        outside.steps.push(Step::Rename {
            from: fx.path("LIVE/user.cfg"),
            to: fx.tmp.path().join("stolen.cfg"),
        });
        fs::write(fx.journal(), serde_json::to_vec(&outside).unwrap()).unwrap();
        assert!(matches!(
            pending_swap(&fx.journal()),
            Err(Error::Invalid(_))
        ));
        assert!(fx.journal().exists());
    }

    #[test]
    fn journal_path_is_per_install() {
        let fx = fixture();
        let dir = fx.tmp.path().join("journals");
        let path = journal_path(&dir, &fx.install);
        assert_eq!(path.parent(), Some(dir.as_path()));
        assert!(path.to_string_lossy().contains(&fx.install.key()));
    }

    // settings

    #[test]
    fn part_files_map_each_part() {
        let fx = fixture();
        let live = fx.channel("LIVE");
        let mappings = live.mappings_dir();
        fs::create_dir_all(mappings.join("sub.xml")).unwrap();
        for name in ["b.xml", "A.XML", "notes.txt"] {
            fs::write(mappings.join(name), b"x\n").unwrap();
        }
        assert_eq!(part_files(&live, Part::UserCfg), [live.user_cfg()]);
        assert_eq!(part_files(&live, Part::Keybinds), [live.actionmaps_xml()]);
        assert_eq!(
            part_files(&live, Part::Layouts),
            [mappings.join("A.XML"), mappings.join("b.xml")]
        );
        assert_eq!(
            part_files(&live, Part::GameSettings),
            [live.attributes_xml()]
        );
        assert_eq!(part_files(&live, Part::StarStrings), [live.global_ini()]);
        let ptu = fx.channel("PTU");
        assert!(part_files(&ptu, Part::Layouts).is_empty());
    }

    #[test]
    fn copy_settings_writes_only_chosen_parts_and_snapshots_target() {
        let fx = fixture();
        let (live, ptu) = (fx.channel("LIVE"), fx.channel("PTU"));
        let writer = fx.writer(RunState::NotRunning);
        let info = copy_settings(
            &live,
            &ptu,
            &[Part::UserCfg, Part::Keybinds, Part::UserCfg],
            &writer,
            &scope("PTU"),
        )
        .unwrap();
        let actionmaps = "USER/Client/0/Profiles/default/actionmaps.xml";
        assert_eq!(fx.read("PTU/user.cfg"), content("LIVE", "user.cfg"));
        assert_eq!(
            fx.read(&format!("PTU/{actionmaps}")),
            content("LIVE", actionmaps)
        );
        let ini = "data/Localization/english/global.ini";
        assert_eq!(fx.read(&format!("PTU/{ini}")), content("PTU", ini));
        assert_eq!(info.label, "Copy settings from LIVE");
        let rels: Vec<&str> = info.files.iter().map(|file| file.rel.as_str()).collect();
        assert_eq!(rels, ["user.cfg", actionmaps]);
        assert_eq!(
            writer
                .store()
                .read_file(&scope("PTU"), &info.id, "user.cfg")
                .unwrap(),
            Some(content("PTU", "user.cfg"))
        );
        assert_eq!(fx.read("LIVE/user.cfg"), content("LIVE", "user.cfg"));
    }

    #[test]
    fn copy_settings_skips_files_the_source_lacks() {
        let fx = fixture();
        let attributes = "PTU/USER/Client/0/Profiles/default/attributes.xml";
        fx.write(attributes, b"ptu attributes\n");
        let (live, ptu) = (fx.channel("LIVE"), fx.channel("PTU"));
        let writer = fx.writer(RunState::NotRunning);
        let info = copy_settings(
            &live,
            &ptu,
            &[Part::UserCfg, Part::GameSettings, Part::Layouts],
            &writer,
            &scope("PTU"),
        )
        .unwrap();
        let rels: Vec<&str> = info.files.iter().map(|file| file.rel.as_str()).collect();
        assert_eq!(rels, ["user.cfg"]);
        assert_eq!(fx.read(attributes), b"ptu attributes\n");
        let result = copy_settings(&live, &ptu, &[Part::GameSettings], &writer, &scope("PTU"));
        assert!(matches!(result, Err(Error::Invalid(_))));
        assert_eq!(writer.store().list(&scope("PTU")).unwrap().len(), 1);
    }

    #[test]
    fn copy_settings_refuses_when_game_running() {
        let fx = fixture();
        let before = fx.tree();
        let writer = fx.writer(RunState::Running);
        let result = copy_settings(
            &fx.channel("LIVE"),
            &fx.channel("PTU"),
            &[Part::UserCfg],
            &writer,
            &scope("PTU"),
        );
        assert!(matches!(result, Err(Error::GameRunning)));
        assert_eq!(fx.tree(), before);
        assert!(writer.store().list(&scope("PTU")).unwrap().is_empty());
    }

    #[test]
    fn swap_settings_exchanges_and_deletes_one_sided_files() {
        let fx = fixture();
        let attributes = "USER/Client/0/Profiles/default/attributes.xml";
        let pad = "USER/Client/0/Controls/Mappings/pad.xml";
        fx.write(&format!("LIVE/{attributes}"), b"live attributes\n");
        fx.write(&format!("PTU/{pad}"), b"ptu pad\n");
        let (live, ptu) = (fx.channel("LIVE"), fx.channel("PTU"));
        let writer = fx.writer(RunState::NotRunning);
        let (first, second) = swap_settings(
            &live,
            &ptu,
            &[Part::UserCfg, Part::GameSettings, Part::Layouts],
            &writer,
            &scope("LIVE"),
            &scope("PTU"),
        )
        .unwrap();
        assert_eq!(first.label, "Swap settings with PTU");
        assert_eq!(second.label, "Swap settings with LIVE");
        assert_eq!(first.state, crate::backup::SnapshotState::Done);
        assert_eq!(second.state, crate::backup::SnapshotState::Done);
        assert!(writer.store().interrupted().unwrap().is_empty());
        assert_eq!(fx.read("LIVE/user.cfg"), content("PTU", "user.cfg"));
        assert_eq!(fx.read("PTU/user.cfg"), content("LIVE", "user.cfg"));
        assert!(!fx.path(&format!("LIVE/{attributes}")).exists());
        assert_eq!(fx.read(&format!("PTU/{attributes}")), b"live attributes\n");
        assert_eq!(fx.read(&format!("LIVE/{pad}")), b"ptu pad\n");
        assert!(!fx.path(&format!("PTU/{pad}")).exists());
        let ini = "data/Localization/english/global.ini";
        assert_eq!(fx.read(&format!("LIVE/{ini}")), content("LIVE", ini));
    }

    #[cfg(unix)]
    #[test]
    fn swap_settings_rolls_back_first_side_when_second_fails() {
        use crate::safe_write::testing::ReadOnly;
        let fx = fixture();
        let before = fx.tree();
        let (live, ptu) = (fx.channel("LIVE"), fx.channel("PTU"));
        let writer = fx.writer(RunState::NotRunning);
        let result = {
            let _guard = ReadOnly::new(&ptu.dir);
            swap_settings(
                &live,
                &ptu,
                &[Part::UserCfg, Part::Keybinds],
                &writer,
                &scope("LIVE"),
                &scope("PTU"),
            )
        };
        assert!(matches!(result, Err(Error::Io { .. })));
        assert_eq!(fx.tree(), before);
    }
}
