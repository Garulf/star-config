//! Shader cache handling.
use crate::error::{Error, Result};
use crate::install::{Install, InstallKind, resolve_ci};
use crate::process::{GameProbe, RunState};
use crate::safe_write::{self, write_atomic};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;
use time::OffsetDateTime;
use time::macros::format_description;
use walkdir::WalkDir;

const STAR_CITIZEN_DIR: &str = "Star Citizen";
const SHADERS_DIR: &str = "shaders";
const VULKAN_DIR: &str = "vulkanshadercache";
const BIN64: &str = "Bin64";
const DXVK_SUFFIX: &str = ".dxvk-cache";
const VKD3D_PREFIX: &str = "vkd3d-proton.cache";
const UNVERSIONED: &str = "unversioned";
const BACKUP_META: &str = "backup.toml";
const OLD_SUFFIX: &str = ".star-config.old";
const NOT_VERSION_DIRS: [&str; 1] = ["graphicssettings"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheKind {
    Shaders,
    VulkanShaderCache,
    DxvkFile,
    Vkd3dFile,
}

impl CacheKind {
    fn is_folder(self) -> bool {
        matches!(self, Self::Shaders | Self::VulkanShaderCache)
    }

    fn recognises(self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        match self {
            Self::Shaders => lower == SHADERS_DIR,
            Self::VulkanShaderCache => lower == VULKAN_DIR,
            Self::DxvkFile => lower.len() > DXVK_SUFFIX.len() && lower.ends_with(DXVK_SUFFIX),
            Self::Vkd3dFile => lower.starts_with(VKD3D_PREFIX),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Shaders => "shaders",
            Self::VulkanShaderCache => "vulkan",
            Self::DxvkFile => "dxvk",
            Self::Vkd3dFile => "vkd3d",
        }
    }

    fn from_label(label: &str) -> Option<Self> {
        [
            Self::Shaders,
            Self::VulkanShaderCache,
            Self::DxvkFile,
            Self::Vkd3dFile,
        ]
        .into_iter()
        .find(|kind| kind.label() == label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Current,
    Unmatched,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cache {
    pub path: PathBuf,
    /// The "Star Citizen" folder, or the channel folder for the two file kinds.
    pub root: PathBuf,
    /// Version folder name, `None` when the cache sits directly under `root`.
    pub version: Option<String>,
    pub kind: CacheKind,
    pub size: u64,
    pub modified_unix: i64,
    pub tag: Tag,
}

fn entries(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = read
        .filter_map(|entry| entry.ok())
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                entry.path(),
            )
        })
        .collect();
    found.sort();
    found
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

fn is_real_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file())
}

pub fn appdata_roots(install: &Install, native_local_appdata: Option<&Path>) -> Vec<PathBuf> {
    match &install.kind {
        InstallKind::Native => native_local_appdata
            .map(Path::to_path_buf)
            .into_iter()
            .collect(),
        InstallKind::WinePrefix { prefix } => {
            let Some(users) = resolve_ci(prefix, "drive_c/users") else {
                return Vec::new();
            };
            entries(&users)
                .into_iter()
                .filter(|(name, path)| !name.eq_ignore_ascii_case("public") && is_real_dir(path))
                .filter_map(|(_, user)| resolve_ci(&user, "AppData/Local"))
                .filter(|local| local.is_dir())
                .collect()
        }
    }
}

fn dir_size(path: &Path) -> u64 {
    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| entry.metadata().ok())
        .map(|meta| meta.len())
        .sum()
}

fn tree_size(path: &Path) -> u64 {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => meta.len(),
        Ok(meta) if meta.is_dir() => dir_size(path),
        _ => 0,
    }
}

fn modified_unix(path: &Path) -> i64 {
    fs::symlink_metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_secs() as i64)
}

fn current_versions(install: &Install) -> Vec<String> {
    install
        .channels()
        .iter()
        .filter_map(|channel| channel.build_id())
        .map(|id| id.split(" (").next().unwrap_or("").trim().to_lowercase())
        .filter(|version| !version.is_empty())
        .collect()
}

fn folder_cache(
    path: PathBuf,
    kind: CacheKind,
    root: &Path,
    version: Option<&str>,
    current: &[String],
) -> Cache {
    let lower = version.map(str::to_lowercase);
    let tag = match lower {
        Some(lower) if current.iter().any(|v| lower.contains(v.as_str())) => Tag::Current,
        _ => Tag::Unmatched,
    };
    Cache {
        size: dir_size(&path),
        modified_unix: modified_unix(&path),
        path,
        root: root.to_path_buf(),
        version: version.map(str::to_string),
        kind,
        tag,
    }
}

fn folder_kind(name: &str) -> Option<CacheKind> {
    [CacheKind::Shaders, CacheKind::VulkanShaderCache]
        .into_iter()
        .find(|kind| kind.recognises(name))
}

fn cache_folders_in(
    dir: &Path,
    root: &Path,
    version: Option<&str>,
    current: &[String],
) -> Vec<Cache> {
    entries(dir)
        .into_iter()
        .filter(|(_, path)| is_real_dir(path))
        .filter_map(|(name, path)| Some((folder_kind(&name)?, path)))
        .map(|(kind, path)| folder_cache(path, kind, root, version, current))
        .collect()
}

fn discover_folders(appdata_root: &Path, current: &[String]) -> Vec<Cache> {
    let Some(sc) = resolve_ci(appdata_root, STAR_CITIZEN_DIR).filter(|sc| is_real_dir(sc)) else {
        return Vec::new();
    };
    let mut caches = cache_folders_in(&sc, &sc, None, current);
    for (name, path) in entries(&sc) {
        let lower = name.to_ascii_lowercase();
        let is_cache = folder_kind(&name).is_some();
        if is_cache || NOT_VERSION_DIRS.contains(&lower.as_str()) || !is_real_dir(&path) {
            continue;
        }
        caches.extend(cache_folders_in(&path, &sc, Some(&name), current));
    }
    caches
}

