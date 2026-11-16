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

/// A user.cfg as a player keeps it: a few tweaks, one comment, and lines the catalogue
/// does not know.
pub const DEMO_USER_CFG: &str = "\
; my tweaks, FOV 90 feels right in the Gladius
r_DisplayInfo = 1
cl_fov = 90
g_language = english
g_headtrackingToggleEnabled = 1
g_headtrackingGlobalSmoothingRotation = 0.4
r_MotionBlur = 0
r_filmgrain = 0
pl_pit.forceSoftwareCursor = 1
sys_MaxFPS = 144
";

const DEMO_HEAD_TRACKING: [(&str, &str); 18] = [
    ("HeadtrackingSource", "1"),
    ("HeadtrackingToggle", "1"),
    ("HeadtrackingToggleAutoCalibrate", "1"),
    ("HeadtrackingGlobalSmoothingPosition", "0.25"),
    ("HeadtrackingGlobalSmoothingRotation", "0.4"),
    ("HeadtrackingEnableSeatPositionOffset", "1"),
    ("HeadtrackingThirdPersonCameraToggle", "0"),
    ("HeadtrackingThirdPersonDisableDuringInventory", "1"),
    ("HeadtrackingDisableDuringSeat", "0"),
    ("HeadtrackingDisableDuringVehicleAds", "1"),
    ("HeadtrackingDisableDuringWalking", "1"),
    ("HeadtrackingDisableDuringADS", "1"),
    ("HeadtrackingDisableDuringMobiGlas", "1"),
    ("HeadtrackingDisableDuringIM", "0"),
    ("HeadtrackingEnableRollSeat", "1"),
    ("HeadtrackingEnableRollFPS", "0"),
    ("HeadtrackingHoldUseRawDuringHold", "0"),
    ("HeadtrackingInactivityTime", "2.0"),
];

const DEMO_GENERAL: [(&str, &str); 44] = [
    ("AudioDialogueVolume", "1"),
    ("AudioEffectsVolume", "0.9"),
    ("AudioMasterVolume", "0.8"),
    ("AudioMusicVolume", "0.35"),
    ("AutoZoomOnSelectedTarget", "1"),
    ("AutoZoomOnSelectedTargetStrength", "0.5"),
    ("Brightness", "0.5"),
    ("CameraSpringMovement", "0.5"),
    ("ChromaticAberration", "0"),
    ("Contrast", "0.5"),
    ("FOV", "90"),
    ("FacewareCameraIndex", "0"),
    ("FilmGrain", "0"),
    ("FoipEnabled", "1"),
    ("GForceHeadBobScale", "0.3"),
    ("Gamma", "1"),
    ("GraphicsRenderer", "1"),
    ("HmdCursorSensitivity", "2"),
    ("HudColor", "0"),
    ("HudOpacity", "1"),
    ("LensFlare", "0"),
    ("LookAheadEnabled", "0"),
    ("MicSensitivity", "0.6"),
    ("MotionBlur", "0"),
    ("ObjectDetail", "3"),
    ("ParticleQuality", "2"),
    ("PlanetDetail", "3"),
    ("PostProcessQuality", "2"),
    ("Resolution", "2560x1440"),
    ("ShaderQuality", "3"),
    ("ShadowQuality", "2"),
    ("ShakeScale", "0.3"),
    ("Sharpening", "0.5"),
    ("SysSpec", "3"),
    ("TextureQuality", "2"),
    ("Upscaling", "1"),
    ("UpscalingQuality", "2"),
    ("VSync", "0"),
    ("VoipInputVolume", "0.85"),
    ("VoipOutputVolume", "0.7"),
    ("VoipPushToTalk", "1"),
    ("VolumetricCloudQuality", "2"),
    ("WaterQuality", "2"),
    ("WindowMode", "2"),
];

/// An attributes.xml with a few hundred settings, every head tracking name from the
/// catalogue among them, in the layout the game writes.
pub fn demo_attributes() -> String {
    let devices = ["Gamepad", "Joystick", "Mouse"];
    let axes = [
        "AimPitch",
        "AimYaw",
        "LookPitch",
        "LookYaw",
        "Pitch",
        "Roll",
        "StrafeForward",
        "StrafeLateral",
        "StrafeVertical",
        "Throttle",
        "Yaw",
    ];
    let tuning = [
        ("Deadzone", "0.05"),
        ("Exponent", "1"),
        ("Invert", "0"),
        ("Saturation", "1"),
        ("Sensitivity", "1"),
    ];
    let mut entries: Vec<(String, String)> = DEMO_GENERAL
        .iter()
        .chain(&DEMO_HEAD_TRACKING)
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    for device in devices {
        for axis in axes {
            for (setting, value) in tuning {
                entries.push((format!("{device}{axis}{setting}"), value.to_string()));
            }
        }
    }
    entries.sort();
    attributes_xml(&entries)
}

/// The text of an attributes.xml holding `entries` in order.
pub fn attributes_xml(entries: &[(String, String)]) -> String {
    let mut xml = String::from("<Attributes Version=\"1\">\n");
    for (name, value) in entries {
        xml.push_str(&format!(" <Attr name=\"{name}\" value=\"{value}\"/>\n"));
    }
    xml.push_str("</Attributes>\n");
    xml
}

/// Writes `user_cfg` and `attributes` into the channel's usual places when given.
pub fn write_channel_files(
    channel: &sc_core::install::Channel,
    user_cfg: Option<&[u8]>,
    attributes: Option<&[u8]>,
) {
    if let Some(bytes) = user_cfg {
        fs::write(channel.user_cfg(), bytes).unwrap();
    }
    if let Some(bytes) = attributes {
        let path = channel.attributes_xml();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
}
