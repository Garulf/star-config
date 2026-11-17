//! Reads a channel's defaults out of its `Data.p4k` and caches the decoded files per game build.

use super::{GameData, Sources};
use crate::cryxml;
use crate::error::Error;
use crate::install::{Channel, resolve_ci};
use crate::localization;
use crate::p4k::Archive;
use crate::safe_write::write_atomic;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const PROFILE: &str = "Data/Libs/Config/defaultProfile.xml";
pub const ATTRIBUTES: &str = "Data/Libs/Config/Profiles/default/attributes.xml";
pub const KEY_NAMES: &str = "Data/Libs/Config/keybinding_localization.xml";

const ENGLISH: &str = "english";
const STAMP_FILE: &str = "stamp.json";
const PARTIAL_SUFFIX: &str = ".partial";
const KEEP_BUILDS: usize = 2;
const STALE_PARTIAL: Duration = Duration::from_secs(10 * 60);

pub fn strings_entry(language: &str) -> String {
    format!("Data/Localization/{language}/global.ini")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unavailable {
    NoArchive,
    NoBuildId,
    Unreadable(String),
    Missing(String),
    Encrypted(String),
}

impl Unavailable {
    pub fn message(&self) -> String {
        const TAIL: &str = "so only your own binds are shown.";
        match self {
            Self::NoArchive => format!("This channel has no Data.p4k, {TAIL}"),
            Self::NoBuildId => format!(
                "This channel has no build manifest, so the game's defaults cannot be matched to a build, {TAIL}"
            ),
            Self::Unreadable(reason) => {
                format!("The game's Data.p4k could not be read ({reason}), {TAIL}")
            }
            Self::Missing(entry) => format!("The game's Data.p4k has no {entry}, {TAIL}"),
            Self::Encrypted(entry) => {
                format!("{entry} in the game's Data.p4k is encrypted, {TAIL}")
            }
        }
    }
}

pub fn load(channel: &Channel, cache_root: &Path) -> Result<GameData, Unavailable> {
    let p4k = resolve_ci(&channel.dir, "Data.p4k").ok_or(Unavailable::NoArchive)?;
    let build = channel.build_id().ok_or(Unavailable::NoBuildId)?;
    let stamp = Stamp::of(&p4k, &build).map_err(|err| archive_error(&Error::io(&p4k, err)))?;
    let language = chosen_language(channel);
    let loose = loose_strings(channel, &language);
    let dir = build_dir(cache_root, channel, &build);

    let cached = read_cache(&dir, &stamp, &language, loose.as_deref());
    if let Some(data) =
        cached.and_then(|sources| GameData::from_sources(build.clone(), &sources).ok())
    {
        return Ok(data);
    }

    let archive = Archive::open_only(&p4k, is_read_entry).map_err(|err| archive_error(&err))?;
    let extracted = extract(&archive, &language)?;
    let used = Sources {
        strings: loose.unwrap_or_else(|| extracted.strings.clone()),
        ..extracted.clone()
    };
    let data = GameData::from_sources(build, &used).map_err(|err| unreadable(&err))?;
    store_and_prune(&dir, &stamp, &language, &extracted);
    Ok(data)
}

/// Writes `sources` as the cache `load` accepts for the channel's current `Data.p4k` and build,
/// so tests and tools can fill the cache without an archive. The file must exist; it is only
/// stat-ed, never opened.
pub fn seed_cache(channel: &Channel, cache_root: &Path, sources: &Sources) -> Result<(), Error> {
    let p4k = resolve_ci(&channel.dir, "Data.p4k")
        .ok_or_else(|| Error::NotFound(format!("{}/Data.p4k", channel.name)))?;
    let build = channel
        .build_id()
        .ok_or_else(|| Error::Invalid(format!("{} has no build manifest", channel.name)))?;
    let stamp = Stamp::of(&p4k, &build).map_err(|err| Error::io(&p4k, err))?;
    let dir = build_dir(cache_root, channel, &build);
    let partial = partial_dir(&dir);
    let stored = store(&partial, &dir, &stamp, &chosen_language(channel), sources);
    if stored.is_err() {
        let _ = fs::remove_dir_all(&partial);
    }
    stored
}

/// For tools that read an archive directly, without a channel or the cache.
pub fn from_archive(
    archive: &Archive,
    build: &str,
    language: &str,
) -> Result<GameData, Unavailable> {
    let sources = extract(archive, language)?;
    GameData::from_sources(build.to_string(), &sources).map_err(|err| unreadable(&err))
}

/// The entries `load` reads, as `Archive::open_only` names them.
fn is_read_entry(name: &str) -> bool {
    [PROFILE, ATTRIBUTES, KEY_NAMES]
        .iter()
        .any(|entry| entry.eq_ignore_ascii_case(name))
        || name
            .strip_prefix("data/localization/")
            .and_then(|rest| rest.strip_suffix("/global.ini"))
            .is_some_and(|language| !language.is_empty() && !language.contains('/'))
}

fn extract(archive: &Archive, language: &str) -> Result<Sources, Unavailable> {
    Ok(Sources {
        profile: read_xml(archive, PROFILE)?,
        attributes: read_xml(archive, ATTRIBUTES)?,
        key_names: read_xml(archive, KEY_NAMES)?,
        strings: read_strings(archive, language)?,
    })
}

fn read_xml(archive: &Archive, entry: &str) -> Result<String, Unavailable> {
    let bytes = archive
        .read(entry)
        .map_err(|err| entry_error(&err, entry))?;
    cryxml::decode_entry(bytes)
        .map_err(|_| Unavailable::Unreadable(format!("{entry} is not readable XML")))
}

/// A language the archive lacks falls back to English; no strings at all is still usable, as
/// labels then fall back to humanised names.
fn read_strings(archive: &Archive, language: &str) -> Result<String, Unavailable> {
    for entry in [strings_entry(language), strings_entry(ENGLISH)] {
        match archive.read(&entry) {
            Ok(bytes) => return Ok(String::from_utf8_lossy(&bytes).into_owned()),
            Err(Error::NotFound(_)) => {}
            Err(err) => return Err(entry_error(&err, &entry)),
        }
    }
    Ok(String::new())
}

fn archive_error(err: &Error) -> Unavailable {
    match err {
        Error::Io { source, .. } if source.kind() == ErrorKind::NotFound => Unavailable::NoArchive,
        _ => unreadable(err),
    }
}

fn entry_error(err: &Error, entry: &str) -> Unavailable {
    match err {
        Error::NotFound(_) => Unavailable::Missing(entry.to_string()),
        Error::Invalid(message) if message.contains("encrypted") => {
            Unavailable::Encrypted(entry.to_string())
        }
        _ => unreadable(err),
    }
}

fn unreadable(err: &Error) -> Unavailable {
    Unavailable::Unreadable(match err {
        Error::Io { source, .. } => source.to_string(),
        Error::Invalid(message) => message.clone(),
        Error::Parse {
            path,
            line,
            message,
        } if *line > 0 => {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            format!("{name}, line {line}: {message}")
        }
        Error::Parse { message, .. } => message.clone(),
        other => other.to_string(),
    })
}

fn chosen_language(channel: &Channel) -> String {
    let cfg = fs::read_to_string(channel.user_cfg()).ok();
    let language = localization::language(cfg.as_deref()).to_lowercase();
    let safe = !language.is_empty() && !language.contains(['/', '\\']) && !language.contains("..");
    if safe { language } else { ENGLISH.to_string() }
}

fn loose_strings(channel: &Channel, language: &str) -> Option<String> {
    let path = resolve_ci(&channel.dir, &strings_entry(language))?;
    let bytes = fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Stamp {
    p4k_size: u64,
    p4k_modified: u64,
    build: String,
}

impl Stamp {
    fn of(p4k: &Path, build: &str) -> std::io::Result<Stamp> {
        let meta = fs::metadata(p4k)?;
        let nanos = meta
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        Ok(Stamp {
            p4k_size: meta.len(),
            p4k_modified: u64::try_from(nanos).unwrap_or(u64::MAX),
            build: build.to_string(),
        })
    }
}

fn sanitised(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|ch| match ch {
            ch if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') => ch,
            _ => '_',
        })
        .collect();
    match clean.trim_end_matches('.') {
        "" => "_".to_string(),
        trimmed => trimmed.to_string(),
    }
}

fn channel_cache(cache_root: &Path, channel: &Channel) -> PathBuf {
    let name = channel.dir.file_name().map_or_else(
        || channel.name.clone(),
        |name| name.to_string_lossy().into(),
    );
    cache_root.join(sanitised(&name))
}

fn build_dir(cache_root: &Path, channel: &Channel, build: &str) -> PathBuf {
    channel_cache(cache_root, channel).join(sanitised(build))
}

/// Unique per store, so two loaders of one channel never write into the same folder.
fn partial_dir(dir: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut name = dir.file_name().unwrap_or_default().to_os_string();
    name.push(format!(
        "{PARTIAL_SUFFIX}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    dir.with_file_name(name)
}

fn is_partial(name: &str) -> bool {
    name.contains(PARTIAL_SUFFIX)
}

fn file_name(entry: &str) -> &str {
    entry.rsplit('/').next().unwrap_or(entry)
}

fn strings_file(language: &str) -> String {
    format!("global-{}.ini", sanitised(language))
}

fn read_cache(dir: &Path, stamp: &Stamp, language: &str, loose: Option<&str>) -> Option<Sources> {
    let saved: Stamp = serde_json::from_slice(&fs::read(dir.join(STAMP_FILE)).ok()?).ok()?;
    if saved != *stamp {
        return None;
    }
    let text = |name: &str| fs::read_to_string(dir.join(name)).ok();
    Some(Sources {
        profile: text(file_name(PROFILE))?,
        attributes: text(file_name(ATTRIBUTES))?,
        key_names: text(file_name(KEY_NAMES))?,
        strings: match loose {
            Some(text) => text.to_string(),
            None => text(&strings_file(language))?,
        },
    })
}

/// The data is already good by now, so a cache that cannot be written costs only speed.
fn store_and_prune(dir: &Path, stamp: &Stamp, language: &str, sources: &Sources) {
    let partial = partial_dir(dir);
    if store(&partial, dir, stamp, language, sources).is_err() {
        let _ = fs::remove_dir_all(&partial);
        return;
    }
    prune(dir);
}

fn store(
    partial: &Path,
    dir: &Path,
    stamp: &Stamp,
    language: &str,
    sources: &Sources,
) -> crate::error::Result<()> {
    let files = [
        (file_name(PROFILE).to_string(), &sources.profile),
        (file_name(ATTRIBUTES).to_string(), &sources.attributes),
        (file_name(KEY_NAMES).to_string(), &sources.key_names),
        (strings_file(language), &sources.strings),
    ];
    for (name, text) in files {
        write_atomic(&partial.join(name), text.as_bytes())?;
    }
    let stamp_json =
        serde_json::to_vec_pretty(stamp).map_err(|err| Error::Invalid(err.to_string()))?;
    write_atomic(&partial.join(STAMP_FILE), &stamp_json)?;
    remove_dir(dir)?;
    fs::rename(partial, dir).map_err(|err| Error::io(dir, err))
}

fn remove_dir(dir: &Path) -> crate::error::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(err) if err.kind() != ErrorKind::NotFound => Err(Error::io(dir, err)),
        _ => Ok(()),
    }
}

/// Keeps `current` and the newest other build; clears leftovers of crashed runs once they are
/// too old to belong to a load still running.
fn prune(current: &Path) {
    let Some(parent) = current.parent() else {
        return;
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    let mut older: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) || path == current {
            continue;
        }
        let modified = entry.metadata().and_then(|meta| meta.modified());
        if is_partial(&entry.file_name().to_string_lossy()) {
            let age = modified
                .ok()
                .and_then(|when| SystemTime::now().duration_since(when).ok());
            if age.is_some_and(|age| age > STALE_PARTIAL) {
                let _ = fs::remove_dir_all(&path);
            }
            continue;
        }
        older.push((modified.unwrap_or(UNIX_EPOCH), path));
    }
    older.sort();
    let surplus = older.len().saturating_sub(KEEP_BUILDS - 1);
    for (_, path) in older.into_iter().take(surplus) {
        let _ = fs::remove_dir_all(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p4k::testing::{Method, build};
    use tempfile::{TempDir, tempdir};

    fn fixture(name: &str) -> String {
        let path = format!(
            "{}/tests/fixtures/defaults/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        String::from_utf8(fs::read(path).unwrap()).unwrap()
    }

    type Entries = Vec<(String, Method, Vec<u8>)>;

    fn standard() -> Entries {
        vec![
            (
                PROFILE.into(),
                Method::Deflate,
                cryxml::encode(&fixture("defaultProfile.xml")),
            ),
            (
                ATTRIBUTES.into(),
                Method::Zstd,
                cryxml::encode(&fixture("attributes.xml")),
            ),
            (
                KEY_NAMES.into(),
                Method::Store,
                cryxml::encode(&fixture("keybinding_localization.xml")),
            ),
            (
                strings_entry(ENGLISH),
                Method::Deflate,
                fixture("global.ini").into_bytes(),
            ),
        ]
    }

    fn archive_of(entries: &Entries) -> Vec<u8> {
        let borrowed: Vec<(&str, Method, &[u8])> = entries
            .iter()
            .map(|(name, method, data)| (name.as_str(), *method, data.as_slice()))
            .collect();
        build(&borrowed)
    }

    fn set_mtime(path: &Path, when: SystemTime) {
        fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    #[cfg(unix)]
    fn set_dir_mtime(path: &Path, when: SystemTime) {
        fs::File::open(path).unwrap().set_modified(when).unwrap();
    }

    struct Install {
        _root: TempDir,
        channel: Channel,
        cache: PathBuf,
    }

    impl Install {
        fn bare() -> Install {
            let root = tempdir().unwrap();
            let dir = root.path().join("LIVE");
            fs::create_dir_all(&dir).unwrap();
            Install {
                cache: root.path().join("cache"),
                channel: Channel {
                    name: "LIVE".into(),
                    dir,
                },
                _root: root,
            }
        }

        fn new() -> Install {
            let install = Install::bare();
            install.set_build("100");
            install.write_archive(&standard());
            install
        }

        fn set_build(&self, id: &str) {
            let manifest = format!("{{\"Data\":{{\"RequestedP4ChangeNum\":\"{id}\"}}}}");
            fs::write(self.channel.dir.join("build_manifest.id"), manifest).unwrap();
        }

        fn p4k(&self) -> PathBuf {
            self.channel.dir.join("Data.p4k")
        }

        fn write_archive(&self, entries: &Entries) {
            fs::write(self.p4k(), archive_of(entries)).unwrap();
        }

        fn write_loose(&self, relative: &str, text: &str) {
            let path = self.channel.dir.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }

        fn load(&self) -> Result<GameData, Unavailable> {
            load(&self.channel, &self.cache)
        }

        fn dir_for(&self, build: &str) -> PathBuf {
            build_dir(&self.cache, &self.channel, build)
        }

        fn built(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(channel_cache(&self.cache, &self.channel))
                .unwrap()
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    fn pitch_up_label(data: &GameData) -> Option<&str> {
        data.strings.get("ui_CiPitchUp")
    }

    #[test]
    fn first_load_extracts_and_second_reads_the_cache() {
        let install = Install::new();
        let first = install.load().unwrap();
        assert_eq!(first.build, "100");
        assert_eq!(first.maps.len(), 3);
        assert_eq!(pitch_up_label(&first), Some("Pitch up (ship)"));

        let dir = install.dir_for("100");
        for name in [
            "defaultProfile.xml",
            "attributes.xml",
            "keybinding_localization.xml",
            "global-english.ini",
            "stamp.json",
        ] {
            assert!(dir.join(name).is_file(), "{name} is cached");
        }

        let p4k = install.p4k();
        let meta = fs::metadata(&p4k).unwrap();
        fs::write(&p4k, vec![0xAB; meta.len() as usize]).unwrap();
        set_mtime(&p4k, meta.modified().unwrap());
        assert!(Archive::open(&p4k).is_err(), "the archive is now garbage");
        assert_eq!(install.load().unwrap(), first);
    }

    #[test]
    fn a_changed_archive_is_not_served_from_the_cache() {
        let install = Install::new();
        install.load().unwrap();

        let mut patched = standard();
        patched[3].2 = b"ui_CiPitchUp=Patched label\r\n".to_vec();
        install.write_archive(&patched);
        let data = install.load().unwrap();
        assert_eq!(pitch_up_label(&data), Some("Patched label"));

        let p4k = install.p4k();
        let size = fs::metadata(&p4k).unwrap().len() as usize;
        fs::write(&p4k, vec![0xAB; size]).unwrap();
        set_mtime(&p4k, SystemTime::now() + std::time::Duration::from_secs(60));
        assert!(matches!(install.load(), Err(Unavailable::Unreadable(_))));
    }

    #[test]
    fn new_build_id_re_extracts_and_prunes_old_builds() {
        let install = Install::new();
        let start = SystemTime::now();
        for (step, build) in ["1", "2", "3"].into_iter().enumerate() {
            install.set_build(build);
            assert_eq!(install.load().unwrap().build, build);
            #[cfg(unix)]
            set_dir_mtime(
                &install.dir_for(build),
                start + std::time::Duration::from_secs(step as u64),
            );
            #[cfg(not(unix))]
            let _ = (start, step);
        }
        let built = install.built();
        assert_eq!(built.len(), 2, "{built:?}");
        assert!(built.contains(&"3".to_string()), "{built:?}");
        #[cfg(unix)]
        assert_eq!(built, ["2", "3"]);
    }

    #[test]
    fn half_written_cache_is_ignored() {
        let install = Install::new();
        let dir = install.dir_for("100");
        let partial = partial_dir(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("defaultProfile.xml"), "<profile>").unwrap();
        fs::create_dir_all(&partial).unwrap();
        fs::write(partial.join("leftover.xml"), "x").unwrap();

        let data = install.load().unwrap();
        assert_eq!(data.maps.len(), 3);
        assert!(dir.join(STAMP_FILE).is_file());
        assert!(!dir.join("leftover.xml").exists());
        assert!(
            partial.exists(),
            "a fresh partial may belong to a load still running"
        );
        assert_ne!(
            fs::read_to_string(dir.join("defaultProfile.xml")).unwrap(),
            "<profile>"
        );
    }

    #[cfg(unix)]
    #[test]
    fn stale_partials_are_cleared() {
        let install = Install::new();
        let partial = partial_dir(&install.dir_for("100"));
        fs::create_dir_all(&partial).unwrap();
        fs::File::open(&partial)
            .unwrap()
            .set_modified(SystemTime::now() - STALE_PARTIAL * 2)
            .unwrap();
        install.load().unwrap();
        assert!(!partial.exists());
    }

    #[test]
    fn concurrent_loads_leave_one_whole_cache() {
        let install = Install::new();
        std::thread::scope(|scope| {
            let loads: Vec<_> = (0..4).map(|_| scope.spawn(|| install.load())).collect();
            for load in loads {
                assert_eq!(load.join().unwrap().unwrap().maps.len(), 3);
            }
        });
        let dir = install.dir_for("100");
        assert!(dir.join(STAMP_FILE).is_file());
        assert_eq!(install.load().unwrap().settings.len(), 8);
        let leftovers: Vec<_> = fs::read_dir(dir.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|entry| is_partial(&entry.file_name().to_string_lossy()))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn a_stamped_cache_missing_a_file_is_rebuilt() {
        let install = Install::new();
        install.load().unwrap();
        let dir = install.dir_for("100");
        fs::remove_file(dir.join("attributes.xml")).unwrap();
        assert_eq!(install.load().unwrap().settings.len(), 8);
        assert!(dir.join("attributes.xml").is_file());
    }

    #[test]
    fn loose_global_ini_wins_over_the_archive() {
        let install = Install::new();
        let from_archive = install.load().unwrap();
        assert_eq!(pitch_up_label(&from_archive), Some("Pitch up (ship)"));

        install.write_loose(
            "data/Localization/english/global.ini",
            "ui_CiPitchUp=Loose label\r\n",
        );
        let loose = install.load().unwrap();
        assert_eq!(pitch_up_label(&loose), Some("Loose label"));

        fs::remove_file(
            install
                .channel
                .dir
                .join("data/Localization/english/global.ini"),
        )
        .unwrap();
        assert_eq!(
            pitch_up_label(&install.load().unwrap()),
            Some("Pitch up (ship)")
        );
    }

    #[test]
    fn the_language_in_user_cfg_picks_the_strings() {
        let install = Install::new();
        let mut entries = standard();
        entries.push((
            strings_entry("german_(germany)"),
            Method::Store,
            b"ui_CiPitchUp=Nick hoch\r\n".to_vec(),
        ));
        install.write_archive(&entries);

        install.write_loose("user.cfg", "g_language = German_(Germany)\n");
        assert_eq!(pitch_up_label(&install.load().unwrap()), Some("Nick hoch"));
        let cached = install.dir_for("100").join("global-german__germany_.ini");
        assert!(cached.is_file());

        install.write_loose("user.cfg", "g_language = french_(france)\n");
        assert_eq!(
            pitch_up_label(&install.load().unwrap()),
            Some("Pitch up (ship)"),
            "a language the archive lacks falls back to English"
        );

        install.write_loose("user.cfg", "g_language = ../../elsewhere\n");
        assert_eq!(
            pitch_up_label(&install.load().unwrap()),
            Some("Pitch up (ship)")
        );
    }

    #[test]
    fn an_archive_without_strings_still_loads() {
        let install = Install::new();
        let mut entries = standard();
        entries.pop();
        install.write_archive(&entries);
        let data = install.load().unwrap();
        assert!(data.strings.is_empty());
        assert_eq!(data.maps.len(), 3);
    }

    #[test]
    fn a_seeded_cache_loads_without_opening_the_archive() {
        let install = Install::bare();
        install.set_build("100");
        fs::write(install.p4k(), b"placeholder, not an archive").unwrap();
        let sources = Sources {
            profile: fixture("defaultProfile.xml"),
            attributes: fixture("attributes.xml"),
            key_names: fixture("keybinding_localization.xml"),
            strings: fixture("global.ini"),
        };
        seed_cache(&install.channel, &install.cache, &sources).unwrap();

        let data = install.load().unwrap();
        assert_eq!(data.build, "100");
        assert_eq!(data.maps.len(), 3);
        assert_eq!(pitch_up_label(&data), Some("Pitch up (ship)"));
    }

    #[test]
    fn seeding_needs_an_archive_file_and_a_build() {
        let install = Install::bare();
        let sources = Sources::default();
        assert!(seed_cache(&install.channel, &install.cache, &sources).is_err());
        fs::write(install.p4k(), b"x").unwrap();
        assert!(seed_cache(&install.channel, &install.cache, &sources).is_err());
    }

    #[test]
    fn nothing_is_written_inside_the_channel() {
        let install = Install::new();
        let before = fs::read_dir(&install.channel.dir).unwrap().count();
        install.load().unwrap();
        install.load().unwrap();
        assert_eq!(fs::read_dir(&install.channel.dir).unwrap().count(), before);
    }

    #[test]
    fn unavailable_reasons() {
        let no_archive = Install::bare();
        no_archive.set_build("100");
        let no_manifest = Install::bare();
        no_manifest.write_archive(&standard());
        let no_profile = Install::new();
        no_profile.write_archive(&standard()[1..].to_vec());
        let encrypted = Install::new();
        let mut entries = standard();
        entries[0].1 = Method::ZstdGarbage;
        encrypted.write_archive(&entries);
        let not_an_archive = Install::new();
        fs::write(not_an_archive.p4k(), b"this is not a Data.p4k at all").unwrap();
        let bad_xml = Install::new();
        let mut entries = standard();
        entries[1].2 = b"<Attributes><Attr".to_vec();
        bad_xml.write_archive(&entries);

        let cases = [
            (no_archive, Unavailable::NoArchive),
            (no_manifest, Unavailable::NoBuildId),
            (no_profile, Unavailable::Missing(PROFILE.to_string())),
            (encrypted, Unavailable::Encrypted(PROFILE.to_string())),
        ];
        for (install, expected) in cases {
            let got = install.load().unwrap_err();
            assert_eq!(got, expected);
            assert_plain(&got);
        }
        for install in [not_an_archive, bad_xml] {
            let got = install.load().unwrap_err();
            assert!(matches!(got, Unavailable::Unreadable(_)), "{got:?}");
            assert_plain(&got);
        }
    }

    fn assert_plain(reason: &Unavailable) {
        let message = reason.message();
        assert!(!message.is_empty());
        for debug in ["Unreadable", "Missing", "Encrypted", "NoArchive", "\"", "{"] {
            assert!(!message.contains(debug), "{message}");
        }
        assert!(
            message.ends_with("only your own binds are shown."),
            "{message}"
        );
    }

    #[test]
    fn no_archive_message_reads_as_plain_words() {
        assert_eq!(
            Unavailable::NoArchive.message(),
            "This channel has no Data.p4k, so only your own binds are shown."
        );
    }

    #[test]
    fn the_loader_keeps_only_the_entries_it_reads() {
        for name in [
            "data/libs/config/defaultprofile.xml",
            "data/libs/config/profiles/default/attributes.xml",
            "data/libs/config/keybinding_localization.xml",
            "data/localization/english/global.ini",
            "data/localization/german_(germany)/global.ini",
        ] {
            assert!(is_read_entry(name), "{name}");
        }
        for name in [
            "data/objects/ship.cgf",
            "data/libs/config/defaultprofile.xml.bak",
            "data/localization/global.ini",
            "data/localization/english/sub/global.ini",
            "data/localization/english/other.ini",
        ] {
            assert!(!is_read_entry(name), "{name}");
        }
    }

    #[test]
    fn an_archive_with_other_entries_still_loads() {
        let install = Install::new();
        let mut entries = standard();
        entries.insert(
            0,
            (
                "Data\\Objects\\ship.cgf".into(),
                Method::Store,
                b"mesh".to_vec(),
            ),
        );
        install.write_archive(&entries);
        assert_eq!(install.load().unwrap().maps.len(), 3);
    }

    #[test]
    fn cache_names_are_safe_path_components() {
        assert_eq!(
            sanitised("4.10.200.58746 (13165946)"),
            "4.10.200.58746__13165946_"
        );
        assert_eq!(sanitised(".."), "_");
        assert_eq!(sanitised("a/b\\c"), "a_b_c");
        assert_eq!(sanitised(""), "_");
    }

    #[test]
    #[ignore = "needs a real Data.p4k: SC_P4K=<path> cargo test -p sc-core defaults -- --ignored"]
    fn real_archive_counts() {
        let path = std::env::var("SC_P4K").expect("SC_P4K names a Data.p4k");
        let archive = Archive::open(Path::new(&path)).unwrap();
        let data = from_archive(&archive, "real", ENGLISH).unwrap();
        let counts = data.counts();
        assert_eq!(counts.maps, 50);
        assert_eq!(counts.actions, 1107);
        assert_eq!(counts.settings, 345);
        assert_eq!(counts.option_trees, 3);
        assert_eq!(counts.groups, 131);
        assert!(counts.label_keys > 0);
        assert!(
            counts.labels_found * 100 >= counts.label_keys * 98,
            "{} of {} action labels resolve",
            counts.labels_found,
            counts.label_keys
        );
    }
}