fn file_kind(name: &str) -> Option<CacheKind> {
    [CacheKind::DxvkFile, CacheKind::Vkd3dFile]
        .into_iter()
        .find(|kind| kind.recognises(name))
}

fn discover_files(install: &Install) -> Vec<Cache> {
    let mut caches = Vec::new();
    for channel in install.channels() {
        let dirs = [Some(channel.dir.clone()), resolve_ci(&channel.dir, BIN64)];
        for dir in dirs.into_iter().flatten() {
            for (name, path) in entries(&dir) {
                let Some(kind) = file_kind(&name) else {
                    continue;
                };
                if !is_real_file(&path) {
                    continue;
                }
                caches.push(Cache {
                    size: tree_size(&path),
                    modified_unix: modified_unix(&path),
                    path,
                    root: channel.dir.clone(),
                    version: Some(channel.name.clone()),
                    kind,
                    tag: Tag::Current,
                });
            }
        }
    }
    caches
}

pub fn discover(install: &Install, appdata_roots: &[PathBuf]) -> Vec<Cache> {
    let current = current_versions(install);
    let mut caches: Vec<Cache> = appdata_roots
        .iter()
        .flat_map(|root| discover_folders(root, &current))
        .collect();
    caches.extend(discover_files(install));
    caches
}

/// Resolves `path` through its nearest existing ancestor so a path that does not exist yet can
/// still be compared against a canonical root.
fn canonical_lenient(path: &Path) -> Result<PathBuf> {
    let mut missing = Vec::new();
    let mut current = path;
    loop {
        match fs::canonicalize(current) {
            Ok(mut resolved) => {
                resolved.extend(missing.iter().rev());
                return Ok(resolved);
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {
                let (Some(parent), Some(name)) = (current.parent(), current.file_name()) else {
                    return Err(Error::io(path, err));
                };
                missing.push(name.to_owned());
                current = parent;
            }
            Err(err) => return Err(Error::io(path, err)),
        }
    }
}

fn outside(path: &Path, why: &str) -> Error {
    Error::Invalid(format!("refusing to touch {}: {why}", path.display()))
}

fn is_bin64(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().eq_ignore_ascii_case(BIN64)
}

fn placed_correctly(kind: CacheKind, relative: &Path) -> bool {
    let parts: Vec<_> = relative.components().collect();
    let plain = parts
        .iter()
        .all(|part| matches!(part, Component::Normal(_)));
    if !plain {
        return false;
    }
    match (kind.is_folder(), parts.as_slice()) {
        (true, [_]) => true,
        (true, [Component::Normal(version), _]) => {
            !NOT_VERSION_DIRS.contains(&version.to_string_lossy().to_ascii_lowercase().as_str())
        }
        (false, [_]) => true,
        (false, [Component::Normal(dir), _]) => is_bin64(dir),
        _ => false,
    }
}

/// The checks every destructive operation runs first, in this order: game not running, path
/// exists (when `must_exist`), not a symlink, recognised name for the kind, inside `root`.
fn check_destructive(
    probe: &dyn GameProbe,
    path: &Path,
    kind: CacheKind,
    root: &Path,
    must_exist: bool,
) -> Result<()> {
    if probe.game() != RunState::NotRunning {
        return Err(Error::GameRunning);
    }
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => Some(meta),
        Err(err) if err.kind() == ErrorKind::NotFound && !must_exist => None,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return Err(Error::NotFound(path.display().to_string()));
        }
        Err(err) => return Err(Error::io(path, err)),
    };
    if meta.as_ref().is_some_and(|meta| meta.is_symlink()) {
        return Err(outside(path, "it is a symlink"));
    }
    let name = path.file_name().map(|name| name.to_string_lossy());
    if !name.is_some_and(|name| kind.recognises(&name)) {
        return Err(outside(path, "its name is not a shader cache name"));
    }
    if let Some(meta) = &meta
        && meta.is_dir() != kind.is_folder()
    {
        return Err(outside(
            path,
            "it is not the kind of entry a cache should be",
        ));
    }
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(outside(path, "it contains relative components"));
    }
    if kind.is_folder()
        && !root.file_name().is_some_and(|name| {
            name.to_string_lossy()
                .eq_ignore_ascii_case(STAR_CITIZEN_DIR)
        })
    {
        return Err(outside(root, "it is not the Star Citizen folder"));
    }
    let canonical_root = fs::canonicalize(root).map_err(|err| Error::io(root, err))?;
    let canonical = canonical_lenient(path)?;
    let inside = canonical
        .strip_prefix(&canonical_root)
        .is_ok_and(|relative| placed_correctly(kind, relative));
    if !inside {
        return Err(outside(path, "it is not where this cache belongs"));
    }
    Ok(())
}

fn remove_tree(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path).map_err(|err| Error::io(path, err))?;
    let removed = if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    removed.map_err(|err| Error::io(path, err))
}

pub fn clear(cache: &Cache, probe: &dyn GameProbe) -> Result<u64> {
    check_destructive(probe, &cache.path, cache.kind, &cache.root, true)?;
    let bytes = tree_size(&cache.path);
    remove_tree(&cache.path)?;
    Ok(bytes)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderBackup {
    pub id: String,
    pub version: String,
    pub kind: CacheKind,
    pub size: u64,
    pub path: PathBuf,
    pub created_unix: i64,
    /// False while the data is still being moved in, or when that move was interrupted. An
    /// incomplete backup can be deleted but not restored.
    pub complete: bool,
}

#[derive(Debug, Clone)]
pub struct ShaderBackups {
    root: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct BackupMeta {
    kind: String,
    size: u64,
    created_unix: i64,
    original: String,
    #[serde(default)]
    complete: bool,
}

fn write_meta(path: &Path, meta: &BackupMeta) -> Result<()> {
    let text = toml::to_string(meta).map_err(|err| Error::Invalid(err.to_string()))?;
    write_atomic(path, text.as_bytes())
}

fn plain_component(text: &str) -> bool {
    let mut parts = Path::new(text).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(part)), None) if part == text
    )
}

