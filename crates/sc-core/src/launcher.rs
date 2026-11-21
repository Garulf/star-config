//! Launcher detection and launching.

use crate::error::{Error, Result};
use crate::install::{Install, InstallKind, canonical_or_raw, resolve_ci};
use crate::prefixes::{LauncherScript, PrefixSources};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchMethod {
    Exe(PathBuf),
    Script(PathBuf),
    Command(String),
}

impl LaunchMethod {
    pub fn describe(&self) -> String {
        match self {
            Self::Exe(path) | Self::Script(path) => path.display().to_string(),
            Self::Command(line) => line.clone(),
        }
    }

    pub fn closes_wine_programs(&self) -> bool {
        matches!(self, Self::Script(_))
    }
}

pub fn detect(install: &Install, saved_command: Option<&str>) -> Option<LaunchMethod> {
    detect_in(install, saved_command, &PrefixSources::from_system())
}

pub fn detect_in(
    install: &Install,
    saved_command: Option<&str>,
    sources: &PrefixSources,
) -> Option<LaunchMethod> {
    if let Some(line) = saved_command.map(str::trim).filter(|line| !line.is_empty()) {
        return Some(LaunchMethod::Command(line.to_string()));
    }
    let found = |base: &Path, rel: &str| resolve_ci(base, rel).filter(|path| path.is_file());
    match &install.kind {
        InstallKind::WinePrefix { prefix } => found(prefix, "sc-launch.sh")
            .or_else(|| nix_citizen_script(prefix, sources.launcher_scripts()))
            .map(LaunchMethod::Script),
        InstallKind::Native => {
            found(&install.rsi_dir(), "RSI Launcher/RSI Launcher.exe").map(LaunchMethod::Exe)
        }
    }
}

/// A script known to start another prefix is never picked.
fn nix_citizen_script(prefix: &Path, scripts: Vec<LauncherScript>) -> Option<PathBuf> {
    let wanted = canonical_or_raw(prefix);
    let starts_this = |script: &LauncherScript| {
        script
            .prefix
            .as_deref()
            .is_some_and(|baked| canonical_or_raw(baked) == wanted)
    };
    let chosen = scripts
        .iter()
        .find(|script| starts_this(script))
        .or_else(|| scripts.iter().find(|script| script.prefix.is_none()))?;
    Some(chosen.path.clone())
}

