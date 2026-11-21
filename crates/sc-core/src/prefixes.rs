//! Where a Wine prefix with Star Citizen may live on Linux: the default folders, the LUG
//! helper's config, nix-citizen's generated launcher scripts and `WINEPREFIX`.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_FOLDERS: [&str; 2] = ["Games/star-citizen", "Games/rsi-launcher"];
const LUG_CONFIG_DIR: &str = "starcitizen-lug";
const LUG_CONFIG_FILES: [&str; 2] = ["winedir.conf", "gamedir.conf"];
const NIX_CITIZEN_SCRIPTS: [&str; 2] = ["rsi-launcher", "star-citizen"];
const NIX_STORE: &str = "/nix/store";
const LOCATION_START: &str = "export WINEPREFIX=\"$(readlink -f \"";
const LOCATION_END: &str = "\")\"";
const NIX_CITIZEN_SUBPREFIX: &str = "prefix";
const MAX_SCRIPT_BYTES: u64 = 1 << 20;
const MAX_SCRIPTS_READ: usize = 64;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrefixSources {
    pub home: Option<PathBuf>,
    pub config_dir: Option<PathBuf>,
    pub wine_prefix: Option<PathBuf>,
    pub path_dirs: Vec<PathBuf>,
    pub store_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherScript {
    pub path: PathBuf,
    pub prefix: Option<PathBuf>,
}

impl PrefixSources {
    /// Only the home folder on Windows, where none of the Linux tools apply.
    pub fn from_system() -> Self {
        let dirs = directories::BaseDirs::new();
        let home = dirs.as_ref().map(|dirs| dirs.home_dir().to_path_buf());
        if cfg!(windows) {
            return Self {
                home,
                ..Self::default()
            };
        }
        Self {
            home,
            config_dir: dirs.as_ref().map(|dirs| dirs.config_dir().to_path_buf()),
            wine_prefix: std::env::var_os("WINEPREFIX")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            path_dirs: std::env::var_os("PATH")
                .map(|value| std::env::split_paths(&value).collect())
                .unwrap_or_default(),
            store_dir: Some(PathBuf::from(NIX_STORE)),
        }
    }

    pub fn candidates(&self) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = self.default_folders();
        found.extend(self.lug_helper_paths());
        found.extend(
            self.launcher_scripts()
                .into_iter()
                .filter_map(|script| script.prefix),
        );
        found.extend(self.wine_prefix.clone());
        found
    }

    fn default_folders(&self) -> Vec<PathBuf> {
        let Some(home) = &self.home else {
            return Vec::new();
        };
        DEFAULT_FOLDERS
            .iter()
            .map(|folder| home.join(folder))
            .collect()
    }

    fn lug_helper_paths(&self) -> Vec<PathBuf> {
        let Some(config_dir) = &self.config_dir else {
            return Vec::new();
        };
        let dir = config_dir.join(LUG_CONFIG_DIR);
        LUG_CONFIG_FILES
            .iter()
            .filter_map(|file| fs::read_to_string(dir.join(file)).ok())
            .filter_map(|text| first_line_path(&text))
            .collect()
    }

    /// nix-citizen's `rsi-launcher` and `star-citizen` commands on `PATH`, each with the prefix
    /// baked into it when that can be read.
    pub fn launcher_scripts(&self) -> Vec<LauncherScript> {
        NIX_CITIZEN_SCRIPTS
            .iter()
            .filter_map(|name| self.on_path(name))
            .map(|path| LauncherScript {
                prefix: self.baked_prefix(&path),
                path,
            })
            .collect()
    }

    fn on_path(&self, name: &str) -> Option<PathBuf> {
        self.path_dirs
            .iter()
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    }

    /// The command on `PATH` is often a wrapper (an FHS sandbox) that calls the real script
    /// by its store path, so store paths named in each file are followed until one sets the
    /// prefix.
    fn baked_prefix(&self, script: &Path) -> Option<PathBuf> {
        let mut queue = VecDeque::from([script.to_path_buf()]);
        let mut read: Vec<PathBuf> = Vec::new();
        while let Some(file) = queue.pop_front() {
            if read.len() >= MAX_SCRIPTS_READ {
                break;
            }
            if read.contains(&file) {
                continue;
            }
            read.push(file.clone());
            let Some(text) = small_text(&file) else {
                continue;
            };
            if let Some(location) = baked_location(&text) {
                return self
                    .expand(location)
                    .map(|location| nix_citizen_prefix(&location));
            }
            if let Some(store) = &self.store_dir {
                queue.extend(store_paths(&text, store).filter(|path| path.is_file()));
            }
        }
        None
    }

    fn expand(&self, location: &str) -> Option<PathBuf> {
        let mut text = location.to_string();
        if text.contains("$HOME") || text.contains("${HOME}") {
            let home = self.home.as_ref()?.to_str()?;
            text = text.replace("${HOME}", home).replace("$HOME", home);
        }
        if text.contains(['$', '`']) {
            return None;
        }
        let path = PathBuf::from(text);
        path.is_absolute().then_some(path)
    }
}

