//! StarStrings localization support.

use crate::backup::{Scope, SnapshotInfo};
use crate::dirs::AppDirs;
use crate::error::{Error, Result};
use crate::guard::{Change, Writer};
use crate::install::Channel;
use crate::safe_write::{hash, load, write_atomic};
use crate::staging::Document;
use crate::usercfg::{Op, UserCfg};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const RELEASE_API: &str = "https://api.github.com/repos/MrKraken/StarStrings/releases/latest";
pub const ASSET_NAME: &str = "StarStrings-LIVE.zip";
pub const ENTRY_PATH: &str = "Data/Localization/english/global.ini";
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const MIN_LINES: usize = 50_000;

const TEXT_LIMIT: u64 = 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const LANGUAGE_KEY: &str = "g_language";
const LANGUAGE_VALUE: &str = "english";

pub trait Http: Send + Sync {
    fn get_text(&self, url: &str) -> Result<Option<String>>;
    fn get_bytes(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>>;
}

pub struct UreqHttp {
    agent: ureq::Agent,
}

impl UreqHttp {
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_global(Some(Duration::from_secs(300)))
            .user_agent("star-config")
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }

    fn fetch(&self, url: &str, limit: u64) -> Result<Option<Vec<u8>>> {
        let mut url = url.to_string();
        for _ in 0..=MAX_REDIRECTS {
            if !allowed_url(&url) {
                return Err(Error::Network(format!("refusing to contact {url}")));
            }
            let response = self
                .agent
                .get(&url)
                .call()
                .map_err(|err| Error::Network(err.to_string()))?;
            let status = response.status().as_u16();
            if (300..400).contains(&status) {
                url = redirect_target(&response)?;
                continue;
            }
            if status == 404 {
                return Ok(None);
            }
            if status != 200 {
                return Err(Error::Network(format!("{url} answered {status}")));
            }
            return read_capped(response.into_body(), limit).map(Some);
        }
        Err(Error::Network("too many redirects".into()))
    }
}

impl Default for UreqHttp {
    fn default() -> Self {
        Self::new()
    }
}

fn redirect_target(response: &ureq::http::Response<ureq::Body>) -> Result<String> {
    response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| Error::Network("redirect without a location".into()))
}

fn read_capped(body: ureq::Body, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    body.into_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| Error::Network(err.to_string()))?;
    if bytes.len() as u64 > limit {
        return Err(Error::Network(format!(
            "download is larger than {limit} bytes"
        )));
    }
    Ok(bytes)
}

impl Http for UreqHttp {
    fn get_text(&self, url: &str) -> Result<Option<String>> {
        match self.fetch(url, TEXT_LIMIT)? {
            Some(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| Error::Network("response is not UTF-8".into())),
            None => Ok(None),
        }
    }

    fn get_bytes(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>> {
        self.fetch(url, max_bytes)?
            .ok_or_else(|| Error::Network(format!("{url} not found")))
    }
}

pub fn allowed_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let plain = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    if !plain {
        return false;
    }
    let host = host.to_ascii_lowercase();
    host == "github.com"
        || host == "api.github.com"
        || host
            .strip_suffix(".githubusercontent.com")
            .is_some_and(|prefix| !prefix.is_empty())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub name: String,
    pub published_at: String,
    pub asset_url: String,
    pub asset_size: u64,
    pub sha256: Option<String>,
}

fn text_field(value: &serde_json::Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::Invalid(format!("release has no {key}")))
}

fn digest_hex(asset: &serde_json::Value) -> Option<String> {
    let digest = asset.get("digest")?.as_str()?;
    let (algorithm, hex) = digest.split_once(':')?;
    algorithm
        .eq_ignore_ascii_case("sha256")
        .then(|| hex.to_ascii_lowercase())
}

