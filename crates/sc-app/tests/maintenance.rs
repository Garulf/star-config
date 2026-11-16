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
use sc_app::screens::maintenance::{self, Message, State};
use sc_core::process::{FixedProbe, RunState};
use sc_core::settings::ThemeChoice;
use sc_core::shaders::{self, ShaderBackups};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const FULL: Size = Size::new(1280.0, 800.0);
const CURRENT: &str = "sc-alpha-4.3.2";
const OLDER: &str = "sc-alpha-4.2.1";
const BLOCKED: &str = "Close the game to change shader caches.";
const UNMATCHED_HINT: &str =
    "No installed channel matches this version. It may be left over from an older build.";
const INTERRUPTED: &str = "This backup was interrupted and may be missing files.";

fn idle() -> Arc<FixedProbe> {
    Arc::new(FixedProbe {
        game: RunState::NotRunning,
        launcher: RunState::NotRunning,
    })
}

fn star_citizen_cache_dir(temp: &Path) -> PathBuf {
    testkit::prefix(temp).join("drive_c/users/steamuser/AppData/Local/Star Citizen")
}

/// A file of `len` bytes that takes no space, so caches can look their real size.
fn put(path: &Path, len: u64) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::File::create(path).unwrap().set_len(len).unwrap();
}

fn age(dir: &Path, days: u64) {
    let when = SystemTime::now() - Duration::from_secs(days * 86_400);
    testkit::set_dir_modified(dir, when);
}

fn write_build(dir: &Path) {
    fs::write(
        dir.join("build_manifest.id"),
        r#"{"Data":{"Version":"4.3.2","RequestedP4ChangeNum":"9876543"}}"#,
    )
    .unwrap();
}

#[derive(Default)]
struct Seen {
    toasts: Vec<Toast>,
    confirm: Option<Confirm<Message>>,
}

struct Harness {
    temp: tempfile::TempDir,
    state: State,
    ctx: Context,
    seen: Seen,
}

impl Harness {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let ctx = testkit::context_with_channels(temp.path(), &["LIVE"]).with_probe(idle());
        write_build(&ctx.channel.as_ref().unwrap().dir);
        Self {
            temp,
            state: State::with_local_appdata(None),
            ctx,
            seen: Seen::default(),
        }
    }

    /// Two versions, each with a shaders and a vulkan cache.
    fn with_caches() -> Self {
        let screen = Self::new();
        for version in [CURRENT, OLDER] {
            put(&screen.cache(version, "shaders").join("cache.bin"), 2048);
            put(
                &screen.cache(version, "vulkanshadercache").join("v.bin"),
                1024,
            );
        }
        screen
    }

    fn sc(&self) -> PathBuf {
        star_citizen_cache_dir(self.temp.path())
    }

    fn cache(&self, version: &str, kind: &str) -> PathBuf {
        self.sc().join(version).join(kind)
    }

    fn store(&self) -> ShaderBackups {
        ShaderBackups::new(self.ctx.dirs.shader_backups())
    }

    fn reload(&mut self) {
        let task = self.state.reload(&self.ctx);
        self.run(task);
    }

    fn send(&mut self, message: Message) {
        let action = self.state.update(message, &self.ctx);
        let task = self.record(action);
        self.run(task);
    }

    fn tick(&mut self, path: PathBuf) {
        self.send(Message::Tick(path, true));
    }

    fn record(&mut self, action: Action<Message>) -> Task<Message> {
        self.seen.toasts.extend(action.toast);
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

    fn back_up(&mut self, version: &str, kind: &str) {
        self.tick(self.cache(version, kind));
        assert_eq!(self.click("Back up and clear"), 1);
    }
}

#[test]
fn no_caches_shows_empty_state() {
    let mut screen = Harness::new();
    screen.reload();
    assert!(screen.has("No shader caches found"));
    assert!(screen.has("They appear after the game has run once."));
    assert_eq!(screen.state.pending(), 0);
}