/// The script switches to `<location>/prefix` when that folder exists.
fn nix_citizen_prefix(location: &Path) -> PathBuf {
    let location = fs::canonicalize(location).unwrap_or_else(|_| location.to_path_buf());
    let nested = location.join(NIX_CITIZEN_SUBPREFIX);
    if nested.is_dir() { nested } else { location }
}

fn first_line_path(text: &str) -> Option<PathBuf> {
    let line = text.lines().next()?.trim();
    let path = PathBuf::from(line);
    path.is_absolute().then_some(path)
}

fn small_text(path: &Path) -> Option<String> {
    let size = fs::metadata(path).ok()?.len();
    if size > MAX_SCRIPT_BYTES {
        return None;
    }
    String::from_utf8(fs::read(path).ok()?).ok()
}

fn baked_location(text: &str) -> Option<&str> {
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix(LOCATION_START)?
            .strip_suffix(LOCATION_END)
    })
}

fn store_paths<'a>(text: &'a str, store: &Path) -> impl Iterator<Item = PathBuf> + 'a {
    let store = store.to_string_lossy().trim_end_matches('/').to_string();
    let starts: Vec<usize> = text
        .match_indices(&format!("{store}/"))
        .map(|(start, _)| start)
        .collect();
    starts.into_iter().map(move |start| {
        let rest = &text[start..];
        let end = rest
            .find(|ch: char| !is_store_path_char(ch))
            .unwrap_or(rest.len());
        PathBuf::from(rest[..end].trim_end_matches(['/', '.']))
    })
}