pub fn parse_release(json: &str) -> Result<Option<Release>> {
    let root: serde_json::Value =
        serde_json::from_str(json).map_err(|err| Error::Invalid(format!("release JSON: {err}")))?;
    let name = text_field(&root, "name")?;
    let published_at = text_field(&root, "published_at")?;
    let assets = root
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let Some(asset) = assets
        .iter()
        .find(|asset| asset.get("name").and_then(serde_json::Value::as_str) == Some(ASSET_NAME))
    else {
        return Ok(None);
    };
    Ok(Some(Release {
        name,
        published_at,
        asset_url: text_field(asset, "browser_download_url")?,
        asset_size: asset
            .get("size")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| Error::Invalid("asset has no size".into()))?,
        sha256: digest_hex(asset),
    }))
}

pub fn latest(http: &dyn Http) -> Result<Option<Release>> {
    match http.get_text(RELEASE_API)? {
        Some(json) => parse_release(&json),
        None => Ok(None),
    }
}

fn normalized(name: &str) -> String {
    name.replace('\\', "/").to_ascii_lowercase()
}

pub fn extract_global_ini(zip_bytes: &[u8]) -> Result<Vec<u8>> {
    let invalid = |err: &dyn std::fmt::Display| Error::Invalid(format!("not a valid zip: {err}"));
    let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes)).map_err(|e| invalid(&e))?;
    let wanted = normalized(ENTRY_PATH);
    let mut matches = Vec::new();
    for index in 0..archive.len() {
        let name = archive
            .name_for_index(index)
            .and_then(std::result::Result::ok)
            .map(|name| normalized(&name));
        if name.as_deref() == Some(wanted.as_str()) {
            matches.push(index);
        }
    }
    let [index] = matches[..] else {
        return Err(Error::Invalid(format!(
            "expected one {ENTRY_PATH} in the download, found {}",
            matches.len()
        )));
    };
    let entry = archive.by_index(index).map_err(|e| invalid(&e))?;
    let declared = entry.size();
    if declared > MAX_BYTES {
        return Err(Error::Invalid(format!(
            "global.ini declares {declared} bytes, more than {MAX_BYTES}"
        )));
    }
    let mut bytes = Vec::new();
    entry
        .take(declared + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| invalid(&e))?;
    if bytes.len() as u64 > declared {
        return Err(Error::Invalid(
            "global.ini is larger than its declared size".into(),
        ));
    }
    Ok(bytes)
}

pub fn count_lines(ini: &[u8]) -> usize {
    ini.split(|b| *b == b'\n')
        .filter(|line| line.contains(&b'='))
        .count()
}

