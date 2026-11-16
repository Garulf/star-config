#[path = "../src/testkit.rs"]
mod testkit;

use iced::futures::StreamExt;
use iced::{Size, Task};
use iced_test::runtime;
use iced_test::selector::Candidate;
use iced_test::simulator::Simulator;
use sc_app::app::{self, App};
use sc_app::context::Context;
use sc_app::screen::{Action, Confirm, Screen, Toast};
use sc_app::screens::starstrings::{self, Message, State};
use sc_core::error::{Error, Result};
use sc_core::process::{FixedProbe, RunState};
use sc_core::settings::{Settings, ThemeChoice};
use sc_core::starstrings::{Http, MIN_LINES, RELEASE_API, Release};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const FULL: Size = Size::new(1280.0, 800.0);
const ASSET_URL: &str =
    "https://github.com/MrKraken/StarStrings/releases/download/latest/StarStrings-LIVE.zip";
const OLD_NAME: &str = "SC LIVE Build (release-2026-10-28-91d0a4e)";
const OLD_DATE: &str = "2026-10-28T09:12:40Z";
const NEW_NAME: &str = "SC LIVE Build (release-2026-11-12-2c6c06f)";
const NEW_DATE: &str = "2026-11-12T17:31:51Z";
const OFFLINE: &str = "Could not reach GitHub. Check your connection and try again.";
const BAD_DOWNLOAD: &str =
    "The download did not look like a StarStrings release, so nothing was changed.";
const FOREIGN: &str = "This channel already has a custom global.ini that star-config did not install. Installing replaces it. A snapshot is taken first.";

// A fake GitHub ---------------------------------------------------------------

struct FakeHttp {
    release: Mutex<Option<(String, String)>>,
    zip: Vec<u8>,
    offline_text: bool,
    offline_bytes: bool,
    asked: Mutex<Vec<String>>,
}

impl FakeHttp {
    fn serving(zip: Vec<u8>) -> Self {
        Self {
            release: Mutex::new(Some((NEW_NAME.into(), NEW_DATE.into()))),
            zip,
            offline_text: false,
            offline_bytes: false,
            asked: Mutex::new(Vec::new()),
        }
    }

    fn good() -> Self {
        Self::serving(good_zip())
    }

    fn offline() -> Self {
        Self {
            offline_text: true,
            offline_bytes: true,
            ..Self::good()
        }
    }

    fn set_release(&self, name: &str, date: &str) {
        *self.release.lock().unwrap() = Some((name.into(), date.into()));
    }

    fn checks(&self) -> usize {
        self.asked
            .lock()
            .unwrap()
            .iter()
            .filter(|url| *url == RELEASE_API)
            .count()
    }
}

impl Http for FakeHttp {
    fn get_text(&self, url: &str) -> Result<Option<String>> {
        self.asked.lock().unwrap().push(url.to_string());
        if self.offline_text {
            return Err(Error::Network(
                "io: failed to lookup address information".into(),
            ));
        }
        Ok(self
            .release
            .lock()
            .unwrap()
            .as_ref()
            .map(|(name, date)| release_json(name, date, self.zip.len())))
    }

    fn get_bytes(&self, url: &str, _max_bytes: u64) -> Result<Vec<u8>> {
        self.asked.lock().unwrap().push(url.to_string());
        if self.offline_bytes {
            return Err(Error::Network("connection reset by peer".into()));
        }
        Ok(self.zip.clone())
    }
}

fn release_json(name: &str, date: &str, size: usize) -> String {
    format!(
        r#"{{"name":"{name}","published_at":"{date}","assets":[{{"name":"StarStrings-LIVE.zip","browser_download_url":"{ASSET_URL}","size":{size}}}]}}"#
    )
}

fn release(name: &str, date: &str, size: u64) -> Release {
    Release {
        name: name.into(),
        published_at: date.into(),
        asset_url: ASSET_URL.into(),
        asset_size: size,
        sha256: None,
    }
}

fn ini(lines: usize) -> Vec<u8> {
    let mut out = b"\xef\xbb\xbf".to_vec();
    for i in 0..lines {
        out.extend(format!("item_Name{i:06}=S3 Bulwark Shield {i}\n").bytes());
    }
    out
}

