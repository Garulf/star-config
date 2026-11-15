use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    Native,
    WinePrefix { prefix: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    pub root: PathBuf,
    pub kind: InstallKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    pub name: String,
    pub dir: PathBuf,
}

pub struct DiscoveryEnv {
    pub home: Option<PathBuf>,
    pub drive_roots: Vec<PathBuf>,
    pub extra: Vec<PathBuf>,
}

impl DiscoveryEnv {
    pub fn from_system(extra: Vec<PathBuf>) -> Self {
        let drive_roots = if cfg!(windows) {
            ('C'..='Z')
                .map(|letter| PathBuf::from(format!("{letter}:\\")))
                .filter(|root| root.exists())
                .collect()
        } else {
            Vec::new()
        };
        Self {
            home: home_dir(),
            drive_roots,
            extra,
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
}

fn names_match(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

fn entry_names(dir: &Path) -> Vec<OsString> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<OsString> = entries.flatten().map(|entry| entry.file_name()).collect();
    names.sort_by(|a, b| a.as_encoded_bytes().cmp(b.as_encoded_bytes()));
    names
}

fn matching_entries(dir: &Path, name: &str) -> Vec<OsString> {
    entry_names(dir)
        .into_iter()
        .filter(|entry| names_match(&entry.to_string_lossy(), name))
        .collect()
}

fn pick_entry(matches: &[OsString], name: &str) -> Option<OsString> {
    matches
        .iter()
        .find(|entry| entry.to_str() == Some(name))
        .or_else(|| matches.first())
        .cloned()
}

fn components(rel: &str) -> impl Iterator<Item = &str> {
    rel.split('/').filter(|part| !part.is_empty())
}

pub fn resolve_ci(base: &Path, rel: &str) -> Option<PathBuf> {
    let mut current = base.to_path_buf();
    for part in components(rel) {
        let found = pick_entry(&matching_entries(&current, part), part)?;
        current.push(found);
    }
    Some(current)
}

pub fn resolve_ci_or_default(base: &Path, rel: &str) -> PathBuf {
    let mut current = base.to_path_buf();
    let mut missing = false;
    for part in components(rel) {
        if !missing {
            match pick_entry(&matching_entries(&current, part), part) {
                Some(found) => {
                    current.push(found);
                    continue;
                }
                None => missing = true,
            }
        }
        current.push(part);
    }
    current
}

/// The first two entries, if any component of `rel` has more than one case-insensitive match.
pub fn case_conflict(base: &Path, rel: &str) -> Option<(PathBuf, PathBuf)> {
    let mut current = base.to_path_buf();
    for part in components(rel) {
        let matches = matching_entries(&current, part);
        if let [first, second, ..] = matches.as_slice() {
            return Some((current.join(first), current.join(second)));
        }
        current.push(pick_entry(&matches, part)?);
    }
    None
}

const STAR_CITIZEN_DIR: &str = "StarCitizen";
const WINE_RSI_PATH: &str = "drive_c/Program Files/Roberts Space Industries/StarCitizen";
const WINDOWS_RSI_PATH: &str = "Program Files/Roberts Space Industries/StarCitizen";
const KNOWN_CHANNELS: [&str; 5] = ["LIVE", "PTU", "EPTU", "HOTFIX", "TECH-PREVIEW"];

fn file_name_matches(path: &Path, name: &str) -> bool {
    path.file_name()
        .is_some_and(|file| names_match(&file.to_string_lossy(), name))
}

fn star_citizen_root(path: &Path) -> Option<PathBuf> {
    if !path.is_dir() {
        return None;
    }
    if file_name_matches(path, STAR_CITIZEN_DIR) {
        return Some(path.to_path_buf());
    }
    if let Some(parent) = path.parent()
        && file_name_matches(parent, STAR_CITIZEN_DIR)
    {
        return Some(parent.to_path_buf());
    }
    [STAR_CITIZEN_DIR, WINE_RSI_PATH]
        .into_iter()
        .filter_map(|rel| resolve_ci(path, rel))
        .find(|candidate| candidate.is_dir())
}

fn wine_prefix_of(root: &Path) -> Option<PathBuf> {
    root.ancestors()
        .find(|ancestor| file_name_matches(ancestor, "drive_c"))
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

fn channel_rank(name: &str) -> usize {
    KNOWN_CHANNELS
        .iter()
        .position(|known| names_match(known, name))
        .unwrap_or(KNOWN_CHANNELS.len())
}

fn is_channel_dir(dir: &Path) -> bool {
    !matching_entries(dir, "Data.p4k").is_empty() || !matching_entries(dir, "USER").is_empty()
}

pub fn discover(env: &DiscoveryEnv) -> Vec<Install> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(home) = &env.home {
        candidates.push(home.join("Games/star-citizen"));
    }
    candidates.extend(
        env.drive_roots
            .iter()
            .map(|drive| resolve_ci_or_default(drive, WINDOWS_RSI_PATH)),
    );
    candidates.extend(env.extra.iter().cloned());

    let mut seen: Vec<PathBuf> = Vec::new();
    let mut installs = Vec::new();
    for install in candidates.iter().filter_map(|path| Install::from_dir(path)) {
        let canonical = fs::canonicalize(&install.root).unwrap_or_else(|_| install.root.clone());
        if !seen.contains(&canonical) {
            seen.push(canonical);
            installs.push(install);
        }
    }
    installs
}

impl Install {
    pub fn from_dir(path: &Path) -> Option<Install> {
        let root = star_citizen_root(path)?;
        let kind = match wine_prefix_of(&root) {
            Some(prefix) => InstallKind::WinePrefix { prefix },
            None => InstallKind::Native,
        };
        Some(Install { root, kind })
    }

    pub fn channels(&self) -> Vec<Channel> {
        let mut channels: Vec<Channel> = entry_names(&self.root)
            .into_iter()
            .map(|name| (self.root.join(&name), name.to_string_lossy().into_owned()))
            .filter(|(dir, _)| dir.is_dir() && is_channel_dir(dir))
            .map(|(dir, name)| Channel { name, dir })
            .collect();
        channels.sort_by_key(|channel| {
            let rank = channel_rank(&channel.name);
            let tiebreak = if rank < KNOWN_CHANNELS.len() {
                String::new()
            } else {
                channel.name.clone()
            };
            (rank, tiebreak)
        });
        channels
    }

    pub fn key(&self) -> String {
        let digest = Sha256::digest(self.root.to_string_lossy().as_bytes());
        digest
            .iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    pub fn rsi_dir(&self) -> PathBuf {
        self.root
            .parent()
            .map_or_else(|| self.root.clone(), Path::to_path_buf)
    }

    pub fn label(&self) -> String {
        let shown = match &self.kind {
            InstallKind::WinePrefix { prefix } => prefix,
            InstallKind::Native => &self.root,
        };
        tilde(shown, home_dir().as_deref())
    }
}

fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

impl Channel {
    fn path(&self, rel: &str) -> PathBuf {
        resolve_ci_or_default(&self.dir, rel)
    }

    pub fn user_cfg(&self) -> PathBuf {
        self.path("user.cfg")
    }

    pub fn user_dir(&self) -> PathBuf {
        self.path("USER")
    }

    pub fn profile_dir(&self) -> PathBuf {
        self.path("USER/Client/0/Profiles/default")
    }

    pub fn attributes_xml(&self) -> PathBuf {
        self.path("USER/Client/0/Profiles/default/attributes.xml")
    }

    pub fn actionmaps_xml(&self) -> PathBuf {
        self.path("USER/Client/0/Profiles/default/actionmaps.xml")
    }

    pub fn mappings_dir(&self) -> PathBuf {
        self.path("USER/Client/0/Controls/Mappings")
    }

    pub fn global_ini(&self) -> PathBuf {
        self.path("data/Localization/english/global.ini")
    }

    pub fn screenshots_dir(&self) -> PathBuf {
        self.path("ScreenShots")
    }

    pub fn build_id(&self) -> Option<String> {
        let text = fs::read_to_string(resolve_ci(&self.dir, "build_manifest.id")?).ok()?;
        let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
        let text_at = |pointer: &str| match manifest.pointer(pointer)? {
            serde_json::Value::String(text) => Some(text.clone()),
            _ => None,
        };
        let id = [
            "/Data/RequestedP4ChangeNum",
            "/Data/BuildId",
            "/RequestedP4ChangeNum",
        ]
        .into_iter()
        .find_map(text_at)?;
        Some(match text_at("/Data/Version") {
            Some(version) => format!("{version} ({id})"),
            None => id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"").unwrap();
    }

    fn mkdir(path: &Path) {
        fs::create_dir_all(path).unwrap();
    }

    struct Wine {
        prefix: PathBuf,
        rsi: PathBuf,
        star_citizen: PathBuf,
    }

    fn wine_layout(base: &Path) -> Wine {
        let prefix = base.join("Zoë pfx");
        let rsi = prefix.join("drive_c/Program Files/Roberts Space Industries");
        let star_citizen = rsi.join("StarCitizen");
        touch(&star_citizen.join("LIVE/Data.p4k"));
        Wine {
            prefix,
            rsi,
            star_citizen,
        }
    }

    #[test]
    fn resolve_ci_finds_differently_cased_path() {
        let dir = tempdir().unwrap();
        mkdir(&dir.path().join("user/Client/0"));
        assert_eq!(
            resolve_ci(dir.path(), "USER/Client/0"),
            Some(dir.path().join("user/Client/0"))
        );
    }

    #[test]
    fn resolve_ci_returns_none_when_a_component_is_missing() {
        let dir = tempdir().unwrap();
        mkdir(&dir.path().join("user"));
        assert_eq!(resolve_ci(dir.path(), "USER/Client"), None);
    }

    #[cfg(unix)]
    #[test]
    fn resolve_ci_prefers_exact_case_when_both_exist() {
        let dir = tempdir().unwrap();
        touch(&dir.path().join("user.cfg"));
        touch(&dir.path().join("USER.cfg"));
        assert!(
            resolve_ci(dir.path(), "USER.cfg")
                .unwrap()
                .ends_with("USER.cfg")
        );
        assert!(
            resolve_ci(dir.path(), "user.cfg")
                .unwrap()
                .ends_with("user.cfg")
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_ci_picks_first_in_byte_order_without_exact_match() {
        let dir = tempdir().unwrap();
        touch(&dir.path().join("User.cfg"));
        touch(&dir.path().join("USER.cfg"));
        assert!(
            resolve_ci(dir.path(), "user.cfg")
                .unwrap()
                .ends_with("USER.cfg")
        );
    }

    #[cfg(unix)]
    #[test]
    fn case_conflict_reports_both_paths() {
        let dir = tempdir().unwrap();
        mkdir(&dir.path().join("USER"));
        mkdir(&dir.path().join("user"));
        assert_eq!(
            case_conflict(dir.path(), "USER/Client/0"),
            Some((dir.path().join("USER"), dir.path().join("user")))
        );

        let single = tempdir().unwrap();
        mkdir(&single.path().join("user"));
        assert_eq!(case_conflict(single.path(), "USER/Client/0"), None);
    }

    #[test]
    fn resolve_ci_or_default_keeps_found_casing_and_appends_rest() {
        let dir = tempdir().unwrap();
        mkdir(&dir.path().join("user"));
        assert_eq!(
            resolve_ci_or_default(dir.path(), "USER/Client/0/Profiles/default/attributes.xml"),
            dir.path()
                .join("user/Client/0/Profiles/default/attributes.xml")
        );
    }

    #[test]
    fn from_dir_accepts_prefix_rsi_starcitizen_and_channel_dirs() {
        let base = tempdir().unwrap();
        let wine = wine_layout(base.path());
        let expected = Install {
            root: wine.star_citizen.clone(),
            kind: InstallKind::WinePrefix {
                prefix: wine.prefix.clone(),
            },
        };
        for input in [
            wine.prefix.clone(),
            wine.rsi.clone(),
            wine.star_citizen.clone(),
            wine.star_citizen.join("LIVE"),
        ] {
            assert_eq!(Install::from_dir(&input).as_ref(), Some(&expected));
        }
    }

    #[test]
    fn from_dir_native_when_no_drive_c() {
        let base = tempdir().unwrap();
        let star_citizen = base
            .path()
            .join("Games/Roberts Space Industries/StarCitizen");
        touch(&star_citizen.join("LIVE/Data.p4k"));
        let install = Install::from_dir(&star_citizen).unwrap();
        assert_eq!(install.root, star_citizen);
        assert_eq!(install.kind, InstallKind::Native);
    }

    #[test]
    fn from_dir_rejects_unrelated_folder() {
        let base = tempdir().unwrap();
        mkdir(&base.path().join("Documents"));
        assert_eq!(Install::from_dir(&base.path().join("Documents")), None);
        assert_eq!(Install::from_dir(&base.path().join("missing")), None);
    }

    #[test]
    fn channels_require_data_p4k_or_user_and_sort_live_first() {
        let base = tempdir().unwrap();
        let root = base.path().join("StarCitizen");
        touch(&root.join("PTU/Data.p4k"));
        mkdir(&root.join("LIVE/user"));
        touch(&root.join("HOTFIX/Data.p4k"));
        mkdir(&root.join("junk"));
        let install = Install::from_dir(&root).unwrap();
        let names: Vec<String> = install.channels().into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["LIVE", "PTU", "HOTFIX"]);
    }

    #[test]
    fn channels_sort_unknown_names_after_known_ones() {
        let base = tempdir().unwrap();
        let root = base.path().join("StarCitizen");
        for name in ["ZZZ", "AAA", "EPTU", "TECH-PREVIEW", "LIVE"] {
            touch(&root.join(name).join("Data.p4k"));
        }
        let install = Install::from_dir(&root).unwrap();
        let names: Vec<String> = install.channels().into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["LIVE", "EPTU", "TECH-PREVIEW", "AAA", "ZZZ"]);
    }

    #[test]
    fn discover_finds_lug_default_and_dedupes_extra() {
        let home = tempdir().unwrap();
        let prefix = home.path().join("Games/star-citizen");
        let star_citizen =
            prefix.join("drive_c/Program Files/Roberts Space Industries/StarCitizen");
        touch(&star_citizen.join("LIVE/Data.p4k"));
        let env = DiscoveryEnv {
            home: Some(home.path().to_path_buf()),
            drive_roots: Vec::new(),
            extra: vec![prefix.clone(), star_citizen.clone()],
        };
        let found = discover(&env);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].root, star_citizen);
        assert_eq!(found[0].kind, InstallKind::WinePrefix { prefix });
    }

    #[test]
    fn discover_checks_drive_roots_and_keeps_order() {
        let drive = tempdir().unwrap();
        let native = drive
            .path()
            .join("Program Files/Roberts Space Industries/StarCitizen");
        touch(&native.join("LIVE/Data.p4k"));
        let extra = tempdir().unwrap();
        let wine = wine_layout(extra.path());
        let env = DiscoveryEnv {
            home: None,
            drive_roots: vec![drive.path().to_path_buf()],
            extra: vec![wine.prefix.clone()],
        };
        let roots: Vec<PathBuf> = discover(&env).into_iter().map(|i| i.root).collect();
        assert_eq!(roots, [native, wine.star_citizen]);
    }

    #[test]
    fn channel_paths_reuse_on_disk_casing() {
        let base = tempdir().unwrap();
        let dir = base.path().join("LIVE");
        mkdir(&dir.join("user/client/0/profiles/default"));
        let channel = Channel {
            name: "LIVE".into(),
            dir: dir.clone(),
        };
        assert_eq!(
            channel.attributes_xml(),
            dir.join("user/client/0/profiles/default/attributes.xml")
        );
        assert_eq!(
            channel.actionmaps_xml(),
            dir.join("user/client/0/profiles/default/actionmaps.xml")
        );
        assert_eq!(channel.user_dir(), dir.join("user"));
        assert_eq!(channel.user_cfg(), dir.join("user.cfg"));
        assert_eq!(
            channel.global_ini(),
            dir.join("data/Localization/english/global.ini")
        );
    }

    #[test]
    fn build_id_none_when_missing_or_garbage() {
        let base = tempdir().unwrap();
        let channel = Channel {
            name: "LIVE".into(),
            dir: base.path().to_path_buf(),
        };
        assert_eq!(channel.build_id(), None);
        fs::write(base.path().join("build_manifest.id"), "not json").unwrap();
        assert_eq!(channel.build_id(), None);
        fs::write(base.path().join("build_manifest.id"), r#"{"Data":{}}"#).unwrap();
        assert_eq!(channel.build_id(), None);
    }

    #[test]
    fn build_id_reads_manifest() {
        let base = tempdir().unwrap();
        fs::write(
            base.path().join("Build_Manifest.id"),
            r#"{"Data":{"Version":"4.10.3","RequestedP4ChangeNum":"13165946"}}"#,
        )
        .unwrap();
        let channel = Channel {
            name: "LIVE".into(),
            dir: base.path().to_path_buf(),
        };
        assert_eq!(channel.build_id().as_deref(), Some("4.10.3 (13165946)"));
    }

    #[test]
    fn build_id_without_version_is_the_bare_id() {
        let base = tempdir().unwrap();
        fs::write(
            base.path().join("build_manifest.id"),
            r#"{"RequestedP4ChangeNum":"42"}"#,
        )
        .unwrap();
        let channel = Channel {
            name: "LIVE".into(),
            dir: base.path().to_path_buf(),
        };
        assert_eq!(channel.build_id().as_deref(), Some("42"));
    }

    #[test]
    fn install_key_is_stable_and_16_hex() {
        let a = Install {
            root: PathBuf::from("/games/StarCitizen"),
            kind: InstallKind::Native,
        };
        let key = a.key();
        assert_eq!(key.len(), 16);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(key, a.clone().key());
        let b = Install {
            root: PathBuf::from("/other/StarCitizen"),
            kind: InstallKind::Native,
        };
        assert_ne!(key, b.key());
    }

    #[test]
    fn rsi_dir_is_parent_of_root() {
        let install = Install {
            root: PathBuf::from("/x/Roberts Space Industries/StarCitizen"),
            kind: InstallKind::Native,
        };
        assert_eq!(
            install.rsi_dir(),
            PathBuf::from("/x/Roberts Space Industries")
        );
    }

    #[test]
    fn label_shows_prefix_for_wine_and_tilde_for_home() {
        let home = Path::new("/home/zoe");
        assert_eq!(
            tilde(Path::new("/home/zoe/Games/star-citizen"), Some(home)),
            format!("~{0}Games{0}star-citizen", std::path::MAIN_SEPARATOR)
        );
        assert_eq!(tilde(home, Some(home)), "~");
        assert_eq!(
            tilde(Path::new("/opt/sc"), Some(home)),
            Path::new("/opt/sc").display().to_string()
        );
        let wine = Install {
            root: PathBuf::from("/opt/pfx/drive_c/StarCitizen"),
            kind: InstallKind::WinePrefix {
                prefix: PathBuf::from("/opt/pfx"),
            },
        };
        assert_eq!(wine.label(), Path::new("/opt/pfx").display().to_string());
    }
}
