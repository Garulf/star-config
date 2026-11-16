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

// The demo story ----------------------------------------------------------------
//
// One player's install, told the same way on every screen: LIVE on 4.3.2, PTU on 4.4.0,
// StarStrings installed before the last patch, a few weeks of snapshots and captures.

pub const LIVE_VERSION: &str = "4.3.2";
pub const LIVE_CHANGE: &str = "9876543";
pub const PTU_VERSION: &str = "4.4.0-PTU";
pub const PTU_CHANGE: &str = "9912345";
/// The LIVE build StarStrings was installed on, before the game updated.
pub const EARLIER_LIVE_CHANGE: &str = "9812345";
pub const DEMO_ACTIONMAPS: &str = include_str!("../tests/fixtures/keybinds/actionmaps.xml");
pub const DEMO_RELEASE: &str = "SC LIVE Build (release-2026-10-28-91d0a4e)";
pub const DEMO_RELEASE_DATE: &str = "2026-10-28T17:31:51Z";
pub const SHADER_CURRENT: &str = "sc-alpha-4.3.2";
pub const SHADER_OLDER: &str = "sc-alpha-4.2.1";
const KB: u64 = 1024;
const MB: u64 = 1024 * KB;
const GB: u64 = 1024 * MB;

pub fn idle_probe() -> std::sync::Arc<sc_core::process::FixedProbe> {
    std::sync::Arc::new(sc_core::process::FixedProbe {
        game: sc_core::process::RunState::NotRunning,
        launcher: sc_core::process::RunState::NotRunning,
    })
}

pub fn channel_named(ctx: &Context, name: &str) -> sc_core::install::Channel {
    ctx.channels
        .iter()
        .find(|c| c.name == name)
        .unwrap()
        .clone()
}

/// Writes the channel's `build_manifest.id`.
pub fn write_build(channel: &sc_core::install::Channel, version: &str, change: &str) {
    fs::write(
        channel.dir.join("build_manifest.id"),
        format!(r#"{{"Data":{{"Version":"{version}","RequestedP4ChangeNum":"{change}"}}}}"#),
    )
    .unwrap();
}

/// A file of `len` bytes that takes no space.
pub fn sparse(path: &Path, len: u64) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::File::create(path).unwrap().set_len(len).unwrap();
}

/// The demo prefix with LIVE and PTU, a launcher, an idle game and both builds, but no
/// player files yet.
pub fn demo_base(temp: &Path) -> Context {
    let mut ctx = context_with_wine_script(temp).with_probe(idle_probe());
    ctx.settings.screenshots.archive_dir = Some(home(temp).join("Pictures/Star Citizen"));
    write_build(&channel_named(&ctx, "LIVE"), LIVE_VERSION, LIVE_CHANGE);
    write_build(&channel_named(&ctx, "PTU"), PTU_VERSION, PTU_CHANGE);
    ctx
}

/// The player's own files in LIVE: user.cfg, attributes.xml and keybinds.
pub fn demo_player_files(ctx: &Context) {
    let live = channel_named(ctx, "LIVE");
    write_channel_files(
        &live,
        Some(DEMO_USER_CFG.as_bytes()),
        Some(demo_attributes().as_bytes()),
    );
    let maps = live.actionmaps_xml();
    fs::create_dir_all(maps.parent().unwrap()).unwrap();
    fs::write(maps, DEMO_ACTIONMAPS).unwrap();
}

/// Everything at once: the whole story, ready for any screen.
pub fn demo_story(temp: &Path) -> Context {
    let ctx = demo_base(temp);
    demo_player_files(&ctx);
    install_starstrings(&ctx, EARLIER_LIVE_CHANGE);
    demo_backups(&ctx);
    demo_shaders(temp);
    demo_shader_backup(&ctx);
    demo_screenshots(&ctx);
    demo_game_files(&ctx);
    ctx
}

// StarStrings --------------------------------------------------------------------

struct ServedZip(Vec<u8>);

impl sc_core::starstrings::Http for ServedZip {
    fn get_text(&self, _url: &str) -> sc_core::error::Result<Option<String>> {
        Ok(None)
    }