fn good_zip() -> Vec<u8> {
    stored_zip(&[
        ("USER.cfg", b"g_language = english\n"),
        ("Data/Localization/english/global.ini", &ini(MIN_LINES + 10)),
    ])
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
fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
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

// Driving one screen ------------------------------------------------------------

#[derive(Default)]
struct Seen {
    toasts: Vec<Toast>,
    settings: Option<Settings>,
    confirm: Option<Confirm<Message>>,
}

struct Harness {
    _temp: tempfile::TempDir,
    state: State,
    ctx: Context,
    seen: Seen,
}

fn idle() -> Arc<FixedProbe> {
    Arc::new(FixedProbe {
        game: RunState::NotRunning,
        launcher: RunState::NotRunning,
    })
}

impl Harness {
    fn new(channel: &str, http: Arc<dyn Http>) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let ctx = testkit::context_with_channels(temp.path(), &[channel]).with_probe(idle());
        Self {
            _temp: temp,
            state: State::with_http(http),
            ctx,
            seen: Seen::default(),
        }
    }

    fn live(http: &Arc<FakeHttp>) -> Self {
        Self::new("LIVE", http.clone())
    }

    fn dir(&self) -> PathBuf {
        self.ctx.channel.as_ref().unwrap().dir.clone()
    }

    fn global_ini(&self) -> PathBuf {
        self.ctx.channel.as_ref().unwrap().global_ini()
    }

    fn user_cfg(&self) -> PathBuf {
        self.ctx.channel.as_ref().unwrap().user_cfg()
    }

    fn set_build(&self, change: &str) {
        write_build(&self.dir(), change);
    }

    fn fresh_state(&mut self, http: Arc<dyn Http>) {
        self.state = State::with_http(http);
    }

    fn reload(&mut self) {
        let task = self.state.reload(&self.ctx);
        self.run(task);
    }

    fn send(&mut self, message: Message) {
        let action = self.state.update(message, &self.ctx);
        self.take(action);
    }

    fn take(&mut self, action: Action<Message>) {
        let task = self.record(action);
        self.run(task);
    }

    fn record(&mut self, action: Action<Message>) -> Task<Message> {
        self.seen.toasts.extend(action.toast);
        if action.settings.is_some() {
            self.seen.settings = action.settings;
        }
        if action.confirm.is_some() {
            self.seen.confirm = action.confirm;
        }
        action.task
    }

    fn run(&mut self, task: Task<Message>) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let mut queue = vec![task];
        while let Some(task) = queue.pop() {
            let Some(stream) = runtime::task::into_stream(task) else {
                continue;
            };
            let actions: Vec<_> = rt.block_on(stream.collect());
            for action in actions {
                if let runtime::Action::Output(message) = action {
                    let action = self.state.update(message, &self.ctx);
                    queue.push(self.record(action));
                }
            }
        }
    }

    fn sim(&self) -> Simulator<'_, Message> {
        Simulator::with_size(sc_app::settings(), FULL, self.state.view(&self.ctx))
    }

    fn has(&self, text: &str) -> bool {
        self.sim().find(text).is_ok()
    }

    fn has_part(&self, needle: &str) -> bool {
        let needle = needle.to_string();
        self.sim()
            .find(move |candidate: Candidate<'_>| match candidate {
                Candidate::Text { content, .. } if content.contains(needle.as_str()) => {
                    Some(content.to_string())
                }
                _ => None,
            })
            .is_ok()
    }

    /// Clicks `label` and feeds what it produced; returns how many messages that was.
    fn click(&mut self, label: &str) -> usize {
        let messages: Vec<Message> = {
            let mut ui = self.sim();
            ui.click(label)
                .unwrap_or_else(|err| panic!("{label}: {err}"));
            ui.into_messages().collect()
        };
        let count = messages.len();
        for message in messages {
            self.send(message);
        }
        count
    }

    fn accept(&mut self) {
        let confirm = self.seen.confirm.take().expect("a confirm dialog");
        let message = confirm
            .actions
            .into_iter()
            .find_map(|action| action.message)
            .expect("an action that does something");
        self.send(message);
    }

    fn check_and_install(&mut self) {
        self.reload();
        self.click("Check for updates");
        assert_eq!(self.click("Install"), 1);
    }
}

