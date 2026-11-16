//! Application settings.

use crate::error::{Error, Result};
use crate::safe_write::write_atomic;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeChoice {
    #[default]
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ArchiveMode {
    #[default]
    Move,
    Copy,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StarStringsSettings {
    pub check_on_start: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenshotSettings {
    pub archive_dir: Option<PathBuf>,
    pub archive_mode: ArchiveMode,
    pub share_max_bytes: u64,
    pub share_max_edge: u32,
}

impl Default for ScreenshotSettings {
    fn default() -> Self {
        Self {
            archive_dir: None,
            archive_mode: ArchiveMode::Move,
            share_max_bytes: 10 * 1024 * 1024,
            share_max_edge: 2560,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ShaderSettings {
    pub backup_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub installs: Vec<PathBuf>,
    pub selected_install: Option<PathBuf>,
    pub selected_channel: Option<String>,
    pub theme: ThemeChoice,
    pub launch_commands: BTreeMap<String, String>,
    pub confirmed_launch: BTreeMap<String, String>,
    pub snapshot_keep: usize,
    pub starstrings: StarStringsSettings,
    pub screenshots: ScreenshotSettings,
    pub shaders: ShaderSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            installs: Vec::new(),
            selected_install: None,
            selected_channel: None,
            theme: ThemeChoice::default(),
            launch_commands: BTreeMap::new(),
            confirmed_launch: BTreeMap::new(),
            snapshot_keep: 50,
            starstrings: StarStringsSettings::default(),
            screenshots: ScreenshotSettings::default(),
            shaders: ShaderSettings::default(),
        }
    }
}

fn bad_copy_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(OsString::from).unwrap_or_default();
    name.push(".bad");
    path.with_file_name(name)
}

fn set_aside(path: &Path) {
    let bad = bad_copy_path(path);
    let _ = fs::remove_file(&bad);
    let _ = fs::rename(path, bad);
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        let Ok(bytes) = fs::read(path) else {
            return Settings::default();
        };
        let parsed = String::from_utf8(bytes)
            .ok()
            .and_then(|text| toml::from_str::<Settings>(&text).ok());
        parsed.unwrap_or_else(|| {
            set_aside(path);
            Settings::default()
        })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).map_err(|err| Error::Invalid(err.to_string()))?;
        write_atomic(path, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn load_missing_gives_defaults() {
        let dir = tempdir().unwrap();
        let settings = Settings::load(&dir.path().join("settings.toml"));
        assert_eq!(settings.snapshot_keep, 50);
        assert_eq!(settings.theme, ThemeChoice::Dark);
        assert_eq!(settings.screenshots.share_max_edge, 2560);
        assert_eq!(settings.screenshots.share_max_bytes, 10_485_760);
        assert_eq!(settings.screenshots.archive_mode, ArchiveMode::Move);
        assert!(!settings.starstrings.check_on_start);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config/settings.toml");
        let mut settings = Settings::default();
        settings
            .installs
            .push("/games/Zoë Games/StarCitizen".into());
        settings.selected_install = Some("/games/Zoë Games/StarCitizen".into());
        settings.selected_channel = Some("PTU".into());
        settings.theme = ThemeChoice::Light;
        settings
            .launch_commands
            .insert("abc".into(), "lutris \"lutris:rungame/sc\"".into());
        settings
            .confirmed_launch
            .insert("abc".into(), "/x/sc-launch.sh".into());
        settings.snapshot_keep = 7;
        settings.starstrings.check_on_start = true;
        settings.screenshots.archive_dir = Some("/shots".into());
        settings.screenshots.archive_mode = ArchiveMode::Copy;
        settings.shaders.backup_dir = Some("/shaders".into());
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
    }

    #[test]
    fn load_corrupt_file_gives_defaults_and_keeps_bad_copy() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        fs::write(&path, "this is not toml [").unwrap();
        fs::write(dir.path().join("settings.toml.bad"), "older").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        assert!(!path.exists());
        assert_eq!(
            fs::read_to_string(dir.path().join("settings.toml.bad")).unwrap(),
            "this is not toml ["
        );
    }

    #[test]
    fn load_wrong_type_counts_as_corrupt() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        fs::write(&path, "snapshot_keep = \"many\"\n").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        assert!(dir.path().join("settings.toml.bad").exists());
    }

    #[test]
    fn load_ignores_unknown_keys_and_fills_missing_ones() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        fs::write(&path, "future_key = 1\ntheme = \"Light\"\n").unwrap();
        let settings = Settings::load(&path);
        assert_eq!(settings.theme, ThemeChoice::Light);
        assert_eq!(settings.snapshot_keep, 50);
        assert!(path.exists());
    }
}