    fn get_bytes(&self, _url: &str, _max_bytes: u64) -> sc_core::error::Result<Vec<u8>> {
        Ok(self.0.clone())
    }
}

pub fn demo_release() -> sc_core::starstrings::Release {
    sc_core::starstrings::Release {
        name: DEMO_RELEASE.into(),
        published_at: DEMO_RELEASE_DATE.into(),
        asset_url:
            "https://github.com/MrKraken/StarStrings/releases/download/latest/StarStrings-LIVE.zip"
                .into(),
        asset_size: 2_516_582,
        sha256: None,
    }
}

/// Installs the demo StarStrings release into LIVE while LIVE reports build `change`,
/// then puts the current build back.
pub fn install_starstrings(ctx: &Context, change: &str) {
    let live = channel_named(ctx, "LIVE");
    write_build(&live, LIVE_VERSION, change);
    let mut ini = b"\xef\xbb\xbf".to_vec();
    for i in 0..200 {
        ini.extend(format!("item_Name{i:06}=S3 Bulwark Shield {i}\n").bytes());
    }
    let zip = stored_zip(&[("Data/Localization/english/global.ini", &ini)]);
    let writer = sc_core::guard::Writer::new(
        sc_core::backup::BackupStore::new(ctx.dirs.backups()),
        idle_probe(),
        false,
    );
    let scope = sc_core::backup::Scope {
        install_key: ctx.install.as_ref().unwrap().key(),
        channel: live.name.clone(),
    };
    let info = sc_core::starstrings::install(
        &ServedZip(zip),
        &demo_release(),
        &writer,
        &scope,
        &live,
        &ctx.dirs,
        1,
    )
    .unwrap();
    write_build(&live, LIVE_VERSION, LIVE_CHANGE);
    let installed = local_unix(9, 22, 9);
    set_snapshot_time(ctx, &info, installed);
    let record = ctx
        .dirs
        .data
        .join("starstrings")
        .join(format!("{}-{}.toml", scope.install_key, scope.channel));
    rewrite_line(&record, "installed_unix = ", &installed.to_string());
}