fn write_build(dir: &Path, change: &str) {
    fs::write(
        dir.join("build_manifest.id"),
        format!(r#"{{"Data":{{"Version":"4.3.2","RequestedP4ChangeNum":"{change}"}}}}"#),
    )
    .unwrap();
}

// Tests ---------------------------------------------------------------------

#[test]
fn not_installed_and_unchecked_shows_check_for_updates() {
    let http = Arc::new(FakeHttp::good());
    let mut screen = Harness::live(&http);
    screen.reload();
    assert!(screen.has("Check for updates"));
    assert!(screen.has("Not installed"));
    assert!(screen.has("Not checked yet"));
    assert!(screen.has("Unknown"));
    assert!(!screen.has("Uninstall"));
    assert_eq!(http.checks(), 0);
    assert_eq!(screen.state.pending(), 0);
}

#[test]
fn check_finds_a_release_and_offers_install() {
    let http = Arc::new(FakeHttp::good());
    let mut screen = Harness::live(&http);
    screen.set_build("9812345");
    screen.reload();
    assert_eq!(screen.click("Check for updates"), 1);
    assert_eq!(http.checks(), 1);
    assert!(screen.has("Install"));
    assert!(screen.has(NEW_NAME));
    assert!(screen.has("Check again"));
    assert!(screen.has("4.3.2 (9812345)"));
    assert!(!screen.global_ini().exists());
}

#[test]
fn install_writes_global_ini_and_language_line() {
    let http = Arc::new(FakeHttp::good());
    let mut screen = Harness::live(&http);
    fs::write(screen.user_cfg(), "r_displayInfo = 0\n").unwrap();
    screen.check_and_install();
    let written = fs::read(screen.global_ini()).unwrap();
    assert_eq!(written, ini(MIN_LINES + 10));
    let cfg = fs::read_to_string(screen.user_cfg()).unwrap();
    assert!(cfg.contains("r_displayInfo = 0"), "{cfg}");
    assert!(cfg.contains("g_language = english"), "{cfg}");
    let toast = screen.seen.toasts.last().expect("a toast");
    assert_eq!(toast.text, format!("Installed StarStrings {NEW_NAME}"));
    assert!(toast.undo.is_some());
    assert!(screen.has("Up to date"));
    assert!(screen.has("Uninstall"));
}

#[test]
fn installed_and_current_shows_up_to_date() {
    let http = Arc::new(FakeHttp::good());
    let mut screen = Harness::live(&http);
    screen.check_and_install();
    screen.fresh_state(http.clone());
    screen.reload();
    assert!(screen.has(NEW_NAME));
    assert!(screen.has("Check for updates"));
    screen.click("Check for updates");
    assert!(screen.has("Up to date"));
    assert!(!screen.has("Install"));
    assert!(!screen.has("Update"));
}

#[test]
fn newer_release_offers_update() {
    let http = Arc::new(FakeHttp::good());
    http.set_release(OLD_NAME, OLD_DATE);
    let mut screen = Harness::live(&http);
    screen.check_and_install();
    assert!(screen.has("Up to date"));
    http.set_release(NEW_NAME, NEW_DATE);
    screen.click("Check again");
    assert!(screen.has("Update"));
    assert!(screen.has(NEW_NAME));
    assert!(screen.has(OLD_NAME));
    assert_eq!(screen.click("Update"), 1);
    assert!(screen.has("Up to date"));
    assert!(
        screen
            .seen
            .toasts
            .iter()
            .any(|toast| toast.text == format!("Installed StarStrings {NEW_NAME}"))
    );
}

#[test]
fn build_change_since_install_shows_the_warning() {
    let http = Arc::new(FakeHttp::good());
    let mut screen = Harness::live(&http);
    screen.set_build("9812345");
    screen.check_and_install();
    assert!(!screen.has_part("The game was updated"));
    screen.set_build("9876543");
    screen.reload();
    assert!(screen.has(
        "The game was updated since StarStrings was installed (build 9876543, was 9812345). Update StarStrings so its text matches the game."
    ));
}

#[test]
fn non_live_channel_asks_before_installing() {
    let http = Arc::new(FakeHttp::good());
    let mut screen = Harness::new("PTU", http.clone());
    screen.reload();
    assert!(screen.has("StarStrings is built for LIVE. On PTU some text can be wrong or missing."));
    screen.click("Check for updates");
    assert_eq!(screen.click("Install"), 1);
    let confirm = screen.seen.confirm.as_ref().expect("asks first");
    assert_eq!(confirm.title, "Install StarStrings on PTU?");
    assert!(confirm.body.contains("built for LIVE"));
    assert!(!screen.global_ini().exists());
    screen.accept();
    assert!(screen.global_ini().exists());
}

#[test]
fn foreign_global_ini_shows_the_replace_warning() {
    let http = Arc::new(FakeHttp::good());
    let mut screen = Harness::live(&http);
    let path = screen.global_ini();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, ini(MIN_LINES)).unwrap();
    screen.reload();
    assert!(screen.has(FOREIGN));
    assert!(screen.has("Not installed"));
    assert!(screen.has("A custom global.ini is in place"));
    assert!(!screen.has("Uninstall"));
}