fn timestamp_id(now: OffsetDateTime) -> String {
    let format = format_description!("[year][month][day]T[hour][minute][second]Z");
    now.format(&format)
        .unwrap_or_else(|_| now.unix_timestamp().to_string())
}

fn ensure_free_destination(source: &Path, destination: &Path) -> Result<()> {
    if fs::symlink_metadata(destination).is_ok() {
        return Err(Error::Invalid(format!(
            "{} already exists",
            destination.display()
        )));
    }
    match destination.parent() {
        Some(parent) if parent.is_dir() => Ok(()),
        _ => Err(outside(source, "the destination folder is missing")),
    }
}

fn file_listing(root: &Path) -> Result<Vec<(PathBuf, u64)>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|err| Error::Invalid(err.to_string()))?;
        if entry.file_type().is_file() {
            let meta = entry
                .metadata()
                .map_err(|err| Error::Invalid(err.to_string()))?;
            let relative = entry.path().strip_prefix(root).unwrap_or(Path::new(""));
            files.push((relative.to_path_buf(), meta.len()));
        }
    }
    files.sort();
    Ok(files)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    let mut dirs = Vec::new();
    for entry in WalkDir::new(source).follow_links(false) {
        let entry = entry.map_err(|err| Error::Invalid(err.to_string()))?;
        let relative = entry.path().strip_prefix(source).unwrap_or(Path::new(""));
        let target = if relative.as_os_str().is_empty() {
            destination.to_path_buf()
        } else {
            destination.join(relative)
        };
        let kind = entry.file_type();
        if kind.is_dir() {
            fs::create_dir_all(&target).map_err(|err| Error::io(&target, err))?;
            dirs.push(target);
        } else if kind.is_file() {
            fs::copy(entry.path(), &target).map_err(|err| Error::io(&target, err))?;
            safe_write::sync_file(&target).map_err(|err| Error::io(&target, err))?;
        }
    }
    for dir in dirs.iter().rev() {
        safe_write::sync_dir(dir);
    }
    if let Some(parent) = destination.parent() {
        safe_write::sync_dir(parent);
    }
    Ok(())
}

/// Copies `source` to `destination`, checks the copy against the source file by file, and only
/// then removes the source. `after_copy` runs between the copy and the check.
fn copy_verify_delete(
    source: &Path,
    destination: &Path,
    after_copy: Option<&dyn Fn(&Path)>,
) -> Result<()> {
    ensure_free_destination(source, destination)?;
    let copied = copy_tree(source, destination).and_then(|()| {
        if let Some(hook) = after_copy {
            hook(destination);
        }
        let matches = file_listing(source)? == file_listing(destination)?;
        matches.then_some(()).ok_or_else(|| {
            Error::Invalid(format!(
                "the copy of {} does not match the original",
                source.display()
            ))
        })
    });
    if let Err(err) = copied {
        let _ = remove_tree(destination);
        return Err(err);
    }
    remove_tree(source)
}

fn move_tree(source: &Path, destination: &Path) -> Result<()> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::CrossesDevices => {
            copy_verify_delete(source, destination, None)
        }
        Err(err) => Err(Error::io(source, err)),
    }
}

impl ShaderBackups {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn id_dir(&self, version: &str, id: &str) -> Result<PathBuf> {
        if !plain_component(version) || !plain_component(id) {
            return Err(Error::Invalid(format!(
                "bad backup reference {version}/{id}"
            )));
        }
        let dir = self.root.join(version).join(id);
        let parent = self.root.join(version);
        if !is_real_dir(&parent) || !is_real_dir(&dir) {
            return Err(Error::NotFound(format!("backup {version}/{id}")));
        }
        Ok(dir)
    }

