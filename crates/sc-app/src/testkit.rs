//! Helpers shared by unit tests and the headless tests in `tests/`.
//! Included from both places, so it names the crate as `sc_app`.
#![allow(dead_code)]

use iced::futures::StreamExt;
use iced::keyboard::{self, Key, Location, Modifiers, key};
use iced::{Task, Theme};
use iced_test::runtime;
use iced_test::simulator::Simulator;
use sc_app::app::{App, Message};
use sc_app::context::Context;
use sc_core::dirs::AppDirs;
use sc_core::install::Install;
use sc_core::settings::Settings;
use std::fs;
use std::path::{Path, PathBuf};

/// Creates `<base>/StarCitizen` with the given channel folders and returns its path.
pub fn make_install(base: &Path, channels: &[&str]) -> PathBuf {
    let root = base.join("StarCitizen");
    fs::create_dir_all(&root).unwrap();
    for name in channels {
        fs::create_dir_all(root.join(name).join("USER")).unwrap();
    }
    root
}

/// The fake home folder every test context shows as `~`.
pub fn home(temp: &Path) -> PathBuf {
    temp.join("home")
}

/// The demo Wine prefix, shown as `~/Games/star-citizen`.
pub fn prefix(temp: &Path) -> PathBuf {
    home(temp).join("Games").join("star-citizen")
}

/// Creates the demo prefix with the given channels and returns its StarCitizen folder.
pub fn make_prefix_install(temp: &Path, channels: &[&str]) -> PathBuf {
    let rsi = prefix(temp).join("drive_c/Program Files/Roberts Space Industries");
    make_install(&rsi, channels)
}

/// A context over `installs` whose app folders live under `<temp>/app` and whose
/// home folder is [`home`]. Rediscovery only looks at `settings.installs`.
pub fn context_for(temp: &Path, installs: Vec<Install>) -> Context {
    Context::new(
        AppDirs::under(&temp.join("app")),
        Settings::default(),
        installs,
    )
    .with_home(home(temp))
}

/// The demo prefix with the given channels and no launcher.
pub fn context_with_channels(temp: &Path, channels: &[&str]) -> Context {
    let root = make_prefix_install(temp, channels);
    context_for(temp, vec![Install::from_dir(&root).unwrap()])
}

pub fn context_without_install(temp: &Path) -> Context {
    context_for(temp, Vec::new())
}

pub fn context_without_channels(temp: &Path) -> Context {
    context_with_channels(temp, &[])
}

/// The demo prefix with LIVE and PTU and an `sc-launch.sh` script.
pub fn context_with_wine_script(temp: &Path) -> Context {
    let root = make_prefix_install(temp, &["LIVE", "PTU"]);
    fs::write(prefix(temp).join("sc-launch.sh"), "#!/bin/sh\n").unwrap();
    context_for(temp, vec![Install::from_dir(&root).unwrap()])
}

/// A Ctrl+`character` key press as the keyboard subscription sees it.
pub fn ctrl(character: &str) -> keyboard::Event {
    let code = match character {
        "0" => key::Code::Digit0,
        "1" => key::Code::Digit1,
        "2" => key::Code::Digit2,
        "3" => key::Code::Digit3,
        "4" => key::Code::Digit4,
        "5" => key::Code::Digit5,
        "6" => key::Code::Digit6,
        "7" => key::Code::Digit7,
        "8" => key::Code::Digit8,
        "9" => key::Code::Digit9,
        "s" => key::Code::KeyS,
        "z" => key::Code::KeyZ,
        _ => key::Code::KeyF,
    };
    keyboard::Event::KeyPressed {
        key: Key::Character(character.into()),
        modified_key: Key::Character(character.into()),
        physical_key: key::Physical::Code(code),
        location: Location::Standard,
        modifiers: Modifiers::CTRL,
        text: None,
        repeat: false,
    }
}

/// Runs `task` and every task its messages produce until nothing is left, feeding each
/// message to `app.update`. Widget operations and other runtime actions are dropped.
pub fn settle(app: &mut App, task: Task<Message>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let mut queue = vec![task];
    while let Some(task) = queue.pop() {
        let Some(stream) = runtime::task::into_stream(task) else {
            continue;
        };
        let actions: Vec<_> = runtime.block_on(stream.collect());
        for action in actions {
            if let runtime::Action::Output(message) = action {
                queue.push(app.update(message));
            }
        }
    }
}

/// Feeds the messages a simulator collected to `app`, settling each resulting task.
pub fn feed(app: &mut App, messages: impl IntoIterator<Item = Message>) {
    for message in messages {
        let task = app.update(message);
        settle(app, task);
    }
}

pub fn snapshot_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/snapshots")
}

/// Writes `<target>/snapshots/<name>-tiny-skia.png`, replacing an older one.
pub fn snapshot<M>(ui: &mut Simulator<'_, M>, theme: &Theme, name: &str) {
    let base = snapshot_dir().join(name);
    let _ = fs::remove_file(snapshot_dir().join(format!("{name}-tiny-skia.png")));
    let _ = fs::remove_file(snapshot_dir().join(format!("{name}-wgpu.png")));
    let shot = ui.snapshot(theme).expect("snapshot renders");
    assert!(shot.matches_image(&base).expect("snapshot written"));
}
