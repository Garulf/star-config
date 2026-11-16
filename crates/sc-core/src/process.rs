//! Game and launcher detection.

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Running,
    NotRunning,
    Unknown,
}

pub trait GameProbe: Send + Sync {
    fn game(&self) -> RunState;
    fn launcher(&self) -> RunState;
}

pub struct SystemProbe;

pub struct FixedProbe {
    pub game: RunState,
    pub launcher: RunState,
}

impl GameProbe for FixedProbe {
    fn game(&self) -> RunState {
        self.game
    }

    fn launcher(&self) -> RunState {
        self.launcher
    }
}

fn mentions(needle: &str, name: &str, cmdline: &[String]) -> bool {
    std::iter::once(name)
        .chain(cmdline.iter().map(String::as_str))
        .any(|text| text.to_lowercase().contains(needle))
}

pub fn matches_game(name: &str, cmdline: &[String]) -> bool {
    mentions("starcitizen.exe", name, cmdline)
}

pub fn matches_launcher(name: &str, cmdline: &[String]) -> bool {
    mentions("rsi launcher.exe", name, cmdline)
}

fn scan(matcher: fn(&str, &[String]) -> bool) -> RunState {
    let Ok(mut system) = System::new() else {
        return RunState::Unknown;
    };
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );
    if system.processes().is_empty() {
        return RunState::Unknown;
    }
    let found = system.processes().values().any(|process| {
        let cmdline: Vec<String> = process
            .cmd()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        matcher(&process.name().to_string_lossy(), &cmdline)
    });
    if found {
        RunState::Running
    } else {
        RunState::NotRunning
    }
}

impl GameProbe for SystemProbe {
    fn game(&self) -> RunState {
        scan(matches_game)
    }

    fn launcher(&self) -> RunState {
        scan(matches_launcher)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn matches_game_on_windows_name() {
        assert!(matches_game("StarCitizen.exe", &[]));
    }

    #[test]
    fn matches_game_in_wine_cmdline() {
        let cmdline = args(&[
            "C:\\Program Files\\Roberts Space Industries\\StarCitizen\\LIVE\\Bin64\\StarCitizen.exe",
        ]);
        assert!(matches_game("wine64-preloader", &cmdline));
    }

    #[test]
    fn launcher_is_not_the_game() {
        assert!(!matches_game("RSI Launcher.exe", &[]));
        assert!(matches_launcher("RSI Launcher.exe", &[]));
    }

    #[test]
    fn unrelated_process_matches_nothing() {
        let cmdline = args(&["--help"]);
        assert!(!matches_game("star-config", &cmdline));
        assert!(!matches_launcher("star-config", &cmdline));
    }
}