pub fn validate(new: &[u8], existing: Option<&[u8]>, min_lines: usize) -> Result<usize> {
    if std::str::from_utf8(new).is_err() {
        return Err(Error::Invalid("global.ini is not valid UTF-8".into()));
    }
    let lines = count_lines(new);
    if lines < min_lines {
        return Err(Error::Invalid(format!(
            "global.ini has {lines} lines, fewer than {min_lines}"
        )));
    }
    if let Some(existing) = existing {
        let before = count_lines(existing);
        if before >= min_lines && lines * 100 < before * 80 {
            return Err(Error::Invalid(format!(
                "global.ini has {lines} lines, under 80% of the installed {before}"
            )));
        }
    }
    Ok(lines)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallRecord {
    pub release_name: String,
    pub published_at: String,
    pub file_sha256: String,
    pub game_build: Option<String>,
    pub installed_unix: i64,
    pub added_language_line: bool,
    pub previous_language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    NotInstalled,
    Managed(InstallRecord),
    Modified(InstallRecord),
    Foreign,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn record_path(dirs: &AppDirs, scope: &Scope) -> PathBuf {
    dirs.data
        .join("starstrings")
        .join(format!("{}-{}.toml", scope.install_key, scope.channel))
}

fn read_record(dirs: &AppDirs, scope: &Scope) -> Option<InstallRecord> {
    let text = std::fs::read_to_string(record_path(dirs, scope)).ok()?;
    toml::from_str(&text).ok()
}

fn write_record(dirs: &AppDirs, scope: &Scope, record: &InstallRecord) -> Result<()> {
    let text =
        toml::to_string(record).map_err(|err| Error::Invalid(format!("install record: {err}")))?;
    write_atomic(&record_path(dirs, scope), text.as_bytes())
}

fn remove_record(dirs: &AppDirs, scope: &Scope) -> Result<()> {
    let path = record_path(dirs, scope);
    match std::fs::remove_file(&path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(Error::io(path, err)),
        _ => Ok(()),
    }
}

pub fn status(channel: &Channel, dirs: &AppDirs, scope: &Scope) -> Status {
    let Ok(ini) = load(&channel.global_ini()) else {
        return Status::NotInstalled;
    };
    let Some(file_hash) = ini.hash else {
        return Status::NotInstalled;
    };
    match read_record(dirs, scope) {
        None => Status::Foreign,
        Some(record) if record.file_sha256 == hex(&file_hash) => Status::Managed(record),
        Some(record) => Status::Modified(record),
    }
}

fn open_cfg(path: &Path, bytes: &[u8]) -> Result<UserCfg> {
    UserCfg::parse(path, bytes)
}

struct LanguageBookkeeping {
    added_language_line: bool,
    previous_language: Option<String>,
}

fn language_bookkeeping(cfg: &UserCfg, earlier: Option<&InstallRecord>) -> LanguageBookkeeping {
    match (cfg.get(LANGUAGE_KEY), earlier) {
        (Some(LANGUAGE_VALUE), Some(record)) => LanguageBookkeeping {
            added_language_line: record.added_language_line,
            previous_language: record.previous_language.clone(),
        },
        (Some(LANGUAGE_VALUE), None) => LanguageBookkeeping {
            added_language_line: false,
            previous_language: None,
        },
        (Some(other), _) => LanguageBookkeeping {
            added_language_line: false,
            previous_language: Some(other.to_string()),
        },
        (None, _) => LanguageBookkeeping {
            added_language_line: true,
            previous_language: None,
        },
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

pub fn install(
    http: &dyn Http,
    release: &Release,
    writer: &Writer,
    scope: &Scope,
    channel: &Channel,
    dirs: &AppDirs,
    min_lines: usize,
) -> Result<SnapshotInfo> {
    let zip = http.get_bytes(&release.asset_url, MAX_BYTES)?;
    if let Some(expected) = &release.sha256
        && !expected.eq_ignore_ascii_case(&hex(&hash(&zip)))
    {
        return Err(Error::Invalid(
            "download does not match the published checksum".into(),
        ));
    }
    let new_ini = extract_global_ini(&zip)?;
    let ini_path = channel.global_ini();
    let ini = load(&ini_path)?;
    let existing = ini.hash.map(|_| ini.bytes.as_slice());
    validate(&new_ini, existing, min_lines)?;

    let cfg_path = channel.user_cfg();
    let cfg_file = load(&cfg_path)?;
    let mut cfg = open_cfg(&cfg_path, &cfg_file.bytes)?;
    let bookkeeping = language_bookkeeping(&cfg, read_record(dirs, scope).as_ref());
    if cfg.get(LANGUAGE_KEY) != Some(LANGUAGE_VALUE) {
        cfg.apply(&Op::Set {
            key: LANGUAGE_KEY.into(),
            value: LANGUAGE_VALUE.into(),
        })?;
    }
    let new_cfg = cfg.to_bytes();

    let mut changes = vec![Change {
        loaded: &ini,
        new_bytes: Some(&new_ini),
    }];
    if new_cfg != cfg_file.bytes || cfg_file.hash.is_none() {
        changes.push(Change {
            loaded: &cfg_file,
            new_bytes: Some(&new_cfg),
        });
    }
    let label = format!("Install StarStrings {}", release.name);
    let snapshot = writer.apply(scope, channel, &changes, &label)?;
    write_record(
        dirs,
        scope,
        &InstallRecord {
            release_name: release.name.clone(),
            published_at: release.published_at.clone(),
            file_sha256: hex(&hash(&new_ini)),
            game_build: channel.build_id(),
            installed_unix: now_unix(),
            added_language_line: bookkeeping.added_language_line,
            previous_language: bookkeeping.previous_language,
        },
    )?;
    Ok(snapshot)
}

pub fn uninstall(
    writer: &Writer,
    scope: &Scope,
    channel: &Channel,
    dirs: &AppDirs,
) -> Result<SnapshotInfo> {
    let (Status::Managed(record) | Status::Modified(record)) = status(channel, dirs, scope) else {
        return Err(Error::Invalid(
            "StarStrings was not installed by star-config".into(),
        ));
    };
    let ini = load(&channel.global_ini())?;
    let cfg_path = channel.user_cfg();
    let cfg_file = load(&cfg_path)?;
    let mut cfg = open_cfg(&cfg_path, &cfg_file.bytes)?;
    let is_set = cfg.get(LANGUAGE_KEY).is_some();
    if record.added_language_line && is_set {
        cfg.apply(&Op::Remove {
            key: LANGUAGE_KEY.into(),
        })?;
    } else if let Some(previous) = &record.previous_language {
        cfg.apply(&Op::Set {
            key: LANGUAGE_KEY.into(),
            value: previous.clone(),
        })?;
    }
    let new_cfg = cfg.to_bytes();

    let mut changes = vec![Change {
        loaded: &ini,
        new_bytes: None,
    }];
    if new_cfg != cfg_file.bytes {
        changes.push(Change {
            loaded: &cfg_file,
            new_bytes: Some(&new_cfg),
        });
    }
    let snapshot = writer.apply(scope, channel, &changes, "Uninstall StarStrings")?;
    remove_record(dirs, scope)?;
    Ok(snapshot)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextAction {
    Install,
    Update,
    UpToDate,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advice {
    pub action: NextAction,
    pub build_changed: bool,
    pub not_live: bool,
}

pub fn advise(status: &Status, latest: Option<&Release>, channel: &Channel) -> Advice {
    let record = match status {
        Status::Managed(record) | Status::Modified(record) => Some(record),
        Status::NotInstalled | Status::Foreign => None,
    };
    let action = match (latest, record) {
        (None, _) => NextAction::Unavailable,
        (Some(_), None) => NextAction::Install,
        (Some(release), Some(record))
            if record.release_name == release.name
                && record.published_at == release.published_at =>
        {
            NextAction::UpToDate
        }
        (Some(_), Some(_)) => NextAction::Update,
    };
    let build_changed = record
        .and_then(|record| record.game_build.as_ref())
        .is_some_and(|built| Some(built) != channel.build_id().as_ref());
    Advice {
        action,
        build_changed,
        not_live: !channel.name.eq_ignore_ascii_case("LIVE"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::BackupStore;
    use crate::process::{FixedProbe, RunState};
    use std::fs;
    use std::io::Write as _;
    use std::sync::{Arc, Mutex};
    use tempfile::{TempDir, tempdir};

    struct FakeHttp {
        text: Option<String>,
        bytes: Vec<u8>,
        asked: Mutex<Vec<String>>,
    }

    impl FakeHttp {
        fn bytes(bytes: Vec<u8>) -> Self {
            Self {
                text: None,
                bytes,
                asked: Mutex::new(Vec::new()),
            }
        }
    }

    impl Http for FakeHttp {
        fn get_text(&self, url: &str) -> Result<Option<String>> {
            self.asked.lock().unwrap().push(url.to_string());
            Ok(self.text.clone())
        }

        fn get_bytes(&self, url: &str, _max_bytes: u64) -> Result<Vec<u8>> {
            self.asked.lock().unwrap().push(url.to_string());
            Ok(self.bytes.clone())
        }
    }

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn ini(lines: usize) -> Vec<u8> {
        let mut out = b"\xef\xbb\xbf".to_vec();
        for i in 0..lines {
            out.extend(format!("key{i}=value{i}\r\n").bytes());
        }
        out
    }

    fn release_for(zip: &[u8]) -> Release {
        Release {
            name: "SC LIVE Build (test)".into(),
            published_at: "2026-11-12T17:31:51Z".into(),
            asset_url:
                "https://github.com/MrKraken/StarStrings/releases/download/latest/StarStrings-LIVE.zip"
                    .into(),
            asset_size: zip.len() as u64,
            sha256: Some(hex(&hash(zip))),
        }
    }

    struct Env {
        _root: TempDir,
        channel: Channel,
        scope: Scope,
        dirs: AppDirs,
        writer: Writer,
    }

    fn env_with(game: RunState) -> Env {
        let root = tempdir().unwrap();
        let channel = Channel {
            name: "LIVE".into(),
            dir: root.path().join("LIVE"),
        };
        fs::create_dir_all(&channel.dir).unwrap();
        let writer = Writer::new(
            BackupStore::new(root.path().join("backups")),
            Arc::new(FixedProbe {
                game,
                launcher: RunState::NotRunning,
            }),
            false,
        );
        Env {
            dirs: AppDirs::under(root.path()),
            scope: Scope {
                install_key: "k".into(),
                channel: "LIVE".into(),
            },
            channel,
            writer,
            _root: root,
        }
    }

    fn env() -> Env {
        env_with(RunState::NotRunning)
    }

    impl Env {
        fn write(&self, path: &Path, bytes: &[u8]) {
            write_atomic(path, bytes).unwrap();
        }

        fn install(&self, ini_bytes: &[u8]) -> Result<SnapshotInfo> {
            let zip = zip_with(&[(ENTRY_PATH, ini_bytes)]);
            let http = FakeHttp::bytes(zip.clone());
            install(
                &http,
                &release_for(&zip),
                &self.writer,
                &self.scope,
                &self.channel,
                &self.dirs,
                10,
            )
        }

        fn snapshots(&self) -> usize {
            self.writer.store().list(&self.scope).unwrap().len()
        }

        fn cfg_text(&self) -> String {
            String::from_utf8(fs::read(self.channel.user_cfg()).unwrap()).unwrap()
        }
    }

    fn assert_nothing_written(env: &Env) {
        assert!(!env.channel.global_ini().exists());
        assert!(!env.channel.user_cfg().exists());
        assert_eq!(env.snapshots(), 0);
        assert!(read_record(&env.dirs, &env.scope).is_none());
        assert!(!record_path(&env.dirs, &env.scope).exists());
    }

    fn record() -> InstallRecord {
        InstallRecord {
            release_name: "r1".into(),
            published_at: "t1".into(),
            file_sha256: "00".into(),
            game_build: Some("b1".into()),
            installed_unix: 0,
            added_language_line: true,
            previous_language: None,
        }
    }

    #[test]
    fn allowed_url_accepts_github_hosts_only() {
        assert!(allowed_url("https://github.com/a/b"));
        assert!(allowed_url("https://api.github.com/repos/x"));
        assert!(allowed_url("https://objects.githubusercontent.com/x?y=1"));
        assert!(allowed_url("https://release-assets.githubusercontent.com"));
        assert!(!allowed_url("http://github.com/a"));
        assert!(!allowed_url("https://example.com/a"));
        assert!(!allowed_url("https://github.com.evil.com/a"));
        assert!(!allowed_url("https://evil.com/github.com"));
        assert!(!allowed_url("https://github.com:8443/a"));
        assert!(!allowed_url("https://user@github.com/a"));
        assert!(!allowed_url("https://github.com@evil.com/a"));
        assert!(!allowed_url("https://githubusercontent.com/a"));
        assert!(!allowed_url("ftp://github.com/a"));
        assert!(!allowed_url(""));
    }

    const FIXTURE: &str = include_str!("../tests/fixtures/starstrings/release.json");

    #[test]
    fn parse_release_reads_fixture() {
        let release = parse_release(FIXTURE).unwrap().unwrap();
        assert_eq!(
            release,
            Release {
                name: "SC LIVE Build (release-2026-11-12-2c6c06f)".into(),
                published_at: "2026-11-12T17:31:51Z".into(),
                asset_url:
                    "https://github.com/MrKraken/StarStrings/releases/download/latest/StarStrings-LIVE.zip"
                        .into(),
                asset_size: 2448158,
                sha256: Some(
                    "d87f62106331fe06cf0fa5207ece7c0c6f9174677eccd260d2db9ade9f2b9906".into()
                ),
            }
        );
    }

    #[test]
    fn parse_release_without_matching_asset_is_none() {
        let json = r#"{"name":"n","published_at":"t","assets":[{"name":"other.zip","size":1,"browser_download_url":"https://github.com/x"}]}"#;
        assert_eq!(parse_release(json).unwrap(), None);
    }

    #[test]
    fn parse_release_without_digest_has_none() {
        for digest in ["", r#","digest":null"#, r#","digest":"md5:abcd""#] {
            let json = format!(
                r#"{{"name":"n","published_at":"t","assets":[{{"name":"{ASSET_NAME}","size":5,"browser_download_url":"https://github.com/x"{digest}}}]}}"#
            );
            assert_eq!(parse_release(&json).unwrap().unwrap().sha256, None);
        }
    }

    #[test]
    fn parse_release_garbage_is_invalid() {
        assert!(matches!(parse_release("not json"), Err(Error::Invalid(_))));
    }

    #[test]
    fn latest_none_on_404() {
        let http = FakeHttp::bytes(Vec::new());
        assert_eq!(latest(&http).unwrap(), None);
        assert_eq!(*http.asked.lock().unwrap(), [RELEASE_API]);
    }

    #[test]
    fn latest_parses_the_response() {
        let http = FakeHttp {
            text: Some(FIXTURE.into()),
            bytes: Vec::new(),
            asked: Mutex::new(Vec::new()),
        };
        assert_eq!(latest(&http).unwrap().unwrap().asset_size, 2448158);
    }

    #[test]
    fn extract_finds_entry_ignoring_case_and_backslashes() {
        let zip = zip_with(&[
            ("readme.txt", b"hi"),
            ("data\\localization\\English\\GLOBAL.INI", b"a=b"),
        ]);
        assert_eq!(extract_global_ini(&zip).unwrap(), b"a=b");
    }

    #[test]
    fn extract_rejects_missing_duplicate_and_non_zip() {
        let missing = zip_with(&[("other.ini", b"a=b")]);
        let twice = zip_with(&[
            (ENTRY_PATH, b"a=b"),
            ("data/localization/english/GLOBAL.ini", b"c=d"),
        ]);
        for bytes in [missing.as_slice(), twice.as_slice(), b"not a zip at all"] {
            assert!(matches!(extract_global_ini(bytes), Err(Error::Invalid(_))));
        }
    }

    #[test]
    fn extract_rejects_truncated_zip() {
        let zip = zip_with(&[(ENTRY_PATH, b"a=b")]);
        assert!(matches!(
            extract_global_ini(&zip[..zip.len() / 2]),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn extract_ignores_other_entries_even_with_traversal_names() {
        let zip = zip_with(&[("../../evil", b"x"), (ENTRY_PATH, b"a=b")]);
        assert_eq!(extract_global_ini(&zip).unwrap(), b"a=b");
    }

    #[test]
    fn count_lines_counts_only_lines_with_equals() {
        assert_eq!(count_lines(b"a=b\n\nnoequals\r\n; c=d\nx="), 3);
        assert_eq!(count_lines(b""), 0);
    }

    #[test]
    fn validate_rejects_non_utf8_too_few_lines_and_big_shrink() {
        assert_eq!(validate(&ini(10), None, 10).unwrap(), 10);
        assert!(validate(&ini(9), None, 10).is_err());
        assert!(validate(b"a=\xff\xfe\n", None, 1).is_err());
        let existing = ini(100);
        assert!(validate(&ini(79), Some(&existing), 10).is_err());
        assert!(validate(&ini(80), Some(&existing), 10).is_ok());
        let small = ini(5);
        assert!(validate(&ini(10), Some(&small), 10).is_ok());
    }

    #[test]
    fn status_not_installed_foreign_managed_modified() {
        let env = env();
        let status_now = || status(&env.channel, &env.dirs, &env.scope);
        assert_eq!(status_now(), Status::NotInstalled);
        env.write(&env.channel.global_ini(), b"a=b\n");
        assert_eq!(status_now(), Status::Foreign);
        let mut rec = record();
        rec.file_sha256 = hex(&hash(b"a=b\n"));
        write_record(&env.dirs, &env.scope, &rec).unwrap();
        assert_eq!(status_now(), Status::Managed(rec.clone()));
        env.write(&env.channel.global_ini(), b"a=c\n");
        assert_eq!(status_now(), Status::Modified(rec));
    }

    #[test]
    fn install_writes_ini_and_language_line_in_one_snapshot() {
        let env = env();
        env.write(&env.channel.user_cfg(), b"r_width = 1920\r\n");
        let bytes = ini(12);
        let info = env.install(&bytes).unwrap();
        assert_eq!(fs::read(env.channel.global_ini()).unwrap(), bytes);
        assert!(env.cfg_text().contains("g_language = english"));
        assert_eq!(env.snapshots(), 1);
        assert_eq!(info.files.len(), 2);
        let Status::Managed(rec) = status(&env.channel, &env.dirs, &env.scope) else {
            panic!("expected managed");
        };
        assert!(rec.added_language_line);
        assert_eq!(rec.previous_language, None);
        assert_eq!(rec.file_sha256, hex(&hash(&bytes)));
        assert_eq!(info.label, "Install StarStrings SC LIVE Build (test)");
    }

    #[test]
    fn install_keeps_other_user_cfg_lines_and_records_previous_language() {
        let env = env();
        env.write(
            &env.channel.user_cfg(),
            b"r_width = 1920\r\ng_language = german\r\nr_height = 1080\r\n",
        );
        env.install(&ini(12)).unwrap();
        assert_eq!(
            env.cfg_text(),
            "r_width = 1920\r\ng_language = english\r\nr_height = 1080\r\n"
        );
        let Status::Managed(rec) = status(&env.channel, &env.dirs, &env.scope) else {
            panic!("expected managed");
        };
        assert!(!rec.added_language_line);
        assert_eq!(rec.previous_language.as_deref(), Some("german"));
    }

    #[test]
    fn install_skips_user_cfg_when_already_english() {
        let env = env();
        env.write(&env.channel.user_cfg(), b"g_language = english\r\n");
        let info = env.install(&ini(12)).unwrap();
        assert_eq!(info.files.len(), 1);
        assert_eq!(env.cfg_text(), "g_language = english\r\n");
    }

    #[test]
    fn reinstall_keeps_the_original_language_bookkeeping() {
        let env = env();
        env.write(&env.channel.user_cfg(), b"g_language = german\r\n");
        env.install(&ini(12)).unwrap();
        env.install(&ini(13)).unwrap();
        uninstall(&env.writer, &env.scope, &env.channel, &env.dirs).unwrap();
        assert_eq!(env.cfg_text(), "g_language = german\r\n");
    }

    #[test]
    fn install_refuses_checksum_mismatch_and_writes_nothing() {
        let env = env();
        let zip = zip_with(&[(ENTRY_PATH, &ini(12))]);
        let mut release = release_for(&zip);
        release.sha256 = Some("ab".repeat(32));
        let http = FakeHttp::bytes(zip);
        let err = install(
            &http,
            &release,
            &env.writer,
            &env.scope,
            &env.channel,
            &env.dirs,
            10,
        )
        .unwrap_err();
        assert!(
            matches!(&err, Error::Invalid(m) if m == "download does not match the published checksum")
        );
        assert_nothing_written(&env);
    }

    #[test]
    fn install_refuses_invalid_ini_and_writes_nothing() {
        let env = env();
        assert!(matches!(env.install(&ini(3)), Err(Error::Invalid(_))));
        assert_nothing_written(&env);
    }

    #[test]
    fn install_refuses_non_zip_and_writes_nothing() {
        let env = env();
        let http = FakeHttp::bytes(b"junk".to_vec());
        let mut release = release_for(b"junk");
        release.sha256 = None;
        let err = install(
            &http,
            &release,
            &env.writer,
            &env.scope,
            &env.channel,
            &env.dirs,
            10,
        );
        assert!(matches!(err, Err(Error::Invalid(_))));
        assert_nothing_written(&env);
    }

    #[test]
    fn install_refuses_when_game_running() {
        let env = env_with(RunState::Running);
        assert!(matches!(env.install(&ini(12)), Err(Error::GameRunning)));
        assert_nothing_written(&env);
    }

    #[test]
    fn install_into_channel_with_lowercase_data_folder_reuses_it() {
        let env = env();
        fs::create_dir_all(env.channel.dir.join("data/Localization/english")).unwrap();
        env.install(&ini(12)).unwrap();
        assert!(
            env.channel
                .dir
                .join("data/Localization/english/global.ini")
                .exists()
        );
        assert!(!env.channel.dir.join("Data").exists());
    }

    #[test]
    fn uninstall_removes_ini_and_added_line() {
        let env = env();
        env.write(&env.channel.user_cfg(), b"r_width = 1920\r\n");
        env.install(&ini(12)).unwrap();
        let info = uninstall(&env.writer, &env.scope, &env.channel, &env.dirs).unwrap();
        assert!(!env.channel.global_ini().exists());
        assert_eq!(env.cfg_text(), "r_width = 1920\r\n");
        assert!(!record_path(&env.dirs, &env.scope).exists());
        assert_eq!(info.label, "Uninstall StarStrings");
        assert_eq!(
            status(&env.channel, &env.dirs, &env.scope),
            Status::NotInstalled
        );
    }

    #[test]
    fn uninstall_restores_previous_language() {
        let env = env();
        env.write(&env.channel.user_cfg(), b"g_language = german\r\n");
        env.install(&ini(12)).unwrap();
        uninstall(&env.writer, &env.scope, &env.channel, &env.dirs).unwrap();
        assert_eq!(env.cfg_text(), "g_language = german\r\n");
    }

    #[test]
    fn uninstall_foreign_is_invalid() {
        let env = env();
        let run = || uninstall(&env.writer, &env.scope, &env.channel, &env.dirs);
        assert!(matches!(run(), Err(Error::Invalid(_))));
        env.write(&env.channel.global_ini(), b"a=b\n");
        assert!(matches!(run(), Err(Error::Invalid(_))));
        assert!(env.channel.global_ini().exists());
        assert_eq!(env.snapshots(), 0);
    }

    fn rel(name: &str, published: &str) -> Release {
        Release {
            name: name.into(),
            published_at: published.into(),
            asset_url: String::new(),
            asset_size: 0,
            sha256: None,
        }
    }

    fn live() -> Channel {
        Channel {
            name: "LIVE".into(),
            dir: "/nonexistent/LIVE".into(),
        }
    }

    #[test]
    fn advise_covers_install_update_up_to_date_unavailable() {
        let channel = live();
        let latest = rel("r1", "t1");
        let action =
            |status: &Status, latest: Option<&Release>| advise(status, latest, &channel).action;
        assert_eq!(action(&Status::Foreign, None), NextAction::Unavailable);
        assert_eq!(
            action(&Status::NotInstalled, Some(&latest)),
            NextAction::Install
        );
        assert_eq!(action(&Status::Foreign, Some(&latest)), NextAction::Install);
        assert_eq!(
            action(&Status::Managed(record()), Some(&latest)),
            NextAction::UpToDate
        );
        assert_eq!(
            action(&Status::Modified(record()), Some(&latest)),
            NextAction::UpToDate
        );
        assert_eq!(
            action(&Status::Managed(record()), Some(&rel("r2", "t1"))),
            NextAction::Update
        );
        assert_eq!(
            action(&Status::Managed(record()), Some(&rel("r1", "t2"))),
            NextAction::Update
        );
    }

    #[test]
    fn advise_flags_build_change_and_non_live_channel() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("build_manifest.id"),
            r#"{"Data":{"RequestedP4ChangeNum":"b2"}}"#,
        )
        .unwrap();
        let ptu = Channel {
            name: "ptu".into(),
            dir: dir.path().to_path_buf(),
        };
        let latest = rel("r1", "t1");
        let advice = advise(&Status::Managed(record()), Some(&latest), &ptu);
        assert!(advice.build_changed);
        assert!(advice.not_live);
        let mut same = record();
        same.game_build = Some("b2".into());
        let advice = advise(&Status::Managed(same), Some(&latest), &ptu);
        assert!(!advice.build_changed);
        let mut unknown = record();
        unknown.game_build = None;
        assert!(!advise(&Status::Managed(unknown), Some(&latest), &ptu).build_changed);
        let advice = advise(&Status::NotInstalled, Some(&latest), &live());
        assert!(!advice.build_changed);
        assert!(!advice.not_live);
    }
}