fn is_store_path_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || "+-._?=/".contains(ch)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn nix_script(location: &str) -> String {
        format!(
            "#!/bin/bash\nset -x\nmkdir -p \"{location}\"\n    export WINEPREFIX=\"$(readlink -f \"{location}\")\"\n"
        )
    }

    fn sources(base: &Path) -> PrefixSources {
        PrefixSources {
            home: Some(base.join("home")),
            config_dir: Some(base.join("config")),
            wine_prefix: None,
            path_dirs: vec![base.join("bin")],
            store_dir: Some(base.join("store")),
        }
    }

    #[test]
    fn defaults_cover_lug_and_nix_citizen_folders() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        assert_eq!(
            sources(dir.path()).candidates(),
            [
                home.join("Games/star-citizen"),
                home.join("Games/rsi-launcher")
            ]
        );
    }

    #[test]
    fn reads_both_lug_helper_config_files() {
        let dir = tempdir().unwrap();
        let lug = dir.path().join("config/starcitizen-lug");
        write(&lug.join("winedir.conf"), "/games/sc pfx\n");
        write(
            &lug.join("gamedir.conf"),
            "/games/sc pfx/drive_c/Program Files/Roberts Space Industries/StarCitizen\n",
        );
        let mut env = sources(dir.path());
        env.home = None;
        assert_eq!(
            env.candidates(),
            [
                PathBuf::from("/games/sc pfx"),
                PathBuf::from(
                    "/games/sc pfx/drive_c/Program Files/Roberts Space Industries/StarCitizen"
                ),
            ]
        );
    }

    #[test]
    fn ignores_empty_or_relative_lug_config() {
        let dir = tempdir().unwrap();
        let lug = dir.path().join("config/starcitizen-lug");
        write(&lug.join("winedir.conf"), "\n");
        write(&lug.join("gamedir.conf"), "Games/sc\n");
        let mut env = sources(dir.path());
        env.home = None;
        assert!(env.candidates().is_empty());
    }

    #[test]
    fn reads_location_baked_into_script_and_expands_home() {
        let dir = tempdir().unwrap();
        let env = sources(dir.path());
        let script = dir.path().join("bin/rsi-launcher");
        write(&script, &nix_script("$HOME/Games/rsi-launcher"));
        assert_eq!(
            env.launcher_scripts(),
            [LauncherScript {
                path: script,
                prefix: Some(dir.path().join("home/Games/rsi-launcher")),
            }]
        );
    }

    #[test]
    fn script_prefers_nested_prefix_folder() {
        let dir = tempdir().unwrap();
        let location = dir.path().join("games/rsi-launcher");
        fs::create_dir_all(location.join("prefix")).unwrap();
        write(
            &dir.path().join("bin/star-citizen"),
            &nix_script(&location.display().to_string()),
        );
        let scripts = sources(dir.path()).launcher_scripts();
        assert_eq!(
            scripts[0].prefix,
            Some(fs::canonicalize(&location).unwrap().join("prefix"))
        );
    }

    #[test]
    fn follows_store_paths_from_a_wrapper_to_the_script() {
        let dir = tempdir().unwrap();
        let store = dir.path().join("store");
        let init = store.join("abc-rsi-launcher-init");
        let real = store.join("def-rsi-launcher/bin/rsi-launcher");
        write(&real, &nix_script("/games/rsi-launcher"));
        write(
            &init,
            &format!(
                "#!/bin/sh\nsource /etc/profile\nexec {} \"$@\"\n",
                real.display()
            ),
        );
        write(
            &dir.path().join("bin/rsi-launcher"),
            &format!(
                "#!/bin/sh\nexec {}/zzz-bwrap/bin/bwrap --ro-bind /nix /nix {} \"$@\"\n",
                store.display(),
                init.display()
            ),
        );
        let scripts = sources(dir.path()).launcher_scripts();
        assert_eq!(
            scripts[0].prefix,
            Some(PathBuf::from("/games/rsi-launcher"))
        );
    }

    #[test]
    fn unreadable_location_leaves_prefix_unknown() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("bin/rsi-launcher"),
            &nix_script("$XDG_DATA_HOME/rsi"),
        );
        write(&dir.path().join("bin/star-citizen"), "#!/bin/sh\nexit 0\n");
        let scripts = sources(dir.path()).launcher_scripts();
        assert_eq!(scripts.len(), 2);
        assert!(scripts.iter().all(|script| script.prefix.is_none()));
    }

    #[test]
    fn first_script_on_path_wins() {
        let dir = tempdir().unwrap();
        let first = dir.path().join("bin/rsi-launcher");
        write(&first, &nix_script("/games/first"));
        write(
            &dir.path().join("later/rsi-launcher"),
            &nix_script("/games/later"),
        );
        let mut env = sources(dir.path());
        env.path_dirs.push(dir.path().join("later"));
        assert_eq!(
            env.launcher_scripts(),
            [LauncherScript {
                path: first,
                prefix: Some(PathBuf::from("/games/first")),
            }]
        );
    }

    #[test]
    fn candidates_list_script_prefixes_then_wineprefix() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("bin/rsi-launcher"),
            &nix_script("/games/rsi-launcher"),
        );
        let mut env = sources(dir.path());
        env.home = None;
        env.wine_prefix = Some(PathBuf::from("/games/other"));
        assert_eq!(
            env.candidates(),
            [
                PathBuf::from("/games/rsi-launcher"),
                PathBuf::from("/games/other")
            ]
        );
    }

    #[test]
    fn store_paths_stop_at_quotes_and_spaces() {
        let text = "exec \"/nix/store/a-b/bin/x\" /nix/store/c-d/ '/nix/store/e'";
        let found: Vec<PathBuf> = store_paths(text, Path::new("/nix/store")).collect();
        assert_eq!(
            found,
            [
                PathBuf::from("/nix/store/a-b/bin/x"),
                PathBuf::from("/nix/store/c-d"),
                PathBuf::from("/nix/store/e"),
            ]
        );
    }
}