    fn read(&self, version: &str, id: &str) -> Result<ShaderBackup> {
        let dir = self.id_dir(version, id)?;
        let meta_path = dir.join(BACKUP_META);
        let text = fs::read_to_string(&meta_path).map_err(|err| Error::io(&meta_path, err))?;
        let invalid = |message: &str| Error::Invalid(format!("{}: {message}", meta_path.display()));
        let meta: BackupMeta = toml::from_str(&text).map_err(|err| invalid(&err.to_string()))?;
        let kind = CacheKind::from_label(&meta.kind).ok_or_else(|| invalid("unknown kind"))?;
        let name = Path::new(&meta.original)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| kind.recognises(name) && plain_component(name))
            .ok_or_else(|| invalid("unrecognised cache name"))?;
        let path = dir.join(name);
        if meta.complete && fs::symlink_metadata(&path).is_err() {
            return Err(invalid("backup data is missing"));
        }
        let size = if meta.complete {
            meta.size
        } else {
            tree_size(&path)
        };
        Ok(ShaderBackup {
            id: id.to_string(),
            version: version.to_string(),
            kind,
            size,
            path,
            created_unix: meta.created_unix,
            complete: meta.complete,
        })
    }

    fn original_of(&self, backup: &ShaderBackup) -> Result<PathBuf> {
        let dir = self.id_dir(&backup.version, &backup.id)?;
        let meta_path = dir.join(BACKUP_META);
        let text = fs::read_to_string(&meta_path).map_err(|err| Error::io(&meta_path, err))?;
        let meta: BackupMeta = toml::from_str(&text)
            .map_err(|err| Error::Invalid(format!("{}: {err}", meta_path.display())))?;
        Ok(PathBuf::from(meta.original))
    }

    fn fresh_id_dir(&self, version: &str) -> Result<(String, PathBuf, i64)> {
        let now = OffsetDateTime::now_utc();
        let base = timestamp_id(now);
        let parent = self.root.join(version);
        fs::create_dir_all(&parent).map_err(|err| Error::io(&parent, err))?;
        for attempt in 1.. {
            let id = if attempt == 1 {
                base.clone()
            } else {
                format!("{base}-{attempt}")
            };
            let dir = parent.join(&id);
            match fs::create_dir(&dir) {
                Ok(()) => return Ok((id, dir, now.unix_timestamp())),
                Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(Error::io(&dir, err)),
            }
        }
        unreachable!()
    }

    pub fn backup_and_clear(&self, cache: &Cache, probe: &dyn GameProbe) -> Result<ShaderBackup> {
        check_destructive(probe, &cache.path, cache.kind, &cache.root, true)?;
        let name = cache
            .path
            .file_name()
            .ok_or_else(|| outside(&cache.path, "it has no name"))?;
        let version = cache.version.as_deref().unwrap_or(UNVERSIONED);
        if !plain_component(version) {
            return Err(Error::Invalid(format!("bad version folder name {version}")));
        }
        let size = tree_size(&cache.path);
        let (id, dir, created_unix) = self.fresh_id_dir(version)?;
        let stored = dir.join(name);
        let meta_path = dir.join(BACKUP_META);
        let mut meta = BackupMeta {
            kind: cache.kind.label().to_string(),
            size,
            created_unix,
            original: cache.path.to_string_lossy().into_owned(),
            complete: false,
        };
        if let Err(err) = write_meta(&meta_path, &meta) {
            let _ = fs::remove_dir(&dir);
            return Err(err);
        }
        if let Err(err) = move_tree(&cache.path, &stored) {
            if fs::symlink_metadata(&stored).is_err() {
                let _ = fs::remove_file(&meta_path);
                let _ = fs::remove_dir(&dir);
            }
            return Err(err);
        }
        meta.complete = true;
        write_meta(&meta_path, &meta).map_err(|err| {
            Error::Invalid(format!(
                "moved {} to {} but could not mark the backup complete, so it is listed as incomplete: {err}",
                cache.path.display(),
                stored.display(),
            ))
        })?;
        Ok(ShaderBackup {
            id,
            version: version.to_string(),
            kind: cache.kind,
            size,
            path: stored,
            created_unix,
            complete: true,
        })
    }

    pub fn list(&self) -> Vec<ShaderBackup> {
        let mut backups: Vec<ShaderBackup> = entries(&self.root)
            .into_iter()
            .filter(|(_, path)| is_real_dir(path))
            .flat_map(|(version, version_dir)| {
                entries(&version_dir)
                    .into_iter()
                    .filter_map(move |(id, _)| self.read(&version, &id).ok())
            })
            .collect();
        backups.sort_by(|a, b| {
            (b.created_unix, &b.id, &b.version).cmp(&(a.created_unix, &a.id, &a.version))
        });
        backups
    }

    pub fn total_size(&self) -> u64 {
        self.list().iter().map(|backup| backup.size).sum()
    }

    pub fn restore(
        &self,
        backup: &ShaderBackup,
        cache_root: &Path,
        probe: &dyn GameProbe,
    ) -> Result<PathBuf> {
        let stored = self.read(&backup.version, &backup.id)?;
        if !stored.complete {
            return Err(Error::Invalid(format!(
                "backup {}/{} is incomplete; delete it instead",
                stored.version, stored.id
            )));
        }
        let name = stored
            .path
            .file_name()
            .ok_or_else(|| outside(&stored.path, "it has no name"))?
            .to_owned();
        let destination = self.destination_for(&stored, &name, cache_root)?;
        check_destructive(probe, &destination, stored.kind, cache_root, false)?;
        let parent = destination
            .parent()
            .ok_or_else(|| outside(&destination, "it has no parent"))?;
        fs::create_dir_all(parent).map_err(|err| Error::io(parent, err))?;
        let displaced = self.displace_existing(&destination)?;
        if let Err(err) = move_tree(&stored.path, &destination) {
            if let Some(old) = &displaced {
                let _ = fs::rename(old, &destination);
            }
            return Err(err);
        }
        if let Some(old) = &displaced {
            remove_tree(old)?;
        }
        let id_dir = self.id_dir(&stored.version, &stored.id)?;
        remove_tree(&id_dir)?;
        let _ = fs::remove_dir(self.root.join(&stored.version));
        Ok(destination)
    }

    fn destination_for(
        &self,
        stored: &ShaderBackup,
        name: &std::ffi::OsStr,
        cache_root: &Path,
    ) -> Result<PathBuf> {
        if !stored.kind.is_folder() {
            let in_bin64 = self
                .original_of(stored)?
                .parent()
                .and_then(Path::file_name)
                .is_some_and(is_bin64);
            let dir = match (in_bin64, resolve_ci(cache_root, BIN64)) {
                (true, Some(bin64)) => bin64,
                (true, None) => cache_root.join(BIN64),
                (false, _) => cache_root.to_path_buf(),
            };
            return Ok(dir.join(name));
        }
        if stored.version == UNVERSIONED {
            return Ok(cache_root.join(name));
        }
        Ok(cache_root.join(&stored.version).join(name))
    }

    fn displace_existing(&self, destination: &Path) -> Result<Option<PathBuf>> {
        if fs::symlink_metadata(destination).is_err() {
            return Ok(None);
        }
        let mut old_name = destination.as_os_str().to_owned();
        old_name.push(OLD_SUFFIX);
        let old = PathBuf::from(old_name);
        if fs::symlink_metadata(&old).is_ok() {
            return Err(Error::Invalid(format!("{} already exists", old.display())));
        }
        fs::rename(destination, &old).map_err(|err| Error::io(destination, err))?;
        Ok(Some(old))
    }

    pub fn delete(&self, backup: &ShaderBackup) -> Result<()> {
        let dir = self.id_dir(&backup.version, &backup.id)?;
        if !is_real_file(&dir.join(BACKUP_META)) {
            return Err(Error::NotFound(format!(
                "backup {}/{}",
                backup.version, backup.id
            )));
        }
        remove_tree(&dir)?;
        let _ = fs::remove_dir(self.root.join(&backup.version));
        Ok(())
    }
}

