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

/// Colours of one made-up sky: top and bottom of the gradient, nebula glow, the planet's
/// two band colours and the light that hits it.
struct Look {
    top: [f32; 3],
    bottom: [f32; 3],
    glow: [f32; 3],
    body: [f32; 3],
    band: [f32; 3],
    light: [f32; 3],
}

const LOOKS: [Look; 4] = [
    Look {
        top: [3.0, 6.0, 18.0],
        bottom: [10.0, 22.0, 54.0],
        glow: [40.0, 80.0, 170.0],
        body: [70.0, 120.0, 200.0],
        band: [28.0, 58.0, 120.0],
        light: [205.0, 225.0, 255.0],
    },
    Look {
        top: [8.0, 4.0, 4.0],
        bottom: [42.0, 16.0, 8.0],
        glow: [160.0, 64.0, 22.0],
        body: [196.0, 104.0, 52.0],
        band: [118.0, 48.0, 24.0],
        light: [255.0, 214.0, 172.0],
    },
    Look {
        top: [2.0, 9.0, 11.0],
        bottom: [6.0, 32.0, 36.0],
        glow: [22.0, 120.0, 120.0],
        body: [70.0, 168.0, 156.0],
        band: [24.0, 88.0, 92.0],
        light: [205.0, 255.0, 245.0],
    },
    Look {
        top: [7.0, 4.0, 16.0],
        bottom: [30.0, 12.0, 54.0],
        glow: [112.0, 52.0, 176.0],
        body: [158.0, 108.0, 206.0],
        band: [78.0, 44.0, 130.0],
        light: [236.0, 216.0, 255.0],
    },
];

struct Lcg(u64);

impl Lcg {
    fn unit(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn between(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }
}

struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<[f32; 3]>,
}

impl Canvas {
    fn add(&mut self, x: i64, y: i64, color: [f32; 3], amount: f32) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let pixel = &mut self.pixels[y as usize * self.width + x as usize];
        for (channel, value) in pixel.iter_mut().zip(color) {
            *channel += value * amount;
        }
    }

    fn sky(&mut self, look: &Look, rng: &mut Lcg) {
        let (w, h) = (self.width as f32, self.height as f32);
        let (glow_x, glow_y) = (rng.between(0.1, 0.9) * w, rng.between(0.1, 0.5) * h);
        let spread = 2.0 * (rng.between(0.25, 0.45) * w).powi(2);
        for y in 0..self.height {
            let t = (y as f32 / h).powf(1.2);
            for x in 0..self.width {
                let distance = (x as f32 - glow_x).powi(2) + (y as f32 - glow_y).powi(2);
                let glow = 0.35 * (-distance / spread).exp();
                self.pixels[y * self.width + x] = std::array::from_fn(|c| {
                    look.top[c] + (look.bottom[c] - look.top[c]) * t + look.glow[c] * glow
                });
            }
        }
    }

    fn stars(&mut self, rng: &mut Lcg) {
        let count = 300 + (rng.unit() * 300.0) as usize;
        for _ in 0..count {
            let x = (rng.unit() * self.width as f32) as i64;
            let y = (rng.unit() * self.height as f32) as i64;
            let brightness = rng.unit().powi(3);
            let tint = rng.unit();
            let color = [235.0 + 20.0 * tint, 235.0, 255.0 - 30.0 * tint];
            self.add(x, y, color, 0.25 + 0.75 * brightness);
            if brightness > 0.6 {
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    self.add(x + dx, y + dy, color, 0.35 * brightness);
                }
            }
        }
    }

    /// A shaded disc lit from `light`, with bands, a bright limb and a thin atmosphere.
    fn planet(
        &mut self,
        look: &Look,
        centre: (f32, f32),
        radius: f32,
        light: [f32; 3],
        phase: f32,
    ) {
        let halo = radius * 1.06;
        let x0 = (centre.0 - halo).floor().max(0.0) as usize;
        let x1 = ((centre.0 + halo).ceil() as usize).min(self.width);
        let y0 = (centre.1 - halo).floor().max(0.0) as usize;
        let y1 = ((centre.1 + halo).ceil() as usize).min(self.height);
        for y in y0..y1 {
            for x in x0..x1 {
                let (dx, dy) = (
                    (x as f32 - centre.0) / radius,
                    (y as f32 - centre.1) / radius,
                );
                let d = (dx * dx + dy * dy).sqrt();
                let facing = (dx * light[0] + dy * light[1]) / d.max(1e-3);
                let pixel = &mut self.pixels[y * self.width + x];
                if d >= 1.0 {
                    let air = (1.0 - (d - 1.0) / 0.06).max(0.0).powi(2) * facing.max(0.0) * 0.5;
                    for (value, light) in pixel.iter_mut().zip(look.light) {
                        *value += light * air * 0.6;
                    }
                    continue;
                }
                let nz = (1.0 - d * d).sqrt();
                let lit = (dx * light[0] + dy * light[1] + nz * light[2]).max(0.0);
                let bands = 0.5 + 0.5 * (dy * 9.0 + (dx * 3.0 + phase).sin() * 0.8).sin();
                let limb = (1.0 - nz).powi(3) * lit;
                let edge = ((1.0 - d) * radius).clamp(0.0, 1.0);
                let old = *pixel;
                *pixel = std::array::from_fn(|c| {
                    let base = look.body[c] + (look.band[c] - look.body[c]) * bands;
                    let glare = limb * 0.7 + lit.powi(12) * 0.12;
                    let shaded = base * (0.03 + 0.97 * lit) + look.light[c] * glare;
                    old[c] * (1.0 - edge) + shaded * edge
                });
            }
        }
    }

    /// Saves with a little film grain at high quality, so files weigh what real captures do.
    fn save(self, path: &Path, rng: &mut Lcg) {
        let bytes: Vec<u8> = self
            .pixels
            .iter()
            .flat_map(|pixel| {
                let grain = rng.between(-5.0, 5.0);
                pixel.map(|value| (value + grain).clamp(0.0, 255.0) as u8)
            })
            .collect();
        let picture =
            image::RgbImage::from_raw(self.width as u32, self.height as u32, bytes).unwrap();
        let file = fs::File::create(path).unwrap();
        image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 92)
            .encode_image(&picture)
            .unwrap();
    }
}