pub fn split_command(line: &str) -> Result<Vec<String>> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    for ch in line.chars() {
        match (quote, ch) {
            (Some(open), _) if ch == open => quote = None,
            (Some(_), _) => current.push(ch),
            (None, '"' | '\'') => {
                quote = Some(ch);
                started = true;
            }
            (None, _) if ch.is_whitespace() => {
                if started {
                    parts.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (None, _) => {
                current.push(ch);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err(Error::Invalid(format!("unterminated quote in: {line}")));
    }
    if started {
        parts.push(current);
    }
    if parts.is_empty() {
        return Err(Error::Invalid("the launch command is empty".into()));
    }
    Ok(parts)
}

fn in_folder_of(program: &Path) -> Command {
    let mut command = Command::new(program);
    if let Some(dir) = program.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        command.current_dir(dir);
    }
    command
}

pub fn command_for(method: &LaunchMethod) -> Result<Command> {
    let mut command = match method {
        LaunchMethod::Exe(path) | LaunchMethod::Script(path) => in_folder_of(path),
        LaunchMethod::Command(line) => {
            let mut parts = split_command(line)?.into_iter();
            let mut command = Command::new(parts.next().unwrap_or_default());
            command.args(parts);
            command
        }
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(command)
}

#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

pub fn launch(method: &LaunchMethod) -> Result<()> {
    let mut command = command_for(method)?;
    detach(&mut command);
    let program = PathBuf::from(command.get_program());
    let mut child = command.spawn().map_err(|err| Error::io(program, err))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::InstallKind;
    use std::fs;
    use tempfile::tempdir;

    fn wine_install(prefix: &Path) -> Install {
        Install {
            root: prefix.join("drive_c/Program Files/Roberts Space Industries/StarCitizen"),
            kind: InstallKind::WinePrefix {
                prefix: prefix.to_path_buf(),
            },
        }
    }

    fn no_path() -> PrefixSources {
        PrefixSources::default()
    }

    fn path_with(dir: &Path, scripts: &[(&str, &str)]) -> PrefixSources {
        let bin = dir.join("bin");
        fs::create_dir_all(&bin).unwrap();
        for (name, text) in scripts {
            fs::write(bin.join(name), text).unwrap();
        }
        PrefixSources {
            path_dirs: vec![bin],
            ..PrefixSources::default()
        }
    }

    fn baking(location: &Path) -> String {
        format!(
            "export WINEPREFIX=\"$(readlink -f \"{}\")\"\n",
            location.display()
        )
    }

    #[test]
    fn saved_command_wins() {
        let dir = tempdir().unwrap();
        let prefix = dir.path().join("pfx");
        fs::create_dir_all(&prefix).unwrap();
        fs::write(prefix.join("sc-launch.sh"), "").unwrap();
        let method = detect_in(
            &wine_install(&prefix),
            Some("lutris lutris:rungame/sc"),
            &no_path(),
        );
        assert_eq!(
            method,
            Some(LaunchMethod::Command("lutris lutris:rungame/sc".into()))
        );
        assert!(matches!(
            detect_in(&wine_install(&prefix), Some("  "), &no_path()),
            Some(LaunchMethod::Script(_))
        ));
    }

    #[test]
    fn detects_lug_script_in_prefix() {
        let dir = tempdir().unwrap();
        let prefix = dir.path().join("pfx");
        fs::create_dir_all(&prefix).unwrap();
        fs::write(prefix.join("sc-launch.sh"), "").unwrap();
        let method = detect_in(&wine_install(&prefix), None, &no_path()).unwrap();
        assert_eq!(method, LaunchMethod::Script(prefix.join("sc-launch.sh")));
        assert!(method.closes_wine_programs());
        assert_eq!(
            method.describe(),
            prefix.join("sc-launch.sh").display().to_string()
        );
    }

    #[test]
    fn falls_back_to_nix_citizen_script_for_its_prefix() {
        let dir = tempdir().unwrap();
        let prefix = dir.path().join("pfx");
        let other = dir.path().join("other");
        fs::create_dir_all(&prefix).unwrap();
        let sources = path_with(
            dir.path(),
            &[
                ("rsi-launcher", &baking(&other)),
                ("star-citizen", &baking(&prefix)),
            ],
        );
        let method = detect_in(&wine_install(&prefix), None, &sources).unwrap();
        assert_eq!(
            method,
            LaunchMethod::Script(dir.path().join("bin/star-citizen"))
        );
        assert!(method.closes_wine_programs());
    }

    #[test]
    fn nix_citizen_script_for_another_prefix_is_skipped() {
        let dir = tempdir().unwrap();
        let prefix = dir.path().join("pfx");
        fs::create_dir_all(&prefix).unwrap();
        let sources = path_with(
            dir.path(),
            &[("rsi-launcher", &baking(&dir.path().join("other")))],
        );
        assert_eq!(detect_in(&wine_install(&prefix), None, &sources), None);
    }

    #[test]
    fn unread_nix_citizen_script_is_used_when_nothing_matches() {
        let dir = tempdir().unwrap();
        let prefix = dir.path().join("pfx");
        let sources = path_with(dir.path(), &[("rsi-launcher", "#!/bin/sh\n")]);
        assert_eq!(
            detect_in(&wine_install(&prefix), None, &sources),
            Some(LaunchMethod::Script(dir.path().join("bin/rsi-launcher")))
        );
    }

    #[test]
    fn lug_script_beats_nix_citizen_script() {
        let dir = tempdir().unwrap();
        let prefix = dir.path().join("pfx");
        fs::create_dir_all(&prefix).unwrap();
        fs::write(prefix.join("sc-launch.sh"), "").unwrap();
        let sources = path_with(dir.path(), &[("rsi-launcher", &baking(&prefix))]);
        assert_eq!(
            detect_in(&wine_install(&prefix), None, &sources),
            Some(LaunchMethod::Script(prefix.join("sc-launch.sh")))
        );
    }

    #[test]
    fn detects_windows_exe_beside_starcitizen() {
        let dir = tempdir().unwrap();
        let rsi = dir
            .path()
            .join("Zoë Games/Program Files/Roberts Space Industries");
        let exe = rsi.join("RSI Launcher/RSI Launcher.exe");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, "").unwrap();
        let install = Install {
            root: rsi.join("StarCitizen"),
            kind: InstallKind::Native,
        };
        let method = detect_in(&install, None, &no_path()).unwrap();
        assert_eq!(method, LaunchMethod::Exe(exe));
        assert!(!method.closes_wine_programs());
    }

    #[test]
    fn detect_none_when_nothing_found() {
        let dir = tempdir().unwrap();
        let native = Install {
            root: dir.path().join("RSI/StarCitizen"),
            kind: InstallKind::Native,
        };
        assert_eq!(detect_in(&native, None, &no_path()), None);
        assert_eq!(
            detect_in(&wine_install(&dir.path().join("pfx")), None, &no_path()),
            None
        );
    }

    #[test]
    fn split_command_handles_quotes() {
        let parts = split_command(r#"lutris "lutris:rungame/star citizen" --x"#).unwrap();
        assert_eq!(parts, ["lutris", "lutris:rungame/star citizen", "--x"]);
        let single = split_command("run 'a b' c\"d e\"").unwrap();
        assert_eq!(single, ["run", "a b", "cd e"]);
    }

    #[test]
    fn split_command_rejects_empty_and_unterminated() {
        assert!(matches!(split_command("   "), Err(Error::Invalid(_))));
        assert!(matches!(split_command(""), Err(Error::Invalid(_))));
        assert!(matches!(
            split_command("run \"unclosed"),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn launch_missing_program_is_io_error() {
        let err = launch(&LaunchMethod::Command("/no/such/program".into())).unwrap_err();
        assert!(matches!(&err, Error::Io { path, .. } if path == Path::new("/no/such/program")));
    }

    #[cfg(unix)]
    #[test]
    fn launch_runs_detached() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let marker = dir.path().join("ran");
        let script = dir.path().join("sc-launch.sh");
        fs::write(
            &script,
            format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        launch(&LaunchMethod::Script(script)).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !marker.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(marker.exists());
    }
}
