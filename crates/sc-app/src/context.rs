use sc_core::backup::{BackupStore, Scope};
use sc_core::dirs::AppDirs;
use sc_core::guard::Writer;
use sc_core::install::{Channel, DiscoveryEnv, Install, InstallKind, discover};
use sc_core::process::{GameProbe, RunState, SystemProbe};
use sc_core::settings::Settings;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Discovery {
    System,
    ExtrasOnly,
}

/// Everything a screen may read about the app and the selected game files.
#[derive(Clone)]
pub struct Context {
    pub dirs: AppDirs,
    pub settings: Settings,
    pub installs: Vec<Install>,
    pub install: Option<Install>,
    pub channels: Vec<Channel>,
    pub channel: Option<Channel>,
    pub game: RunState,
    pub launcher: RunState,
    pub unlocked: bool,
    probe: Arc<dyn GameProbe>,
    discovery: Discovery,
    home: Option<PathBuf>,
}

impl Context {
    /// Builds a context from known installs, selecting the saved install and channel
    /// when they are still valid. Rediscovery only looks at `settings.installs`.
    pub fn new(dirs: AppDirs, settings: Settings, installs: Vec<Install>) -> Self {
        let mut ctx = Self {
            dirs,
            settings,
            installs,
            install: None,
            channels: Vec::new(),
            channel: None,
            game: RunState::NotRunning,
            launcher: RunState::NotRunning,
            unlocked: false,
            probe: Arc::new(SystemProbe),
            discovery: Discovery::ExtrasOnly,
            home: home_dir(),
        };
        let saved = ctx.settings.selected_install.clone();
        let install = saved
            .and_then(|root| ctx.installs.iter().find(|i| i.root == root).cloned())
            .or_else(|| ctx.installs.first().cloned());
        let channels = install.as_ref().map(Install::channels).unwrap_or_default();
        ctx.set_install(install, channels);
        ctx
    }

    /// Loads settings and discovers installs on this machine. Blocking; call before the UI starts.
    pub fn from_system(dirs: AppDirs) -> Self {
        let settings = Settings::load(&dirs.settings_file());
        let installs = discover(&DiscoveryEnv::from_system(settings.installs.clone()));
        let mut ctx = Self::new(dirs, settings, installs);
        ctx.discovery = Discovery::System;
        ctx.game = ctx.probe.game();
        ctx.launcher = ctx.probe.launcher();
        ctx
    }

    pub fn with_probe(mut self, probe: Arc<dyn GameProbe>) -> Self {
        self.probe = probe;
        self
    }

    /// Sets the folder shown as `~` in paths. Tests use it so snapshots do not show temp paths.
    pub fn with_home(mut self, home: impl Into<PathBuf>) -> Self {
        self.home = Some(home.into());
        self
    }

    /// `path` for display, with the home folder shortened to `~`. Never use it to open files.
    pub fn display_path(&self, path: &Path) -> String {
        match self
            .home
            .as_deref()
            .and_then(|home| path.strip_prefix(home).ok())
        {
            Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
            Some(rest) => format!("~{MAIN_SEPARATOR}{}", rest.display()),
            None => path.display().to_string(),
        }
    }

    /// How an install is named in the UI: its Wine prefix, or its folder, shortened with `~`.
    pub fn install_label(&self, install: &Install) -> String {
        match &install.kind {
            InstallKind::WinePrefix { prefix } => self.display_path(prefix),
            InstallKind::Native => self.display_path(&install.root),
        }
    }

    pub fn probe(&self) -> Arc<dyn GameProbe> {
        self.probe.clone()
    }

    pub fn writer(&self) -> Writer {
        Writer::new(
            BackupStore::new(self.dirs.backups()),
            self.probe.clone(),
            self.unlocked,
        )
    }

    pub fn scope(&self) -> Option<Scope> {
        Some(Scope {
            install_key: self.install.as_ref()?.key(),
            channel: self.channel.as_ref()?.name.clone(),
        })
    }

    pub fn read_only(&self) -> bool {
        self.game != RunState::NotRunning && !self.unlocked
    }