#[test]
fn uninstall_asks_then_removes() {
    let http = Arc::new(FakeHttp::good());
    let mut screen = Harness::live(&http);
    screen.check_and_install();
    assert_eq!(screen.click("Uninstall"), 1);
    let confirm = screen.seen.confirm.as_ref().expect("asks first");
    assert_eq!(confirm.title, "Remove StarStrings from LIVE?");
    assert_eq!(confirm.body, "The game goes back to its own text.");
    assert!(screen.global_ini().exists());
    screen.accept();
    assert!(!screen.global_ini().exists());
    let cfg = fs::read_to_string(screen.user_cfg()).unwrap_or_default();
    assert!(!cfg.contains("g_language"), "{cfg}");
    let toast = screen.seen.toasts.last().unwrap();
    assert_eq!(toast.text, "Removed StarStrings");
    assert!(toast.undo.is_some());
    assert!(screen.has("Not installed"));
    assert!(screen.has("Install"));
}

#[test]
fn network_failure_shows_the_plain_message_and_changes_nothing() {
    let http = Arc::new(FakeHttp::offline());
    let mut screen = Harness::live(&http);
    screen.reload();
    screen.click("Check for updates");
    assert!(screen.has(OFFLINE));
    assert!(screen.has_part("failed to lookup address"));
    assert!(screen.has("Check for updates"));

    let http = Arc::new(FakeHttp {
        offline_bytes: true,
        ..FakeHttp::good()
    });
    screen.fresh_state(http.clone());
    screen.check_and_install();
    assert!(screen.has(OFFLINE));
    assert!(!screen.global_ini().exists());
    assert!(!screen.user_cfg().exists());
    assert!(screen.seen.toasts.is_empty());
    assert!(screen.has("Install"));
}

#[test]
fn bad_download_changes_nothing() {
    let http = Arc::new(FakeHttp::serving(stored_zip(&[(
        "Data/Localization/english/global.ini",
        &ini(120),
    )])));
    let mut screen = Harness::live(&http);
    fs::write(screen.user_cfg(), "r_displayInfo = 0\n").unwrap();
    screen.check_and_install();
    assert!(screen.has(BAD_DOWNLOAD));
    assert!(screen.has_part("fewer than"));
    assert!(!screen.global_ini().exists());
    assert_eq!(
        fs::read_to_string(screen.user_cfg()).unwrap(),
        "r_displayInfo = 0\n"
    );

    let http = Arc::new(FakeHttp::serving(b"<html>rate limited</html>".to_vec()));
    screen.fresh_state(http);
    screen.check_and_install();
    assert!(screen.has(BAD_DOWNLOAD));
    assert!(!screen.global_ini().exists());
}

#[test]
fn actions_disabled_when_read_only() {
    let http = Arc::new(FakeHttp::good());
    let mut screen = Harness::live(&http);
    screen.ctx.game = RunState::Running;
    screen.reload();
    assert_eq!(screen.click("Check for updates"), 1);
    assert!(screen.has("Close the game to install or remove StarStrings."));
    assert_eq!(screen.click("Install"), 0);
    screen.send(Message::Install);
    screen.send(Message::InstallConfirmed);
    assert!(!screen.global_ini().exists());

    screen.ctx.game = RunState::NotRunning;
    assert_eq!(screen.click("Install"), 1);
    assert!(screen.global_ini().exists());
    screen.ctx.game = RunState::Running;
    assert_eq!(screen.click("Uninstall"), 0);
    screen.send(Message::UninstallConfirmed);
    assert!(screen.global_ini().exists());
    assert!(screen.seen.confirm.is_none());
}