#[test]
fn caches_list_with_kind_size_and_tag() {
    let mut screen = Harness::with_caches();
    screen.reload();
    assert!(screen.has(CURRENT));
    assert!(screen.has(OLDER));
    assert!(screen.has("Shaders"));
    assert!(screen.has("Vulkan"));
    assert!(screen.has("2.0 KB"));
    assert!(screen.has("1.0 KB"));
    assert!(screen.has("Current"));
    assert!(screen.has("Unmatched"));
    assert!(screen.has(UNMATCHED_HINT));
    assert!(screen.has("6.0 KB"));
}

#[test]
fn nothing_is_ticked_by_default_and_actions_are_disabled() {
    let mut screen = Harness::with_caches();
    screen.reload();
    assert!(screen.has("Tick the caches to back up or clear."));
    assert_eq!(screen.click("Clear"), 0);
    assert_eq!(screen.click("Back up and clear"), 0);
    assert!(screen.seen.confirm.is_none());
    assert!(screen.cache(OLDER, "shaders").exists());
}

#[test]
fn clear_asks_naming_count_and_size_then_deletes() {
    let mut screen = Harness::with_caches();
    screen.reload();
    screen.tick(screen.cache(OLDER, "shaders"));
    screen.tick(screen.cache(OLDER, "vulkanshadercache"));
    assert!(screen.has("2 selected, 3.0 KB"));
    assert_eq!(screen.click("Clear"), 1);
    let confirm = screen.seen.confirm.as_ref().expect("asks first");
    assert_eq!(confirm.title, "Clear 2 shader caches (3.0 KB)?");
    let shown = screen.ctx.display_path(&screen.cache(OLDER, "shaders"));
    assert!(shown.starts_with('~'), "{shown}");
    assert!(confirm.body.contains(&shown), "{}", confirm.body);
    assert!(confirm.body.ends_with("This cannot be undone."));
    assert!(screen.cache(OLDER, "shaders").exists());
    screen.accept();
    assert!(!screen.cache(OLDER, "shaders").exists());
    assert!(!screen.cache(OLDER, "vulkanshadercache").exists());
    assert!(screen.cache(CURRENT, "shaders").join("cache.bin").exists());
    assert!(screen.cache(CURRENT, "vulkanshadercache").exists());
    let toast = screen.seen.toasts.last().unwrap();
    assert_eq!(toast.text, "Cleared 2 caches, freed 3.0 KB");
    assert!(toast.undo.is_none());
    assert!(!screen.has(OLDER));
    assert!(screen.has("Tick the caches to back up or clear."));
}

#[test]
fn back_up_and_clear_moves_into_the_store_and_lists_the_backup() {
    let mut screen = Harness::with_caches();
    screen.reload();
    screen.back_up(OLDER, "shaders");
    assert!(!screen.cache(OLDER, "shaders").exists());
    let backups = screen.store().list();
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].version, OLDER);
    assert!(backups[0].path.join("cache.bin").exists());
    let toast = screen.seen.toasts.last().unwrap();
    assert!(
        toast.text.starts_with("Backed up and cleared 1 cache"),
        "{}",
        toast.text
    );
    assert!(toast.undo.is_none());
    assert!(screen.has("BACKUPS (2.0 KB)"));
    assert!(screen.has_part("Shaders, backed up"));
    assert!(screen.has("Restore"));
}

#[test]
fn restore_puts_a_backup_back() {
    let mut screen = Harness::with_caches();
    screen.reload();
    screen.back_up(OLDER, "shaders");
    assert_eq!(screen.click("Restore"), 1);
    assert!(screen.seen.confirm.is_none());
    assert!(screen.cache(OLDER, "shaders").join("cache.bin").exists());
    assert!(screen.store().list().is_empty());
    assert_eq!(
        screen.seen.toasts.last().unwrap().text,
        format!("Restored the {OLDER} shader cache")
    );
}