/// Writes a made-up space scene as a JPEG: a dark sky with a soft glow, a few hundred
/// stars, and a lit planet or a horizon, with a palette picked by `seed`. The same seed
/// always gives the same picture.
pub fn space_scene(path: &Path, width: u32, height: u32, seed: u64) {
    let look = &LOOKS[(seed % LOOKS.len() as u64) as usize];
    let mut rng = Lcg(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut canvas = Canvas {
        width: width as usize,
        height: height as usize,
        pixels: vec![[0.0; 3]; width as usize * height as usize],
    };
    canvas.sky(look, &mut rng);
    canvas.stars(&mut rng);
    let (w, h) = (width as f32, height as f32);
    let from_left = rng.unit() < 0.5;
    let light_x = if from_left { -0.62 } else { 0.62 };
    let light = [light_x, -0.45, 0.64];
    let phase = rng.between(0.0, 6.0);
    if seed % 3 == 2 {
        let radius = w * 1.5;
        let centre = (w * rng.between(0.35, 0.65), h + radius * 0.84);
        canvas.planet(look, centre, radius, light, phase);
    } else {
        let side = if from_left {
            rng.between(0.58, 0.8)
        } else {
            rng.between(0.2, 0.42)
        };
        let radius = h * rng.between(0.22, 0.4);
        canvas.planet(
            look,
            (w * side, h * rng.between(0.5, 0.72)),
            radius,
            light,
            phase,
        );
    }
    if rng.unit() < 0.6 {
        let moon = h * rng.between(0.03, 0.06);
        let centre = (w * rng.between(0.1, 0.9), h * rng.between(0.12, 0.3));
        canvas.planet(
            &LOOKS[(seed as usize + 2) % LOOKS.len()],
            centre,
            moon,
            light,
            phase,
        );
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).unwrap();
    }
    canvas.save(path, &mut rng);
}