#[cfg(unix)]
pub fn same_volume(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev(),
        _ => false,
    }
}

#[cfg(windows)]
pub fn same_volume(a: &Path, b: &Path) -> bool {
    let prefix = |path: &Path| match path.components().next() {
        Some(Component::Prefix(prefix)) => {
            Some(prefix.as_os_str().to_string_lossy().to_ascii_lowercase())
        }
        _ => None,
    };
    match (prefix(a), prefix(b)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

#[cfg(not(any(unix, windows)))]
pub fn same_volume(_: &Path, _: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::FixedProbe;
    use std::fs;
    use tempfile::{TempDir, tempdir};

    const IDLE: FixedProbe = FixedProbe {
        game: RunState::NotRunning,
        launcher: RunState::NotRunning,
    };

    fn write(path: &Path, len: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![7u8; len]).unwrap();
    }

    struct Fx {
        tmp: TempDir,
        install: Install,
        local: PathBuf,
        sc: PathBuf,
    }

    fn fixture() -> Fx {
        let tmp = tempdir().unwrap();
        let prefix = tmp.path().join("pfx");
        let root = prefix.join("drive_c/Program Files/Roberts Space Industries/StarCitizen");
        write(&root.join("LIVE/Data.p4k"), 0);
        write(&root.join("LIVE/build_manifest.id"), 0);
        fs::write(
            root.join("LIVE/build_manifest.id"),
            r#"{"Data":{"Version":"4.10.3","RequestedP4ChangeNum":"13165946"}}"#,
        )
        .unwrap();
        let local = prefix.join("drive_c/users/alice/AppData/Local");
        let sc = local.join("Star Citizen");
        write(&sc.join("sc-alpha-4.10.3_abc/shaders/x.bin"), 100);
        write(&sc.join("sc-alpha-4.10.3_abc/shaders/sub/y.bin"), 50);
        write(&sc.join("sc-alpha-4.9.0_x/shaders/z.bin"), 10);
        write(&sc.join("vulkanshadercache/v.bin"), 5);
        write(&sc.join("GraphicsSettings/shaders/g.bin"), 1);
        write(&sc.join("sc-alpha-4.10.3_abc/other/shaders/q.bin"), 1);
        write(&sc.join("sc-alpha-4.10.3_abc/Deep/er/shaders/q.bin"), 1);
        write(
            &prefix.join("drive_c/users/Public/AppData/Local/Star Citizen/v1/shaders/p.bin"),
            1,
        );
        let install = Install {
            root,
            kind: InstallKind::WinePrefix { prefix },
        };
        Fx {
            tmp,
            install,
            local,
            sc,
        }
    }

    fn found(fx: &Fx) -> Vec<Cache> {
        discover(&fx.install, &appdata_roots(&fx.install, None))
    }

    fn cache_at(caches: &[Cache], suffix: &str) -> Cache {
        caches
            .iter()
            .find(|c| c.path.ends_with(suffix))
            .unwrap_or_else(|| panic!("no cache {suffix}: {caches:?}"))
            .clone()
    }

    fn running() -> FixedProbe {
        FixedProbe {
            game: RunState::Running,
            launcher: RunState::NotRunning,
        }
    }

    fn unknown() -> FixedProbe {
        FixedProbe {
            game: RunState::Unknown,
            launcher: RunState::NotRunning,
        }
    }

    #[test]
    fn appdata_roots_lists_prefix_users_and_skips_public() {
        let fx = fixture();
        let roots = appdata_roots(&fx.install, None);
        assert_eq!(roots, vec![fx.local.clone()]);
    }

    #[test]
    fn appdata_roots_native_uses_given_dir() {
        let tmp = tempdir().unwrap();
        let install = Install {
            root: tmp.path().join("StarCitizen"),
            kind: InstallKind::Native,
        };
        assert!(appdata_roots(&install, None).is_empty());
        assert_eq!(
            appdata_roots(&install, Some(tmp.path())),
            vec![tmp.path().to_path_buf()]
        );
    }

    #[test]
    fn discover_finds_versioned_and_unversioned_caches_only() {
        let fx = fixture();
        let caches = found(&fx);
        let mut paths: Vec<_> = caches
            .iter()
            .map(|c| c.path.strip_prefix(&fx.sc).unwrap().to_path_buf())
            .collect();
        paths.sort();
        assert_eq!(
            paths,
            vec![
                PathBuf::from("sc-alpha-4.10.3_abc/shaders"),
                PathBuf::from("sc-alpha-4.9.0_x/shaders"),
                PathBuf::from("vulkanshadercache"),
            ]
        );
        let unversioned = cache_at(&caches, "vulkanshadercache");
        assert_eq!(unversioned.version, None);
        assert_eq!(unversioned.kind, CacheKind::VulkanShaderCache);
        assert_eq!(unversioned.root, fx.sc);
        let versioned = cache_at(&caches, "sc-alpha-4.9.0_x/shaders");
        assert_eq!(versioned.version.as_deref(), Some("sc-alpha-4.9.0_x"));
        assert_eq!(versioned.kind, CacheKind::Shaders);
    }

    #[test]
    fn discover_is_case_insensitive_about_folder_names() {
        let tmp = tempdir().unwrap();
        let local = tmp.path().join("local");
        write(&local.join("star citizen/v1/Shaders/a.bin"), 3);
        write(&local.join("star citizen/VulkanShaderCache/b.bin"), 4);
        let install = Install {
            root: tmp.path().join("StarCitizen"),
            kind: InstallKind::Native,
        };
        let caches = discover(&install, std::slice::from_ref(&local));
        assert_eq!(caches.len(), 2);
        assert_eq!(cache_at(&caches, "v1/Shaders").size, 3);
        assert_eq!(cache_at(&caches, "VulkanShaderCache").size, 4);
    }

    #[cfg(unix)]
    #[test]
    fn discover_skips_symlinked_candidates() {
        let fx = fixture();
        let outside = fx.tmp.path().join("outside");
        write(&outside.join("o.bin"), 9);
        let linked = fx.sc.join("sc-alpha-4.11.0_n");
        fs::create_dir_all(&linked).unwrap();
        std::os::unix::fs::symlink(&outside, linked.join("shaders")).unwrap();
        std::os::unix::fs::symlink(&outside, fx.sc.join("linkedversion")).unwrap();
        let caches = found(&fx);
        assert_eq!(caches.len(), 3);
        assert!(caches.iter().all(|c| !c.path.starts_with(&linked)));
    }

    #[test]
    fn discover_lists_dxvk_and_vkd3d_files_in_channel_and_bin64() {
        let fx = fixture();
        let live = fx.install.root.join("LIVE");
        write(&live.join("StarCitizen.dxvk-cache"), 11);
        write(&live.join("Bin64/vkd3d-proton.cache.write"), 12);
        write(&live.join("Bin64/notes.txt"), 1);
        let caches = found(&fx);
        let dxvk = cache_at(&caches, "StarCitizen.dxvk-cache");
        assert_eq!(dxvk.kind, CacheKind::DxvkFile);
        assert_eq!(dxvk.size, 11);
        assert_eq!(dxvk.root, live);
        assert_eq!(dxvk.version.as_deref(), Some("LIVE"));
        assert_eq!(dxvk.tag, Tag::Current);
        let vkd3d = cache_at(&caches, "vkd3d-proton.cache.write");
        assert_eq!(vkd3d.kind, CacheKind::Vkd3dFile);
        assert_eq!(vkd3d.tag, Tag::Current);
        assert!(!caches.iter().any(|c| c.path.ends_with("notes.txt")));
    }

    #[test]
    fn tag_current_when_version_folder_contains_a_channel_version() {
        let fx = fixture();
        let caches = found(&fx);
        assert_eq!(
            cache_at(&caches, "sc-alpha-4.10.3_abc/shaders").tag,
            Tag::Current
        );
        assert_eq!(
            cache_at(&caches, "sc-alpha-4.9.0_x/shaders").tag,
            Tag::Unmatched
        );
        assert_eq!(cache_at(&caches, "vulkanshadercache").tag, Tag::Unmatched);
    }

    #[test]
    fn tag_unmatched_when_no_build_ids() {
        let fx = fixture();
        fs::remove_file(fx.install.root.join("LIVE/build_manifest.id")).unwrap();
        assert!(found(&fx).iter().all(|c| c.tag == Tag::Unmatched));
    }

    #[test]
    fn size_sums_regular_files() {
        let fx = fixture();
        let caches = found(&fx);
        assert_eq!(cache_at(&caches, "sc-alpha-4.10.3_abc/shaders").size, 150);
    }

    #[test]
    fn clear_refuses_when_game_running_or_unknown() {
        let fx = fixture();
        let cache = cache_at(&found(&fx), "sc-alpha-4.10.3_abc/shaders");
        for probe in [running(), unknown()] {
            assert!(matches!(clear(&cache, &probe), Err(Error::GameRunning)));
            assert!(cache.path.join("x.bin").exists());
        }
    }

    #[test]
    fn clear_removes_folder_and_returns_bytes() {
        let fx = fixture();
        let cache = cache_at(&found(&fx), "sc-alpha-4.10.3_abc/shaders");
        assert_eq!(clear(&cache, &IDLE).unwrap(), 150);
        assert!(!cache.path.exists());
        assert!(fx.sc.join("sc-alpha-4.10.3_abc/other").exists());
    }

    #[test]
    fn clear_removes_cache_files() {
        let fx = fixture();
        let live = fx.install.root.join("LIVE");
        write(&live.join("Bin64/a.dxvk-cache"), 8);
        let cache = cache_at(&found(&fx), "a.dxvk-cache");
        assert_eq!(clear(&cache, &IDLE).unwrap(), 8);
        assert!(!cache.path.exists());
        assert!(live.join("Data.p4k").exists());
    }

    #[cfg(unix)]
    #[test]
    fn clear_refuses_path_swapped_for_a_symlink() {
        let fx = fixture();
        let cache = cache_at(&found(&fx), "sc-alpha-4.10.3_abc/shaders");
        let outside = fx.tmp.path().join("outside");
        write(&outside.join("keep.bin"), 9);
        fs::remove_dir_all(&cache.path).unwrap();
        std::os::unix::fs::symlink(&outside, &cache.path).unwrap();
        assert!(matches!(clear(&cache, &IDLE), Err(Error::Invalid(_))));
        assert!(outside.join("keep.bin").exists());
    }

    #[test]
    fn clear_refuses_renamed_or_relocated_path() {
        let fx = fixture();
        let hand_built = |path: PathBuf| Cache {
            path,
            root: fx.sc.clone(),
            version: None,
            kind: CacheKind::Shaders,
            size: 0,
            modified_unix: 0,
            tag: Tag::Unmatched,
        };
        let renamed = fx.sc.join("GraphicsSettings");
        let wrong_place = fx.sc.join("GraphicsSettings/shaders");
        let outside = fx.tmp.path().join("shaders");
        let deep = fx.sc.join("sc-alpha-4.10.3_abc/Deep/er/shaders");
        let dotted = fx.sc.join("sc-alpha-4.9.0_x/../GraphicsSettings/shaders");
        write(&outside.join("a.bin"), 1);
        for path in [renamed, wrong_place, outside, deep, dotted, fx.sc.clone()] {
            let cache = hand_built(path.clone());
            assert!(
                matches!(clear(&cache, &IDLE), Err(Error::Invalid(_))),
                "{}",
                path.display()
            );
            assert!(path.exists());
        }
    }

    #[test]
    fn clear_reports_missing_path_distinctly() {
        let fx = fixture();
        let mut cache = cache_at(&found(&fx), "sc-alpha-4.10.3_abc/shaders");
        fs::remove_dir_all(&cache.path).unwrap();
        cache.size = 0;
        assert!(matches!(clear(&cache, &IDLE), Err(Error::NotFound(_))));
    }

    #[cfg(unix)]
    #[test]
    fn clear_does_not_follow_symlinks_inside() {
        let fx = fixture();
        let cache = cache_at(&found(&fx), "sc-alpha-4.10.3_abc/shaders");
        let outside_file = fx.tmp.path().join("outside.txt");
        let outside_dir = fx.tmp.path().join("outside_dir");
        fs::write(&outside_file, b"keep").unwrap();
        write(&outside_dir.join("k.bin"), 4);
        std::os::unix::fs::symlink(&outside_file, cache.path.join("link")).unwrap();
        std::os::unix::fs::symlink(&outside_dir, cache.path.join("dirlink")).unwrap();
        assert_eq!(clear(&cache, &IDLE).unwrap(), 150);
        assert!(!cache.path.exists());
        assert!(outside_file.exists());
        assert!(outside_dir.join("k.bin").exists());
    }

    fn backups(fx: &Fx) -> ShaderBackups {
        ShaderBackups::new(fx.tmp.path().join("backups"))
    }

    #[test]
    fn backup_and_clear_moves_data_and_lists_it() {
        let fx = fixture();
        let store = backups(&fx);
        let cache = cache_at(&found(&fx), "sc-alpha-4.10.3_abc/shaders");
        let backup = store.backup_and_clear(&cache, &IDLE).unwrap();
        assert!(!cache.path.exists());
        assert_eq!(backup.version, "sc-alpha-4.10.3_abc");
        assert_eq!(backup.kind, CacheKind::Shaders);
        assert_eq!(backup.size, 150);
        assert!(backup.path.join("x.bin").exists());
        assert!(backup.path.starts_with(fx.tmp.path().join("backups")));
        assert_eq!(store.list(), vec![backup]);
    }

    #[test]
    fn backup_and_clear_refuses_when_game_running() {
        let fx = fixture();
        let cache = cache_at(&found(&fx), "sc-alpha-4.10.3_abc/shaders");
        let result = backups(&fx).backup_and_clear(&cache, &running());
        assert!(matches!(result, Err(Error::GameRunning)));
        assert!(cache.path.exists());
    }

    #[test]
    fn backup_and_clear_refuses_relocated_path() {
        let fx = fixture();
        let mut cache = cache_at(&found(&fx), "sc-alpha-4.10.3_abc/shaders");
        cache.path = fx.sc.join("GraphicsSettings/shaders");
        let store = backups(&fx);
        assert!(matches!(
            store.backup_and_clear(&cache, &IDLE),
            Err(Error::Invalid(_))
        ));
        assert!(cache.path.exists());
        assert!(store.list().is_empty());
    }

    #[test]
    fn unversioned_cache_is_backed_up_under_unversioned() {
        let fx = fixture();
        let store = backups(&fx);
        let cache = cache_at(&found(&fx), "vulkanshadercache");
        let backup = store.backup_and_clear(&cache, &IDLE).unwrap();
        assert!(
            backup
                .path
                .starts_with(fx.tmp.path().join("backups/unversioned"))
        );
    }

    #[test]
    fn second_backup_of_same_version_keeps_both() {
        let fx = fixture();
        let store = backups(&fx);
        let cache = cache_at(&found(&fx), "sc-alpha-4.9.0_x/shaders");
        let first = store.backup_and_clear(&cache, &IDLE).unwrap();
        write(&cache.path.join("again.bin"), 20);
        let second = store.backup_and_clear(&cache, &IDLE).unwrap();
        assert_ne!(first.id, second.id);
        assert!(first.path.join("z.bin").exists());
        assert!(second.path.join("again.bin").exists());
        assert_eq!(store.list().len(), 2);
    }

    #[test]
    fn list_ignores_incomplete_backup_folder() {
        let fx = fixture();
        let store = backups(&fx);
        write(
            &fx.tmp
                .path()
                .join("backups/v1/20250101T000000Z/shaders/a.bin"),
            3,
        );
        assert!(store.list().is_empty());
        assert_eq!(store.total_size(), 0);
    }

    #[test]
    fn list_reports_an_incomplete_backup_which_can_be_deleted_but_not_restored() {
        let fx = fixture();
        let store = backups(&fx);
        let dir = fx.tmp.path().join("backups/v1/20250101T000000Z");
        write(&dir.join("shaders/a.bin"), 3);
        let meta = BackupMeta {
            kind: CacheKind::Shaders.label().to_string(),
            size: 99,
            created_unix: 1,
            original: fx.sc.join("v1/shaders").to_string_lossy().into_owned(),
            complete: false,
        };
        write_meta(&dir.join(BACKUP_META), &meta).unwrap();
        let listed = store.list();
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].complete);
        assert_eq!(listed[0].size, 3);
        assert!(matches!(
            store.restore(&listed[0], &fx.sc, &IDLE),
            Err(Error::Invalid(_))
        ));
        assert!(dir.join("shaders/a.bin").exists());
        store.delete(&listed[0]).unwrap();
        assert!(store.list().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn backup_and_clear_leaves_nothing_behind_when_the_move_fails() {
        use crate::safe_write::testing::ReadOnly;
        let fx = fixture();
        let store = backups(&fx);
        let cache = cache_at(&found(&fx), "sc-alpha-4.10.3_abc/shaders");
        let result = {
            let _guard = ReadOnly::new(cache.path.parent().unwrap());
            store.backup_and_clear(&cache, &IDLE)
        };
        assert!(result.is_err());
        assert!(cache.path.join("x.bin").exists());
        assert!(store.list().is_empty());
        let version_dir = fx.tmp.path().join("backups/sc-alpha-4.10.3_abc");
        assert_eq!(fs::read_dir(version_dir).unwrap().count(), 0);
    }

    #[test]
    fn total_size_sums_backups() {
        let fx = fixture();
        let store = backups(&fx);
        let caches = found(&fx);
        store
            .backup_and_clear(&cache_at(&caches, "sc-alpha-4.10.3_abc/shaders"), &IDLE)
            .unwrap();
        store
            .backup_and_clear(&cache_at(&caches, "sc-alpha-4.9.0_x/shaders"), &IDLE)
            .unwrap();
        assert_eq!(store.total_size(), 160);
    }

    #[test]
    fn restore_into_empty_place() {
        let fx = fixture();
        let store = backups(&fx);
        let cache = cache_at(&found(&fx), "sc-alpha-4.10.3_abc/shaders");
        let backup = store.backup_and_clear(&cache, &IDLE).unwrap();
        let restored = store.restore(&backup, &fx.sc, &IDLE).unwrap();
        assert_eq!(restored, cache.path);
        assert!(restored.join("sub/y.bin").exists());
        assert!(store.list().is_empty());
        assert!(!backup.path.exists());
    }

    #[test]
    fn restore_unversioned_goes_directly_under_root() {
        let fx = fixture();
        let store = backups(&fx);
        let cache = cache_at(&found(&fx), "vulkanshadercache");
        let backup = store.backup_and_clear(&cache, &IDLE).unwrap();
        let restored = store.restore(&backup, &fx.sc, &IDLE).unwrap();
        assert_eq!(restored, fx.sc.join("vulkanshadercache"));
        assert!(restored.join("v.bin").exists());
    }

    #[test]
    fn restore_replaces_existing_cache_and_removes_old() {
        let fx = fixture();
        let store = backups(&fx);
        let cache = cache_at(&found(&fx), "sc-alpha-4.10.3_abc/shaders");
        let backup = store.backup_and_clear(&cache, &IDLE).unwrap();
        write(&cache.path.join("fresh.bin"), 2);
        let restored = store.restore(&backup, &fx.sc, &IDLE).unwrap();
        assert!(restored.join("x.bin").exists());
        assert!(!restored.join("fresh.bin").exists());
        let siblings: Vec<_> = fs::read_dir(restored.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(siblings.iter().all(|n| !n.contains("star-config.old")));
    }

    #[test]
    fn restore_refuses_when_game_running() {
        let fx = fixture();
        let store = backups(&fx);
        let cache = cache_at(&found(&fx), "sc-alpha-4.10.3_abc/shaders");
        let backup = store.backup_and_clear(&cache, &IDLE).unwrap();
        assert!(matches!(
            store.restore(&backup, &fx.sc, &running()),
            Err(Error::GameRunning)
        ));
        assert!(backup.path.join("x.bin").exists());
        assert!(!cache.path.exists());
    }

    #[test]
    fn restore_refuses_a_tampered_backup_description() {
        let fx = fixture();
        let store = backups(&fx);
        let cache = cache_at(&found(&fx), "sc-alpha-4.10.3_abc/shaders");
        let mut backup = store.backup_and_clear(&cache, &IDLE).unwrap();
        backup.version = "../../../elsewhere".to_string();
        assert!(store.restore(&backup, &fx.sc, &IDLE).is_err());
        assert!(store.delete(&backup).is_err());
        backup.version = "sc-alpha-4.10.3_abc".to_string();
        backup.id = "..".to_string();
        assert!(store.delete(&backup).is_err());
        assert_eq!(store.list().len(), 1);
    }

    #[test]
    fn restore_refuses_a_root_that_is_not_star_citizen() {
        let fx = fixture();
        let store = backups(&fx);
        let cache = cache_at(&found(&fx), "sc-alpha-4.10.3_abc/shaders");
        let backup = store.backup_and_clear(&cache, &IDLE).unwrap();
        let other = fx.tmp.path().join("home");
        fs::create_dir_all(&other).unwrap();
        assert!(matches!(
            store.restore(&backup, &other, &IDLE),
            Err(Error::Invalid(_))
        ));
        assert!(backup.path.exists());
    }

    #[test]
    fn delete_removes_backup() {
        let fx = fixture();
        let store = backups(&fx);
        let cache = cache_at(&found(&fx), "sc-alpha-4.10.3_abc/shaders");
        let backup = store.backup_and_clear(&cache, &IDLE).unwrap();
        store.delete(&backup).unwrap();
        assert!(store.list().is_empty());
        assert!(!backup.path.exists());
        assert!(fx.tmp.path().join("backups").exists());
    }

    fn tree(dir: &Path) {
        write(&dir.join("a.bin"), 10);
        write(&dir.join("sub/b.bin"), 20);
    }

    #[test]
    fn copy_fallback_moves_the_tree() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        tree(&src);
        copy_verify_delete(&src, &dst, None).unwrap();
        assert!(!src.exists());
        assert_eq!(fs::metadata(dst.join("sub/b.bin")).unwrap().len(), 20);
    }

    #[test]
    fn copy_fallback_verifies_before_deleting_source() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        tree(&src);
        let tamper = |copied: &Path| {
            fs::write(copied.join("sub/b.bin"), b"").unwrap();
        };
        let result = copy_verify_delete(&src, &dst, Some(&tamper));
        assert!(matches!(result, Err(Error::Invalid(_))));
        assert_eq!(fs::metadata(src.join("a.bin")).unwrap().len(), 10);
        assert_eq!(fs::metadata(src.join("sub/b.bin")).unwrap().len(), 20);
        assert!(!dst.exists());
    }

    #[test]
    fn copy_fallback_refuses_existing_destination() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        tree(&src);
        write(&dst.join("keep.bin"), 1);
        assert!(copy_verify_delete(&src, &dst, None).is_err());
        assert!(src.join("a.bin").exists());
        assert!(dst.join("keep.bin").exists());
    }

    #[test]
    fn same_volume_is_true_within_one_temp_dir() {
        let tmp = tempdir().unwrap();
        let a = tmp.path().join("a");
        fs::create_dir_all(&a).unwrap();
        assert!(same_volume(&a, tmp.path()));
        assert!(!same_volume(&a, &tmp.path().join("missing")) || !cfg!(unix));
    }
}