#[test]
fn toggle_check_on_start_returns_new_settings() {
    let http = Arc::new(FakeHttp::good());
    let mut screen = Harness::live(&http);
    screen.send(Message::CheckOnStart(true));
    let settings = screen.seen.settings.take().expect("new settings");
    assert!(settings.starstrings.check_on_start);
    screen.ctx.settings = settings;
    screen.send(Message::CheckOnStart(false));
    assert!(
        !screen
            .seen
            .settings
            .take()
            .unwrap()
            .starstrings
            .check_on_start
    );
    assert_eq!(screen.state.pending(), 0);
}

#[test]
fn check_on_start_runs_a_check_on_first_reload() {
    let http = Arc::new(FakeHttp::good());
    let mut screen = Harness::live(&http);
    screen.ctx.settings.starstrings.check_on_start = true;
    screen.reload();
    assert_eq!(http.checks(), 1);
    assert!(screen.has(NEW_NAME));
    assert!(screen.has("Install"));
    screen.reload();
    assert_eq!(http.checks(), 1);

    let quiet = Arc::new(FakeHttp::good());
    let mut screen = Harness::live(&quiet);
    screen.reload();
    assert_eq!(quiet.checks(), 0);
}

// Pictures --------------------------------------------------------------------

fn shot(app: &App, name: &str) {
    let mut ui = Simulator::with_size(sc_app::settings(), FULL, app.view());
    testkit::snapshot(&mut ui, &app.theme(), name);
}

fn installed_app(temp: &Path, choice: ThemeChoice) -> App {
    let mut ctx = testkit::context_with_channels(temp, &["LIVE", "PTU"]).with_probe(idle());
    ctx.settings.theme = choice;
    let channel = ctx.channel.clone().unwrap();
    write_build(&channel.dir, "9812345");
    let zip = good_zip();
    let http = FakeHttp::serving(zip.clone());
    sc_core::starstrings::install(
        &http,
        &release(OLD_NAME, OLD_DATE, zip.len() as u64),
        &ctx.writer(),
        &ctx.scope().unwrap(),
        &channel,
        &ctx.dirs,
        MIN_LINES,
    )
    .unwrap();
    write_build(&channel.dir, "9876543");
    let mut app = App::with(ctx);
    testkit::feed(&mut app, [app::Message::Navigate(Screen::StarStrings)]);
    testkit::feed(
        &mut app,
        [app::Message::StarStrings(starstrings::Message::Checked(
            Ok(Some(release(NEW_NAME, NEW_DATE, 2_516_582))),
        ))],
    );
    app
}

#[test]
fn starstrings_pictures() {
    for (choice, name) in [(ThemeChoice::Dark, "dark"), (ThemeChoice::Light, "light")] {
        let temp = tempfile::tempdir().unwrap();
        let app = installed_app(temp.path(), choice);
        shot(&app, &format!("starstrings-{name}"));
    }

    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::context_with_channels(temp.path(), &["LIVE", "PTU"]).with_probe(idle());
    write_build(&ctx.channel.as_ref().unwrap().dir, "9876543");
    let mut app = App::with(ctx);
    testkit::feed(&mut app, [app::Message::Navigate(Screen::StarStrings)]);
    shot(&app, "starstrings-fresh-dark");
}

#[test]
fn a_channel_switch_during_the_download_installs_nothing() {
    let http = Arc::new(FakeHttp::good());
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::context_with_channels(temp.path(), &["LIVE", "PTU"]).with_probe(idle());
    let mut screen = Harness {
        _temp: temp,
        state: State::with_http(http.clone()),
        ctx,
        seen: Seen::default(),
    };
    let live = screen.ctx.channel.clone().unwrap();
    screen.reload();
    screen.click("Check for updates");
    let action = screen.state.update(Message::Install, &screen.ctx);
    let download = screen.record(action);
    assert!(screen.ctx.set_channel("PTU"));
    screen.reload();
    screen.run(download);

    assert!(!screen.global_ini().exists());
    assert!(!screen.user_cfg().exists());
    assert!(!live.global_ini().exists());
    assert!(!live.user_cfg().exists());
    let toast = screen.seen.toasts.last().expect("a toast");
    assert_eq!(
        toast.text,
        "StarStrings was downloaded for LIVE but you switched channel, so it was not installed."
    );
    assert!(toast.undo_in.is_empty());
}