    /// Selects `install` with its already loaded `channels`, keeping the channel name when it exists.
    pub fn set_install(&mut self, install: Option<Install>, channels: Vec<Channel>) {
        let wanted = self
            .channel
            .as_ref()
            .map(|channel| channel.name.clone())
            .or_else(|| self.settings.selected_channel.clone());
        self.channel = wanted
            .and_then(|name| channels.iter().find(|c| c.name == name).cloned())
            .or_else(|| channels.first().cloned());
        self.channels = channels;
        self.install = install;
        self.remember_selection();
    }

    pub fn set_channel(&mut self, name: &str) -> bool {
        let Some(channel) = self.channels.iter().find(|c| c.name == name).cloned() else {
            return false;
        };
        self.channel = Some(channel);
        self.remember_selection();
        true
    }

    fn remember_selection(&mut self) {
        if let Some(install) = &self.install {
            self.settings.selected_install = Some(install.root.clone());
        }
        if let Some(channel) = &self.channel {
            self.settings.selected_channel = Some(channel.name.clone());
        }
    }

    /// Finds installs again. Blocking; run it through a task.
    pub fn discover(&self) -> Vec<Install> {
        discover_with(self.discovery_env())
    }

    pub(crate) fn discovery_env(&self) -> DiscoveryRequest {
        DiscoveryRequest {
            system: self.discovery == Discovery::System,
            extra: self.settings.installs.clone(),
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

pub(crate) struct DiscoveryRequest {
    system: bool,
    extra: Vec<PathBuf>,
}

pub(crate) fn discover_with(request: DiscoveryRequest) -> Vec<Install> {
    let env = if request.system {
        DiscoveryEnv::from_system(request.extra)
    } else {
        DiscoveryEnv {
            home: None,
            drive_roots: Vec::new(),
            extra: request.extra,
        }
    };
    discover(&env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::process::FixedProbe;
    use std::fs;

    fn install_with(root: &std::path::Path, channels: &[&str]) -> Install {
        let sc = root.join("StarCitizen");
        for name in channels {
            fs::create_dir_all(sc.join(name).join("USER")).unwrap();
        }
        fs::create_dir_all(&sc).unwrap();
        Install::from_dir(&sc).unwrap()
    }

    #[test]
    fn new_selects_saved_channel_when_valid() {
        let temp = tempfile::tempdir().unwrap();
        let install = install_with(temp.path(), &["LIVE", "PTU"]);
        let settings = Settings {
            selected_channel: Some("PTU".into()),
            ..Settings::default()
        };
        let ctx = Context::new(AppDirs::under(temp.path()), settings, vec![install]);
        assert_eq!(ctx.channel.unwrap().name, "PTU");
    }

    #[test]
    fn new_falls_back_to_first_channel() {
        let temp = tempfile::tempdir().unwrap();
        let install = install_with(temp.path(), &["LIVE", "PTU"]);
        let settings = Settings {
            selected_channel: Some("EPTU".into()),
            ..Settings::default()
        };
        let ctx = Context::new(AppDirs::under(temp.path()), settings, vec![install]);
        assert_eq!(ctx.channel.unwrap().name, "LIVE");
        assert_eq!(ctx.settings.selected_channel.as_deref(), Some("LIVE"));
    }

    #[test]
    fn read_only_follows_game_and_unlock() {
        let temp = tempfile::tempdir().unwrap();
        let mut ctx = Context::new(AppDirs::under(temp.path()), Settings::default(), Vec::new());
        assert!(!ctx.read_only());
        ctx.game = RunState::Unknown;
        assert!(ctx.read_only());
        ctx.unlocked = true;
        assert!(!ctx.read_only());
    }

    #[test]
    fn display_path_shortens_home() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = Context::new(AppDirs::under(temp.path()), Settings::default(), Vec::new())
            .with_home(temp.path().join("home"));
        let inside = temp.path().join("home").join("Games").join("star-citizen");
        assert_eq!(
            ctx.display_path(&inside),
            format!("~{MAIN_SEPARATOR}Games{MAIN_SEPARATOR}star-citizen")
        );
        assert_eq!(
            ctx.display_path(temp.path()),
            temp.path().display().to_string()
        );
    }

    #[test]
    fn writer_uses_injected_probe() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = Context::new(AppDirs::under(temp.path()), Settings::default(), Vec::new())
            .with_probe(Arc::new(FixedProbe {
                game: RunState::Running,
                launcher: RunState::NotRunning,
            }));
        assert!(ctx.writer().blocked());
    }
}