#[test]
fn restore_confirms_when_a_current_cache_exists() {
    let mut screen = Harness::with_caches();
    screen.reload();
    screen.back_up(OLDER, "shaders");
    put(&screen.cache(OLDER, "shaders").join("rebuilt.bin"), 10);
    screen.reload();
    assert_eq!(screen.click("Restore"), 1);
    let confirm = screen.seen.confirm.as_ref().expect("asks first");
    assert_eq!(
        confirm.body,
        "The current cache for this version is replaced."
    );
    assert!(screen.cache(OLDER, "shaders").join("rebuilt.bin").exists());
    screen.accept();
    assert!(screen.cache(OLDER, "shaders").join("cache.bin").exists());
    assert!(!screen.cache(OLDER, "shaders").join("rebuilt.bin").exists());
    assert!(screen.store().list().is_empty());
}

#[test]
fn delete_backup_asks_then_removes() {
    let mut screen = Harness::with_caches();
    screen.reload();
    screen.back_up(OLDER, "shaders");
    assert_eq!(screen.click("Delete"), 1);
    let confirm = screen.seen.confirm.as_ref().expect("asks first");
    assert_eq!(confirm.title, format!("Delete the {OLDER} backup?"));
    assert!(confirm.body.contains("This cannot be undone."));
    assert_eq!(screen.store().list().len(), 1);
    screen.accept();
    assert!(screen.store().list().is_empty());
    assert!(
        screen
            .seen
            .toasts
            .last()
            .unwrap()
            .text
            .starts_with(&format!("Deleted the {OLDER} shader backup, freed 2.0 KB"))
    );
    assert!(!screen.has("Restore"));
}

#[test]
fn everything_disabled_while_game_runs() {
    let mut screen = Harness::with_caches();
    screen.reload();
    screen.back_up(OLDER, "vulkanshadercache");
    screen.ctx.game = RunState::Running;
    screen.reload();
    assert!(screen.has(BLOCKED));
    screen.tick(screen.cache(OLDER, "shaders"));
    assert!(screen.has("1 selected, 2.0 KB"));
    assert_eq!(screen.click("Clear"), 0);
    assert_eq!(screen.click("Back up and clear"), 0);
    assert_eq!(screen.click("Restore"), 0);
    assert_eq!(screen.click("Delete"), 0);
    screen.send(Message::Clear);
    screen.send(Message::ClearConfirmed);
    screen.send(Message::BackUpAndClear);
    assert!(screen.seen.confirm.is_none());
    assert!(screen.cache(OLDER, "shaders").join("cache.bin").exists());
    assert_eq!(screen.store().list().len(), 1);

    screen.ctx.game = RunState::Unknown;
    assert!(screen.has(
        "star-config cannot tell whether Star Citizen is running. Close the game to change shader caches."
    ));
    assert_eq!(screen.click("Clear"), 0);
}

#[test]
fn unknown_folder_beside_caches_is_never_listed() {
    let mut screen = Harness::with_caches();
    put(&screen.sc().join("GraphicsSettings/shaders/g.bin"), 64);
    put(&screen.sc().join("logs/game.log"), 64);
    put(&screen.sc().join(CURRENT).join("crashes/dump.dmp"), 64);
    screen.reload();
    assert!(!screen.has("GraphicsSettings"));
    assert!(!screen.has("logs"));
    assert!(!screen.has_part("crashes"));
    assert!(screen.has("6.0 KB"));
}