/// Replaces the value of the line starting with `prefix` in a small text file.
fn rewrite_line(path: &Path, prefix: &str, value: &str) {
    let text = fs::read_to_string(path).unwrap();
    let rewritten: Vec<String> = text
        .lines()
        .map(|line| {
            if line.starts_with(prefix) {
                format!("{prefix}{value}")
            } else {
                line.to_string()
            }
        })
        .collect();
    fs::write(path, rewritten.join("\n") + "\n").unwrap();
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// A zip with uncompressed entries, enough for the core's reader.
pub fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let header = |sig: u32, central_entry: bool| {
            let mut h = Vec::new();
            h.extend(sig.to_le_bytes());
            if central_entry {
                h.extend(20u16.to_le_bytes());
            }
            for value in [20u16, 0, 0, 0, 0x21] {
                h.extend(value.to_le_bytes());
            }
            h.extend(crc.to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((name.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes());
            if central_entry {
                for value in [0u16, 0, 0] {
                    h.extend(value.to_le_bytes());
                }
                h.extend(0u32.to_le_bytes());
                h.extend(offset.to_le_bytes());
            }
            h.extend(name.as_bytes());
            h
        };
        out.extend(header(0x0403_4b50, false));
        out.extend(*data);
        central.extend(header(0x0201_4b50, true));
    }
    let central_offset = out.len() as u32;
    let central_size = central.len() as u32;
    out.extend(central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    for value in [0u16, 0, entries.len() as u16, entries.len() as u16] {
        out.extend(value.to_le_bytes());
    }
    out.extend(central_size.to_le_bytes());
    out.extend(central_offset.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

// Backups ------------------------------------------------------------------------

/// Moves a snapshot's time to `unix` by rewriting its manifest.
pub fn set_snapshot_time(ctx: &Context, info: &sc_core::backup::SnapshotInfo, unix: i64) {
    let scope = ctx.scope().unwrap();
    let manifest = ctx
        .dirs
        .backups()
        .join(&scope.install_key)
        .join(&scope.channel)
        .join(&info.id.0)
        .join("manifest.toml");
    rewrite_line(&manifest, "created_unix = ", &unix.to_string());
}

/// Snapshots over the last two weeks of LIVE, the newest one at 21:02 today when that has
/// passed, else an hour ago.
pub fn demo_backups(ctx: &Context) {
    let live = channel_named(ctx, "LIVE");
    let scope = ctx.scope().unwrap();
    let store = sc_core::backup::BackupStore::new(ctx.dirs.backups());
    let now = chrono::Local::now();
    let today_at = now
        .date_naive()
        .and_hms_opt(21, 2, 0)
        .and_then(|at| at.and_local_timezone(chrono::Local).single())
        .filter(|at| *at < now)
        .unwrap_or(now - chrono::Duration::hours(1))
        .timestamp();
    let day = 86_400;
    let snapshots: [(Vec<PathBuf>, &str, i64); 4] = [
        (live.managed_files(), "Manual snapshot", today_at - 13 * day),
        (
            vec![live.attributes_xml()],
            "Head tracking and comms changes",
            today_at - 6 * day - 4_000,
        ),
        (
            vec![live.actionmaps_xml()],
            "Keybinds changes",
            today_at - day - 2_000,
        ),
        (
            vec![live.user_cfg(), live.attributes_xml()],
            "Config changes",
            today_at - 3 * 3_600,
        ),
    ];
    for (files, label, unix) in snapshots {
        let info = store.snapshot(&scope, &live, &files, label).unwrap();
        set_snapshot_time(ctx, &info, unix);
    }
    let current = fs::read(live.user_cfg()).ok();
    fs::write(live.user_cfg(), DEMO_USER_CFG_BEFORE).unwrap();
    let info = store
        .snapshot(&scope, &live, &[live.user_cfg()], "Config changes")
        .unwrap();
    set_snapshot_time(ctx, &info, today_at);
    match current {
        Some(bytes) => fs::write(live.user_cfg(), bytes).unwrap(),
        None => fs::remove_file(live.user_cfg()).unwrap(),
    }
}

/// user.cfg as it was before tonight's changes: a lower FOV and a frame cap.
pub const DEMO_USER_CFG_BEFORE: &str = "\
; my tweaks, FOV 90 feels right in the Gladius
r_DisplayInfo = 1
cl_fov = 80
g_language = english
g_headtrackingToggleEnabled = 1
g_headtrackingGlobalSmoothingRotation = 0.4
r_MotionBlur = 0
r_filmgrain = 1
pl_pit.forceSoftwareCursor = 1
sys_MaxFPS = 60
";

/// The big game files of LIVE and PTU, as sparse files of their real size.
pub fn demo_game_files(ctx: &Context) {
    for (name, p4k) in [("LIVE", 96 * GB + 410 * MB), ("PTU", 92 * GB + 120 * MB)] {
        let dir = channel_named(ctx, name).dir;
        sparse(&dir.join("Data.p4k"), p4k);
        sparse(&dir.join("Bin64/StarCitizen.exe"), 180 * MB);
    }
}

// Shader caches ------------------------------------------------------------------

pub fn shader_cache_dir(temp: &Path) -> PathBuf {
    prefix(temp).join("drive_c/users/steamuser/AppData/Local/Star Citizen")
}

fn age(dir: &Path, days: u64) {
    let when = std::time::SystemTime::now() - std::time::Duration::from_secs(days * 86_400);
    fs::File::open(dir).unwrap().set_modified(when).unwrap();
}

/// Backs up and clears the oldest demo cache, so Maintenance lists one shader backup.
pub fn demo_shader_backup(ctx: &Context) {
    let install = ctx.install.clone().unwrap();
    let roots = sc_core::shaders::appdata_roots(&install, None);
    let old = sc_core::shaders::discover(&install, &roots)
        .into_iter()
        .find(|cache| cache.version.as_deref() == Some("sc-alpha-4.1.1"))
        .unwrap();
    sc_core::shaders::ShaderBackups::new(ctx.dirs.shader_backups())
        .backup_and_clear(&old, idle_probe().as_ref())
        .unwrap();
}

/// Caches for the current build and two older ones, as sparse files.
pub fn demo_shaders(temp: &Path) {
    let sc = shader_cache_dir(temp);
    sparse(
        &sc.join(SHADER_CURRENT).join("shaders/cache-0.bin"),
        1_650 * MB,
    );
    sparse(
        &sc.join(SHADER_CURRENT)
            .join("vulkanshadercache/pipeline.bin"),
        412 * MB,
    );
    sparse(
        &sc.join(SHADER_OLDER).join("shaders/cache-0.bin"),
        GB + 420 * MB,
    );
    sparse(
        &sc.join(SHADER_OLDER).join("vulkanshadercache/pipeline.bin"),
        380 * MB,
    );
    sparse(
        &sc.join("sc-alpha-4.1.1/shaders/cache-0.bin"),
        GB + 200 * MB,
    );
    for (version, kind, days) in [
        (SHADER_CURRENT, "shaders", 2),
        (SHADER_CURRENT, "vulkanshadercache", 2),
        (SHADER_OLDER, "shaders", 38),
        (SHADER_OLDER, "vulkanshadercache", 41),
    ] {
        age(&sc.join(version).join(kind), days);
    }
}

// Screenshots --------------------------------------------------------------------

/// Where `days_ago` at `hour:minute` local time falls, as Unix seconds.
pub fn local_unix(days_ago: i64, hour: u32, minute: u32) -> i64 {
    let date = chrono::Local::now().date_naive() - chrono::Duration::days(days_ago);
    date.and_hms_opt(hour, minute, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .earliest()
        .unwrap()
        .timestamp()
}

fn set_file_time(path: &Path, unix: i64) {
    let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(unix as u64);
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(when)
        .unwrap();
}

/// (channel or "Archive", days ago, hour, minute) for each demo capture, newest first.
pub const DEMO_SHOTS: [(&str, i64, u32, u32); 14] = [
    ("LIVE", 0, 21, 14),
    ("LIVE", 0, 20, 58),
    ("PTU", 0, 20, 31),
    ("LIVE", 0, 20, 12),
    ("LIVE", 0, 19, 47),
    ("PTU", 0, 19, 3),
    ("LIVE", 1, 23, 10),
    ("LIVE", 1, 22, 47),
    ("Archive", 1, 22, 45),
    ("PTU", 1, 22, 31),
    ("LIVE", 1, 19, 2),
    ("Archive", 1, 18, 58),
    ("LIVE", 1, 18, 40),
    ("LIVE", 1, 18, 12),
];

/// Fourteen captures over two days in LIVE, PTU and the archive. Captures dated later
/// than now are moved back a day so the story never shows the future.
pub fn demo_screenshots(ctx: &Context) -> Vec<PathBuf> {
    let archive = ctx.settings.screenshots.archive_dir.clone().unwrap();
    let now = chrono::Local::now().timestamp();
    DEMO_SHOTS
        .iter()
        .enumerate()
        .map(|(index, (place, days_ago, hour, minute))| {
            let dir = match *place {
                "Archive" => archive.clone(),
                name => channel_named(ctx, name).screenshots_dir(),
            };
            let mut unix = local_unix(*days_ago, *hour, *minute);
            if unix > now {
                unix -= 2 * 86_400;
            }
            let stamp = chrono::DateTime::from_timestamp(unix, 0)
                .unwrap()
                .with_timezone(&chrono::Local);
            let path = dir.join(format!(
                "ScreenShot-{}.jpg",
                stamp.format("%Y-%m-%d_%H-%M-%S")
            ));
            let (width, height) = if index == 0 {
                (1920, 1080)
            } else {
                (1280, 720)
            };
            space_scene(&path, width, height, 11 + index as u64);
            set_file_time(&path, unix);
            path
        })
        .collect()
}