#[test]
fn incomplete_backup_is_flagged_and_can_only_be_deleted() {
    let mut screen = Harness::with_caches();
    let dir = screen
        .ctx
        .dirs
        .shader_backups()
        .join(OLDER)
        .join("20261114T201500Z");
    put(&dir.join("shaders/cache.bin"), 512);
    let original = screen.cache(OLDER, "shaders");
    fs::write(
        dir.join("backup.toml"),
        format!(
            "kind = \"shaders\"\nsize = 2048\ncreated_unix = 1794687300\noriginal = {:?}\ncomplete = false\n",
            original.to_string_lossy()
        ),
    )
    .unwrap();
    screen.reload();
    assert!(screen.has("Incomplete"));
    assert!(screen.has(INTERRUPTED));
    assert_eq!(screen.click("Restore"), 0);
    screen.send(Message::Restore(Box::new(screen.store().list().remove(0))));
    assert!(screen.seen.confirm.is_none());
    assert!(dir.exists());
    assert_eq!(screen.click("Delete"), 1);
    screen.accept();
    assert!(!dir.exists());
    assert!(screen.store().list().is_empty());
}

#[test]
fn translation_layer_caches_get_their_own_table() {
    let mut screen = Harness::with_caches();
    let live = screen.ctx.channel.as_ref().unwrap().dir.clone();
    put(&live.join("Bin64/StarCitizen.dxvk-cache"), 4096);
    screen.reload();
    assert!(screen.has("TRANSLATION LAYER CACHES"));
    assert!(screen.has("DXVK"));
    assert!(screen.has("Tick the caches to back up or clear."));
    assert!(screen.has("10.0 KB"));
}

// Pictures --------------------------------------------------------------------

const GB: u64 = 1024 * 1024 * 1024;
const MB: u64 = 1024 * 1024;

fn shot(app: &App, name: &str) {
    let mut ui = Simulator::with_size(sc_app::settings(), FULL, app.view());
    testkit::snapshot(&mut ui, &app.theme(), name);
}

fn demo_app(temp: &Path, choice: ThemeChoice) -> App {
    let mut ctx = testkit::context_with_channels(temp, &["LIVE", "PTU"]).with_probe(idle());
    ctx.settings.theme = choice;
    write_build(&ctx.channel.as_ref().unwrap().dir);
    let sc = star_citizen_cache_dir(temp);
    put(&sc.join(CURRENT).join("shaders/cache-0.bin"), 1_650 * MB);
    put(
        &sc.join(CURRENT).join("vulkanshadercache/pipeline.bin"),
        412 * MB,
    );
    put(&sc.join(OLDER).join("shaders/cache-0.bin"), 1_420 * MB);
    put(
        &sc.join(OLDER).join("vulkanshadercache/pipeline.bin"),
        380 * MB,
    );
    put(
        &sc.join("sc-alpha-4.1.1").join("shaders/cache-0.bin"),
        GB + 200 * MB,
    );
    for (version, kind, days) in [
        (CURRENT, "shaders", 2),
        (CURRENT, "vulkanshadercache", 2),
        (OLDER, "shaders", 38),
        (OLDER, "vulkanshadercache", 41),
    ] {
        age(&sc.join(version).join(kind), days);
    }
    let install = ctx.install.clone().unwrap();
    let roots = shaders::appdata_roots(&install, None);
    let old = shaders::discover(&install, &roots)
        .into_iter()
        .find(|cache| cache.version.as_deref() == Some("sc-alpha-4.1.1"))
        .unwrap();
    ShaderBackups::new(ctx.dirs.shader_backups())
        .backup_and_clear(&old, idle().as_ref())
        .unwrap();
    let mut app = App::with(ctx);
    testkit::feed(
        &mut app,
        [
            app::Message::Navigate(Screen::Maintenance),
            app::Message::Maintenance(maintenance::Message::Tick(
                sc.join(OLDER).join("shaders"),
                true,
            )),
            app::Message::Maintenance(maintenance::Message::Tick(
                sc.join(OLDER).join("vulkanshadercache"),
                true,
            )),
        ],
    );
    app
}

#[test]
fn maintenance_pictures() {
    for (choice, name) in [(ThemeChoice::Dark, "dark"), (ThemeChoice::Light, "light")] {
        let temp = tempfile::tempdir().unwrap();
        let app = demo_app(temp.path(), choice);
        shot(&app, &format!("maintenance-{name}"));
    }
}
