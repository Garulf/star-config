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
use sc_app::screens::channels::{
    self, CANNOT_TELL, COMPARE_CONTENTS, Message, NOT_COPIED, PENDING_TITLE, STARSTRINGS_NOTE,
    SWAP_TRAVEL, State, Survey,
};
use sc_core::channels::{self as core, CopyPlan, Part, PlannedFile, Progress, Step, SwapJournal};
use sc_core::install::{Channel, Install};
use sc_core::process::{FixedProbe, RunState};
use sc_core::settings::ThemeChoice;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const FULL: Size = Size::new(1280.0, 800.0);
/// Tall enough that the whole scrolling page is on screen, so every button can be clicked.
const TALL: Size = Size::new(1280.0, 1600.0);
const LIVE_CFG: &str = "cl_fov = 90\nr_MotionBlur = 0\n";
const PTU_CFG: &str = "r_DisplayInfo = 1\n";
const LIVE_MAPS: &str = "<ActionMaps version=\"1\" profile=\"live\"/>\n";
const PTU_MAPS: &str = "<ActionMaps version=\"1\" profile=\"ptu\"/>\n";
const LIVE_ATTRS: &str =
    "<Attributes Version=\"1\">\n <Attr name=\"FOV\" value=\"90\"/>\n</Attributes>\n";
const PTU_ATTRS: &str =
    "<Attributes Version=\"1\">\n <Attr name=\"FOV\" value=\"75\"/>\n</Attributes>\n";
const SAME_CHANNEL: &str = "From and To are the same channel. Pick two different ones.";
const PENDING_BLOCKS: &str =
    "Finish or undo the unfinished swap before you copy or swap game files.";

fn probe(game: RunState, launcher: RunState) -> Arc<FixedProbe> {
    Arc::new(FixedProbe { game, launcher })
}

fn idle() -> Arc<FixedProbe> {
    probe(RunState::NotRunning, RunState::NotRunning)
}

fn put(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn set_mtime(path: &Path, when: SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(when)
        .unwrap();
}

fn mtime(path: &Path) -> SystemTime {
    fs::metadata(path).unwrap().modified().unwrap()
}

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}

fn text(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

fn channel_dir(ctx: &Context, name: &str) -> PathBuf {
    ctx.install.as_ref().unwrap().root.join(name)
}

fn channel(ctx: &Context, name: &str) -> Channel {
    Channel {
        name: name.to_string(),
        dir: channel_dir(ctx, name),
    }
}

/// LIVE and PTU with small stand-ins for the game files and a player's own files.
fn fixture(temp: &Path) -> Context {
    let ctx = testkit::context_with_channels(temp, &["LIVE", "PTU"]).with_probe(idle());
    let live = channel(&ctx, "LIVE");
    let ptu = channel(&ctx, "PTU");
    put(&live.dir.join("Data.p4k"), &[b'L'; 4096]);
    put(&live.dir.join("Bin64/StarCitizen.exe"), &[b'l'; 2048]);
    put(&live.dir.join("Bin64/shared.dll"), &[b's'; 1024]);
    put(
        &live.dir.join("build_manifest.id"),
        br#"{"Data":{"Version":"4.3.2","RequestedP4ChangeNum":"9876543"}}"#,
    );
    put(&live.dir.join("Game.log"), b"live log");
    put(&live.user_cfg(), LIVE_CFG.as_bytes());
    put(&live.actionmaps_xml(), LIVE_MAPS.as_bytes());
    put(&live.attributes_xml(), LIVE_ATTRS.as_bytes());
    put(&live.global_ini(), b"live strings");
    for n in 1..=3 {
        put(
            &live.screenshots_dir().join(format!("ScreenShot-{n}.jpg")),
            b"jpg",
        );
    }
    put(&ptu.dir.join("Data.p4k"), &[b'P'; 3000]);
    put(&ptu.dir.join("Bin64/StarCitizen.exe"), &[b'p'; 2048]);
    put(&ptu.dir.join("Bin64/shared.dll"), &[b's'; 1024]);
    put(
        &ptu.dir.join("build_manifest.id"),
        br#"{"Data":{"Version":"4.4.0-ptu","RequestedP4ChangeNum":"9912345"}}"#,
    );
    put(&ptu.dir.join("Game.log"), b"ptu log");
    put(&ptu.user_cfg(), PTU_CFG.as_bytes());
    put(&ptu.actionmaps_xml(), PTU_MAPS.as_bytes());
    put(&ptu.attributes_xml(), PTU_ATTRS.as_bytes());
    let day_ago = SystemTime::now() - Duration::from_secs(86_400);
    set_mtime(&ptu.dir.join("Bin64/StarCitizen.exe"), day_ago);
    set_mtime(
        &ptu.dir.join("Bin64/shared.dll"),
        mtime(&live.dir.join("Bin64/shared.dll")),
    );
    ctx
}

#[derive(Default)]
struct Seen {
    toasts: Vec<Toast>,
    confirm: Option<Confirm<Message>>,
    rescans: usize,
    survey: Option<Survey>,
}

struct Harness {
    _temp: tempfile::TempDir,
    ctx: Context,
    state: State,
    seen: Seen,
}

impl Harness {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let ctx = fixture(temp.path());
        Self {
            _temp: temp,
            ctx,
            state: State::new(),
            seen: Seen::default(),
        }
    }

    fn loaded() -> Self {
        let mut screen = Self::new();
        screen.reload();
        screen
    }

    fn dir(&self, name: &str) -> PathBuf {
        channel_dir(&self.ctx, name)
    }

    fn reload(&mut self) {
        let task = self.state.reload(&self.ctx);
        self.run(task);
    }

    fn send(&mut self, message: Message) {
        let task = self.send_only(message);
        self.run(task);
    }

    fn send_only(&mut self, message: Message) -> Task<Message> {
        if let Message::Surveyed(survey) = &message {
            self.seen.survey = Some((**survey).clone());
        }
        let action = self.state.update(message, &self.ctx);
        self.record(action)
    }

    fn record(&mut self, action: Action<Message>) -> Task<Message> {
        self.seen.toasts.extend(action.toast);
        if action.confirm.is_some() {
            self.seen.confirm = action.confirm;
        }
        if action.rescan {
            self.seen.rescans += 1;
            let install = Install::from_dir(&self.ctx.install.as_ref().unwrap().root).unwrap();
            let channels = install.channels();
            self.ctx.set_install(Some(install), channels);
            let reload = self.state.reload(&self.ctx);
            return Task::batch([action.task, reload]);
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
                    queue.push(self.send_only(message));
                }
            }
        }
    }

    fn sim(&self) -> Simulator<'_, Message> {
        Simulator::with_size(sc_app::settings(), TALL, self.state.view(&self.ctx))
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

    fn plan(&mut self) {
        assert_eq!(self.click("Copy game files"), 1);
    }

    fn untick_all_but(&mut self, keep: &[Part]) {
        for part in [
            Part::UserCfg,
            Part::Keybinds,
            Part::Layouts,
            Part::GameSettings,
            Part::StarStrings,
        ] {
            self.send(Message::TickPart(part, keep.contains(&part)));
        }
    }
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn path_json(path: &Path) -> String {
    quoted(&path.display().to_string())
}

/// The journal as the core saves it, so `pending_swap` reads it back.
fn journal_json(journal: &SwapJournal) -> String {
    let steps: Vec<String> = journal
        .steps
        .iter()
        .map(|Step::Rename { from, to }| {
            format!(
                "{{\"Rename\":{{\"from\":{},\"to\":{}}}}}",
                path_json(from),
                path_json(to)
            )
        })
        .collect();
    format!(
        "{{\"install_root\":{},\"a\":{},\"b\":{},\"steps\":[{}],\"done\":{},\"ids\":[]}}",
        path_json(&journal.install_root),
        quoted(&journal.a),
        quoted(&journal.b),
        steps.join(","),
        journal.done
    )
}

/// Plans a LIVE and PTU swap with the core, runs its first `done` steps, and leaves the
/// journal behind as if star-config had stopped there.
fn interrupt_swap(ctx: &Context, done: usize) -> PathBuf {
    let install = ctx.install.clone().unwrap();
    let mut journal = core::plan_swap(&install, "LIVE", "PTU", false).unwrap();
    for Step::Rename { from, to } in &journal.steps[..done] {
        fs::rename(from, to).unwrap();
    }
    journal.done = done;
    let file = core::journal_path(&channels::journal_dir(&ctx.dirs), &install);
    put(&file, journal_json(&journal).as_bytes());
    assert!(core::pending_swap(&file).unwrap().is_some());
    file
}

// Channel list and pair ---------------------------------------------------------

#[test]
fn lists_known_channels_installed_first_with_build_and_user_file_line() {
    let screen = Harness::loaded();
    let survey = screen.seen.survey.as_ref().expect("surveyed");
    let names: Vec<&str> = survey.facts.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["LIVE", "PTU", "EPTU", "HOTFIX", "TECH-PREVIEW"]);
    let installed: Vec<bool> = survey.facts.iter().map(|f| f.installed).collect();
    assert_eq!(installed, [true, true, false, false, false]);
    for name in names {
        assert!(screen.has_part(name), "{name}");
    }
    assert!(screen.has("Installed"));
    assert!(screen.has("Not installed"));
    assert!(screen.has("4.3.2 (9876543)"));
    assert!(screen.has("4.4.0-ptu (9912345)"));
    assert!(screen.has("Has settings and 3 screenshots"));
    assert!(screen.has("Has settings"));
    assert!(!screen.has("Calculating..."));
    assert_eq!(screen.state.pending(), 0);
}

#[test]
fn sizes_show_calculating_until_they_arrive() {
    let mut screen = Harness::new();
    let task = screen.state.reload(&screen.ctx);
    let survey = {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let stream = runtime::task::into_stream(task).unwrap();
        let actions: Vec<_> = rt.block_on(stream.collect());
        actions
            .into_iter()
            .find_map(|action| match action {
                runtime::Action::Output(message) => Some(message),
                _ => None,
            })
            .unwrap()
    };
    let _measure = screen.send_only(survey);
    assert!(screen.has("Calculating..."));
}

#[test]
fn same_source_and_target_is_refused_inline() {
    let mut screen = Harness::loaded();
    assert!(!screen.has(SAME_CHANNEL));
    screen.send(Message::PickTo("LIVE".into()));
    assert!(screen.has(SAME_CHANNEL));
    assert_eq!(screen.click("Copy game files"), 0);
    assert_eq!(screen.click("Swap game files"), 0);
}

#[test]
fn source_without_game_files_is_refused_in_plain_words() {
    let mut screen = Harness::loaded();
    screen.send(Message::PickFrom("HOTFIX".into()));
    assert!(screen.has("HOTFIX is not installed, so there is nothing to copy from."));
    assert_eq!(screen.click("Copy game files"), 0);
}

#[test]
fn target_may_be_a_channel_that_is_not_installed() {
    let mut screen = Harness::loaded();
    screen.send(Message::PickTo("HOTFIX".into()));
    assert!(!screen.has(SAME_CHANNEL));
    assert!(screen.has("HOTFIX is created when you copy game files to it."));
    assert_eq!(screen.click("Swap game files"), 0);
    screen.plan();
    assert!(screen.has_part("4 files, "));
    assert_eq!(screen.click("Start copy"), 1);
    let hotfix = screen.dir("HOTFIX");
    assert_eq!(
        read(&hotfix.join("Data.p4k")),
        read(&screen.dir("LIVE").join("Data.p4k"))
    );
    assert!(!hotfix.join("user.cfg").exists());
    assert!(!hotfix.join("Game.log").exists());
    assert!(screen.seen.rescans > 0);
    let survey = screen.seen.survey.as_ref().unwrap();
    let hotfix_facts = survey.facts.iter().find(|f| f.name == "HOTFIX").unwrap();
    assert!(hotfix_facts.installed);
}

// Copy ------------------------------------------------------------------------

#[test]
fn copy_plans_first_and_shows_counts_and_free_space() {
    let mut screen = Harness::loaded();
    let before = read(&screen.dir("PTU").join("Data.p4k"));
    screen.plan();
    assert!(screen.has("Copy LIVE to PTU"));
    assert!(screen.has_part("3 files, "));
    assert!(screen.has_part(" to copy. 1 file already matches and is skipped."));
    assert!(screen.has_part(" free on this disk."));
    assert!(screen.has(NOT_COPIED));
    assert!(screen.has("Start copy"));
    assert_eq!(read(&screen.dir("PTU").join("Data.p4k")), before);
}

#[test]
fn start_copy_copies_game_files_and_leaves_user_files_alone() {
    let mut screen = Harness::loaded();
    let (live, ptu) = (screen.dir("LIVE"), screen.dir("PTU"));
    screen.plan();
    assert_eq!(screen.click("Start copy"), 1);
    assert_eq!(read(&ptu.join("Data.p4k")), read(&live.join("Data.p4k")));
    assert_eq!(
        read(&ptu.join("Bin64/StarCitizen.exe")),
        read(&live.join("Bin64/StarCitizen.exe"))
    );
    assert_eq!(text(&ptu.join("user.cfg")), PTU_CFG);
    assert_eq!(
        text(&channel(&screen.ctx, "PTU").actionmaps_xml()),
        PTU_MAPS
    );
    assert_eq!(text(&ptu.join("Game.log")), "ptu log");
    assert!(!ptu.join("ScreenShots").exists());
    assert!(!screen.state.busy());
}

#[test]
fn copy_finished_banner_tells_the_user_to_verify() {
    let mut screen = Harness::loaded();
    screen.plan();
    screen.click("Start copy");
    let done = "Copy finished. Open the launcher and run Verify on PTU before you play.";
    assert!(screen.has(done));
    screen.reload();
    assert!(screen.has(done), "the banner stays after a reload");
    assert_eq!(screen.click("Dismiss"), 1);
    assert!(!screen.has(done));
}

#[test]
fn cancel_stops_and_reports_the_target_state() {
    let mut screen = Harness::loaded();
    let before = read(&screen.dir("PTU").join("Data.p4k"));
    screen.plan();
    let copy = screen.send_only(Message::StartCopy);
    assert!(screen.state.busy());
    assert!(screen.has("Cancel"));
    assert!(screen.has_part(" of "));
    assert_eq!(screen.click("Cancel"), 1);
    assert!(screen.has("Stopping after the current file..."));
    screen.run(copy);
    assert!(!screen.state.busy());
    assert!(screen.has("Copy stopped. PTU was not changed."));
    assert_eq!(read(&screen.dir("PTU").join("Data.p4k")), before);
}

#[test]
fn progress_shows_bytes_and_the_current_file() {
    let mut screen = Harness::loaded();
    screen.plan();
    let _copy = screen.send_only(Message::StartCopy);
    screen.send(Message::CopyProgress(
        1,
        Progress {
            files_done: 0,
            files_total: 3,
            bytes_done: 5120,
            bytes_total: 6200,
        },
    ));
    assert!(screen.has("5.0 KB of 6.1 KB"));
    let names = ["Data.p4k", "Bin64/StarCitizen.exe", "build_manifest.id"];
    assert!(
        names
            .iter()
            .any(|name| screen.has(&format!("Copying {name}")))
    );
}

#[test]
fn compare_contents_replans() {
    let mut screen = Harness::loaded();
    let (live, ptu) = (screen.dir("LIVE"), screen.dir("PTU"));
    put(&ptu.join("Bin64/shared.dll"), &[b'S'; 1024]);
    set_mtime(
        &ptu.join("Bin64/shared.dll"),
        mtime(&live.join("Bin64/shared.dll")),
    );
    screen.plan();
    assert!(screen.has_part("3 files, "));
    assert!(screen.has_part("1 file already matches"));
    assert_eq!(screen.click(COMPARE_CONTENTS), 1);
    assert!(screen.has_part("4 files, "));
    assert!(!screen.has_part("already match"));
    screen.click("Start copy");
    assert_eq!(
        read(&ptu.join("Bin64/shared.dll")),
        read(&live.join("Bin64/shared.dll"))
    );
}

// Swap ------------------------------------------------------------------------

#[test]
fn swap_confirm_lists_what_happens_and_defaults_to_settings_staying() {
    let mut screen = Harness::loaded();
    let before = read(&screen.dir("LIVE").join("Data.p4k"));
    assert_eq!(screen.click("Swap game files"), 1);
    assert!(screen.has("Swap LIVE and PTU?"));
    assert!(screen.has("LIVE's folder is set aside under a temporary name."));
    assert!(screen.has("PTU's folder is renamed to LIVE."));
    assert!(screen.has("The set-aside folder is renamed to PTU."));
    assert!(screen.has_part("Settings and screenshots move back to their own channel"));
    assert!(screen.has(
        "LIVE's game files become PTU's and PTU's become LIVE's. Nothing is copied or deleted. Your settings stay with their channel."
    ));
    assert!(screen.has(SWAP_TRAVEL));
    assert_eq!(read(&screen.dir("LIVE").join("Data.p4k")), before);
    assert_eq!(screen.click("Cancel"), 1);
    assert!(screen.has("Swap game files"));
}

#[test]
fn swap_exchanges_game_files_and_keeps_user_files_with_their_channel() {
    let mut screen = Harness::loaded();
    let (live, ptu) = (screen.dir("LIVE"), screen.dir("PTU"));
    let (live_p4k, ptu_p4k) = (read(&live.join("Data.p4k")), read(&ptu.join("Data.p4k")));
    screen.click("Swap game files");
    assert_eq!(screen.click("Swap"), 1);
    assert_eq!(read(&live.join("Data.p4k")), ptu_p4k);
    assert_eq!(read(&ptu.join("Data.p4k")), live_p4k);
    assert_eq!(text(&live.join("user.cfg")), LIVE_CFG);
    assert_eq!(text(&ptu.join("user.cfg")), PTU_CFG);
    assert_eq!(
        text(&channel(&screen.ctx, "LIVE").actionmaps_xml()),
        LIVE_MAPS
    );
    assert_eq!(fs::read_dir(live.join("ScreenShots")).unwrap().count(), 3);
    assert!(
        screen.has(
            "Swap finished. Open the launcher and run Verify on LIVE and PTU before you play."
        )
    );
    let journals = fs::read_dir(channels::journal_dir(&screen.ctx.dirs))
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(journals, 0, "no journal is left");
    assert!(screen.seen.rescans > 0);
}

#[test]
fn swap_with_the_tick_box_moves_user_files_too() {
    let mut screen = Harness::loaded();
    let (live, ptu) = (screen.dir("LIVE"), screen.dir("PTU"));
    screen.click("Swap game files");
    assert_eq!(screen.click(SWAP_TRAVEL), 1);
    assert!(screen.has_part("Your settings and screenshots move with the game files."));
    assert_eq!(screen.click("Swap"), 1);
    assert_eq!(text(&live.join("user.cfg")), PTU_CFG);
    assert_eq!(text(&ptu.join("user.cfg")), LIVE_CFG);
    assert_eq!(fs::read_dir(ptu.join("ScreenShots")).unwrap().count(), 3);
}

#[cfg(unix)]
#[test]
fn swap_in_a_folder_that_cannot_be_renamed_says_so_and_changes_nothing() {
    use sc_app::screens::channels::SWAP_UNDONE;
    use std::os::unix::fs::PermissionsExt;
    let mut screen = Harness::loaded();
    let root = screen.ctx.install.as_ref().unwrap().root.clone();
    let before = read(&screen.dir("LIVE").join("Data.p4k"));
    screen.click("Swap game files");
    let original = fs::metadata(&root).unwrap().permissions();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o555)).unwrap();
    screen.click("Swap");
    fs::set_permissions(&root, original).unwrap();
    let shown = screen.ctx.display_path(&root);
    assert!(shown.starts_with('~'), "{shown}");
    assert!(screen.has(&format!(
        "{SWAP_UNDONE} star-config cannot rename folders in {shown}. Check the folder's permissions."
    )));
    assert_eq!(read(&screen.dir("LIVE").join("Data.p4k")), before);
}

#[test]
fn game_file_actions_disabled_while_launcher_runs_and_say_which() {
    let mut screen = Harness::loaded();
    screen.ctx = screen
        .ctx
        .clone()
        .with_probe(probe(RunState::NotRunning, RunState::Running));
    screen.ctx.launcher = RunState::Running;
    assert!(screen.has("Close the launcher to copy or swap game files."));
    assert_eq!(screen.click("Copy game files"), 0);
    assert_eq!(screen.click("Swap game files"), 0);
    screen.ctx.game = RunState::Running;
    assert!(screen.has("Close the game and the launcher to copy or swap game files."));
    screen.ctx.launcher = RunState::NotRunning;
    assert!(screen.has("Close the game to copy or swap game files."));
}

#[test]
fn unknown_launcher_counts_as_running_until_unlocked() {
    let mut screen = Harness::loaded();
    screen.ctx = screen
        .ctx
        .clone()
        .with_probe(probe(RunState::NotRunning, RunState::Unknown));
    screen.ctx.launcher = RunState::Unknown;
    assert!(screen.has("Close the launcher to copy or swap game files."));
    assert_eq!(screen.click("Copy game files"), 0);
    screen.ctx.unlocked = true;
    assert!(!screen.has("Close the launcher to copy or swap game files."));
    screen.plan();
    screen.click("Start copy");
    assert!(screen.has_part("Copy finished."));
}

// Settings --------------------------------------------------------------------

#[test]
fn copy_settings_copies_only_ticked_parts_and_offers_undo() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = fixture(temp.path());
    let ptu = channel(&ctx, "PTU");
    let mut app = App::with(ctx);
    testkit::feed(&mut app, [app::Message::Navigate(Screen::Channels)]);
    testkit::feed(
        &mut app,
        [app::Message::Channels(Message::TickPart(
            Part::Keybinds,
            false,
        ))],
    );
    testkit::feed(&mut app, [app::Message::Channels(Message::CopySettings)]);
    let dialog = app.dialog().expect("asks first");
    assert_eq!(dialog.title, "Copy 3 parts from LIVE to PTU?");
    assert!(
        dialog
            .body
            .starts_with("user.cfg, Exported layouts, Game settings")
    );
    assert_eq!(dialog.actions[0].label, "Copy");
    testkit::feed(&mut app, [app::Message::DialogAction(0)]);
    assert_eq!(text(&ptu.user_cfg()), LIVE_CFG);
    assert_eq!(text(&ptu.attributes_xml()), LIVE_ATTRS);
    assert_eq!(text(&ptu.actionmaps_xml()), PTU_MAPS);
    let toast = app.toasts().last().expect("a toast").clone();
    assert_eq!(toast.text, "Copied 3 parts from LIVE to PTU");
    assert!(toast.undo.is_some());
    assert_eq!(toast.undo_in.len(), 1);
    assert_eq!(toast.undo_in[0].channel.name, "PTU");
    assert_eq!(app.context().channel.as_ref().unwrap().name, "LIVE");
    let index = app.toasts().len() - 1;
    testkit::feed(&mut app, [app::Message::UndoToast(index)]);
    assert_eq!(text(&ptu.user_cfg()), PTU_CFG);
    assert_eq!(text(&ptu.attributes_xml()), PTU_ATTRS);
    assert_eq!(
        app.toasts().last().unwrap().text,
        "Restored the previous files"
    );
}

#[test]
fn starstrings_part_is_unticked_by_default() {
    let mut screen = Harness::loaded();
    assert!(screen.has(STARSTRINGS_NOTE));
    screen.click("Copy settings");
    let confirm = screen.seen.confirm.as_ref().expect("asks first");
    assert_eq!(confirm.title, "Copy 4 parts from LIVE to PTU?");
    assert!(!confirm.body.contains("StarStrings"));
    screen.accept();
    let ptu = channel(&screen.ctx, "PTU");
    assert!(!ptu.global_ini().exists());
    assert_eq!(text(&ptu.user_cfg()), LIVE_CFG);
    assert_eq!(
        screen.seen.toasts.last().unwrap().text,
        "Copied 4 parts from LIVE to PTU"
    );
}

#[test]
fn swap_settings_exchanges_ticked_parts() {
    let mut screen = Harness::loaded();
    screen.untick_all_but(&[Part::UserCfg, Part::Keybinds]);
    screen.click("Swap settings");
    let confirm = screen.seen.confirm.as_ref().expect("asks first");
    assert_eq!(confirm.title, "Swap 2 parts between LIVE and PTU?");
    screen.accept();
    let (live, ptu) = (channel(&screen.ctx, "LIVE"), channel(&screen.ctx, "PTU"));
    assert_eq!(text(&live.user_cfg()), PTU_CFG);
    assert_eq!(text(&ptu.user_cfg()), LIVE_CFG);
    assert_eq!(text(&live.actionmaps_xml()), PTU_MAPS);
    assert_eq!(text(&ptu.actionmaps_xml()), LIVE_MAPS);
    assert_eq!(text(&live.attributes_xml()), LIVE_ATTRS);
    let toast = screen.seen.toasts.last().unwrap();
    assert_eq!(toast.text, "Swapped 2 parts between LIVE and PTU");
    let undo: Vec<&str> = toast
        .undo_in
        .iter()
        .map(|t| t.channel.name.as_str())
        .collect();
    assert_eq!(undo, ["LIVE", "PTU"]);
}

#[test]
fn settings_are_blocked_while_the_game_runs() {
    let mut screen = Harness::loaded();
    screen.ctx = screen
        .ctx
        .clone()
        .with_probe(probe(RunState::Running, RunState::NotRunning));
    screen.ctx.game = RunState::Running;
    assert!(screen.has("Close the game to copy or swap settings."));
    assert_eq!(screen.click("Copy settings"), 0);
    assert_eq!(screen.click("Swap settings"), 0);
}

// Unfinished swap -------------------------------------------------------------

#[test]
fn pending_journal_prompts_at_startup() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = fixture(temp.path());
    interrupt_swap(&ctx, 1);
    let app = App::with(ctx);
    let dialog = app.dialog().expect("prompts at startup");
    assert_eq!(dialog.title, PENDING_TITLE);
    assert!(
        dialog
            .body
            .starts_with("star-config was swapping LIVE and PTU and stopped after step 1 of "),
        "{}",
        dialog.body
    );
    let labels: Vec<&str> = dialog.actions.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(labels, ["Finish the swap", "Undo it", "Decide later"]);
    let mut ui = Simulator::with_size(sc_app::settings(), FULL, app.view());
    assert!(ui.find("A channel swap did not finish").is_ok());
}

#[test]
fn swap_prompt_waits_for_the_interrupted_change_prompt() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = fixture(temp.path());
    interrupt_swap(&ctx, 0);
    let live = channel(&ctx, "LIVE");
    let scope = sc_core::backup::Scope {
        install_key: ctx.install.as_ref().unwrap().key(),
        channel: "LIVE".into(),
    };
    ctx.writer()
        .store()
        .begin(&scope, &live, &[live.user_cfg()], "Config changes")
        .unwrap();
    let mut app = App::with(ctx);
    assert_eq!(app.dialog().unwrap().title, "A change was interrupted");
    testkit::feed(&mut app, [app::Message::CloseDialog]);
    assert_eq!(app.dialog().unwrap().title, PENDING_TITLE);
    testkit::feed(&mut app, [app::Message::CloseDialog]);
    assert!(app.dialog().is_none());
}

#[test]
fn finish_and_undo_both_reach_a_consistent_state() {
    for (choice, finished) in [(0, true), (1, false)] {
        let temp = tempfile::tempdir().unwrap();
        let ctx = fixture(temp.path());
        let (live, ptu) = (channel_dir(&ctx, "LIVE"), channel_dir(&ctx, "PTU"));
        let (live_p4k, ptu_p4k) = (read(&live.join("Data.p4k")), read(&ptu.join("Data.p4k")));
        let journal = interrupt_swap(&ctx, 1);
        let root = ctx.install.as_ref().unwrap().root.clone();
        let mut app = App::with(ctx);
        testkit::feed(&mut app, [app::Message::DialogAction(choice)]);
        let (expect_live, expect_ptu) = if finished {
            (&ptu_p4k, &live_p4k)
        } else {
            (&live_p4k, &ptu_p4k)
        };
        assert_eq!(&read(&live.join("Data.p4k")), expect_live);
        assert_eq!(&read(&ptu.join("Data.p4k")), expect_ptu);
        assert_eq!(text(&live.join("user.cfg")), LIVE_CFG);
        assert_eq!(text(&ptu.join("user.cfg")), PTU_CFG);
        assert!(!journal.exists());
        assert!(!root.join(".star-config-swap").exists());
        let toast = app.toasts().last().expect("a toast");
        if finished {
            assert!(toast.text.starts_with("Swap finished."), "{}", toast.text);
        } else {
            assert_eq!(
                toast.text,
                "The swap was undone. LIVE and PTU are back as they were."
            );
        }
    }
}

#[test]
fn pending_journal_disables_game_file_actions_on_the_screen() {
    let mut screen = Harness::new();
    interrupt_swap(&screen.ctx, 1);
    screen.reload();
    assert!(screen.has(PENDING_TITLE));
    assert!(screen.has("Finish the swap"));
    assert!(screen.has("Undo it"));
    assert!(screen.has(PENDING_BLOCKS));
    assert_eq!(screen.click("Copy game files"), 0);
    assert_eq!(screen.click("Swap game files"), 0);
    assert_eq!(screen.click("Undo it"), 1);
    assert!(!screen.has(PENDING_TITLE));
    assert!(screen.has("The swap was undone. LIVE and PTU are back as they were."));
    assert_eq!(screen.click("Copy game files"), 1);
}

#[test]
fn resume_that_cannot_tell_how_far_it_got_touches_nothing() {
    let mut screen = Harness::new();
    let file = interrupt_swap(&screen.ctx, 1);
    let root = screen.ctx.install.as_ref().unwrap().root.clone();
    fs::rename(root.join("PTU"), root.join("PTU-moved-by-hand")).unwrap();
    screen.reload();
    assert_eq!(screen.click("Finish the swap"), 1);
    assert!(screen.has_part(CANNOT_TELL));
    let shown = screen.ctx.display_path(&root.join("PTU"));
    assert!(screen.has_part(&shown), "{shown}");
    assert!(!screen.has("Finish the swap"));
    assert!(!screen.has("Undo it"));
    assert!(root.join(".star-config-swap").exists());
    assert!(root.join("PTU-moved-by-hand").exists());
    assert_eq!(screen.click("Dismiss"), 1);
    assert!(!screen.has_part(CANNOT_TELL));
    assert!(file.exists(), "the journal stays");
    assert_eq!(screen.click("Copy game files"), 0);
}

// Shell -----------------------------------------------------------------------

#[test]
fn switching_channel_while_a_copy_runs_asks_first() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::with(fixture(temp.path()));
    testkit::feed(
        &mut app,
        [
            app::Message::Navigate(Screen::Channels),
            app::Message::Channels(Message::PlanCopy),
        ],
    );
    let _copy = app.update(app::Message::Channels(Message::StartCopy));
    let _ = app.update(app::Message::PickChannel("PTU".into()));
    let dialog = app.dialog().expect("asks first");
    assert_eq!(dialog.title, "A copy is running. Stop it and switch?");
    assert_eq!(dialog.actions[0].label, "Stop and switch");
    testkit::feed(&mut app, [app::Message::DialogAction(0)]);
    assert_eq!(app.context().channel.as_ref().unwrap().name, "PTU");
}

// Pictures --------------------------------------------------------------------

const GB: u64 = 1024 * 1024 * 1024;
const MB: u64 = 1024 * 1024;

fn sparse(path: &Path, len: u64) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::File::create(path).unwrap().set_len(len).unwrap();
}

fn demo_app(temp: &Path, choice: ThemeChoice) -> App {
    let mut ctx = testkit::context_with_channels(temp, &["LIVE", "PTU"]).with_probe(idle());
    ctx.settings.theme = choice;
    let live = channel(&ctx, "LIVE");
    let ptu = channel(&ctx, "PTU");
    for (dir, p4k, version, change) in [
        (&live.dir, 96 * GB + 410 * MB, "4.3.2", "9876543"),
        (&ptu.dir, 92 * GB + 120 * MB, "4.4.0-PTU", "9912345"),
    ] {
        sparse(&dir.join("Data.p4k"), p4k);
        sparse(&dir.join("Bin64/StarCitizen.exe"), 180 * MB);
        put(
            &dir.join("build_manifest.id"),
            format!(r#"{{"Data":{{"Version":"{version}","RequestedP4ChangeNum":"{change}"}}}}"#)
                .as_bytes(),
        );
    }
    testkit::write_channel_files(
        &live,
        Some(testkit::DEMO_USER_CFG.as_bytes()),
        Some(testkit::demo_attributes().as_bytes()),
    );
    put(&live.actionmaps_xml(), LIVE_MAPS.as_bytes());
    put(
        &live.mappings_dir().join("hotas_vkb_gladiator.xml"),
        LIVE_MAPS.as_bytes(),
    );
    put(
        &live.mappings_dir().join("keyboard_mouse.xml"),
        LIVE_MAPS.as_bytes(),
    );
    for n in 0..214 {
        put(
            &live
                .screenshots_dir()
                .join(format!("ScreenShot-2026-10-{:02}_{n:03}.jpg", n % 28 + 1)),
            b"",
        );
    }
    testkit::write_channel_files(&ptu, Some(PTU_CFG.as_bytes()), None);
    for n in 0..12 {
        put(&ptu.screenshots_dir().join(format!("ptu-{n}.jpg")), b"");
    }
    let mut app = App::with(ctx);
    testkit::feed(&mut app, [app::Message::Navigate(Screen::Channels)]);
    app
}

/// A plan that reads like a real LIVE to PTU copy, with the big archive first.
fn demo_plan(app: &App) -> Box<CopyPlan> {
    let root = app.context().install.as_ref().unwrap().root.clone();
    let mut files = vec![PlannedFile {
        rel: PathBuf::from("Data.p4k"),
        size: 95 * GB,
        copy: true,
    }];
    files.extend((0..1203).map(|n| PlannedFile {
        rel: PathBuf::from(format!("Bin64/module_{n}.dll")),
        size: MB,
        copy: true,
    }));
    files.extend((0..312).map(|n| PlannedFile {
        rel: PathBuf::from(format!("Bin64/shared_{n}.dll")),
        size: MB,
        copy: false,
    }));
    Box::new(CopyPlan {
        source: root.join("LIVE"),
        target: root.join("PTU"),
        files,
        bytes_to_copy: 96 * GB + 410 * MB,
        largest_copy: 95 * GB,
        free_bytes: Some(148 * GB + 100 * MB),
        needs_in_place: false,
    })
}

fn show_plan(app: &mut App) {
    let plan = demo_plan(app);
    let _real_plan = app.update(app::Message::Channels(Message::PlanCopy));
    let _ = app.update(app::Message::Channels(Message::CopyPlanned(
        "LIVE".into(),
        "PTU".into(),
        false,
        Ok(plan),
    )));
}

fn shot(app: &App, name: &str) {
    let mut ui = Simulator::with_size(sc_app::settings(), FULL, app.view());
    testkit::snapshot(&mut ui, &app.theme(), name);
}

#[test]
fn channels_pictures() {
    for (choice, name) in [(ThemeChoice::Dark, "dark"), (ThemeChoice::Light, "light")] {
        let temp = tempfile::tempdir().unwrap();
        let mut app = demo_app(temp.path(), choice);
        show_plan(&mut app);
        assert!(
            Simulator::with_size(sc_app::settings(), FULL, app.view())
                .find("1,204 files, 96.4 GB to copy. 312 files already match and are skipped. 148 GB free on this disk.")
                .is_ok()
        );
        shot(&app, &format!("channels-{name}"));
        if choice == ThemeChoice::Dark {
            let _copy = app.update(app::Message::Channels(Message::StartCopy));
            let _ = app.update(app::Message::Channels(Message::CopyProgress(
                1,
                Progress {
                    files_done: 0,
                    files_total: 1204,
                    bytes_done: 41 * GB + 200 * MB,
                    bytes_total: 96 * GB + 410 * MB,
                },
            )));
            shot(&app, "channels-copying-dark");
            let _ = app.update(app::Message::Channels(Message::CancelCopy));
            let _ = app.update(app::Message::Channels(Message::Copied(
                1,
                Ok(core::CopyOutcome {
                    files_copied: 0,
                    bytes_copied: 0,
                    cancelled: true,
                }),
            )));
            testkit::feed(
                &mut app,
                [
                    app::Message::Channels(Message::DismissNotice),
                    app::Message::Channels(Message::AskSwap),
                ],
            );
            shot(&app, "channels-swap-confirm-dark");
        }
    }
}

const BUSY_BLOCKS: &str = "Wait until the copy or swap of game files has finished.";
const STOPPING: &str = "Stopping after the current file...";

#[test]
fn settings_actions_wait_for_game_file_jobs_and_unfinished_swaps() {
    let mut screen = Harness::loaded();
    screen.plan();
    let copy = screen.send_only(Message::StartCopy);
    assert!(screen.has(BUSY_BLOCKS));
    assert_eq!(screen.click("Copy settings"), 0);
    assert_eq!(screen.click("Swap settings"), 0);
    screen.run(copy);
    assert_eq!(screen.click("Copy settings"), 1);

    let mut screen = Harness::loaded();
    assert_eq!(screen.click("Swap game files"), 1);
    let swap = screen.send_only(Message::Swap);
    assert!(screen.has(BUSY_BLOCKS));
    assert_eq!(screen.click("Copy settings"), 0);
    assert_eq!(screen.click("Swap settings"), 0);
    screen.run(swap);

    let mut screen = Harness::new();
    interrupt_swap(&screen.ctx, 1);
    screen.reload();
    assert_eq!(screen.click("Copy settings"), 0);
    assert_eq!(screen.click("Swap settings"), 0);
}

#[test]
fn stop_and_switch_keeps_the_copy_locked_until_it_ends() {
    let temp = tempfile::tempdir().unwrap();
    let mut ctx = fixture(temp.path());
    let root = ctx.install.as_ref().unwrap().root.clone();
    ctx.settings.installs.push(root);
    let mut app = App::with(ctx);
    testkit::feed(
        &mut app,
        [
            app::Message::Navigate(Screen::Channels),
            app::Message::Channels(Message::PlanCopy),
        ],
    );
    let copy = app.update(app::Message::Channels(Message::StartCopy));
    testkit::feed(
        &mut app,
        [
            app::Message::PickChannel("PTU".into()),
            app::Message::DialogAction(0),
        ],
    );
    assert_eq!(app.context().channel.as_ref().unwrap().name, "PTU");
    assert!(app.dialog().is_none(), "asked once");
    let shows = |app: &App, text: &str| {
        Simulator::with_size(sc_app::settings(), TALL, app.view())
            .find(text)
            .is_ok()
    };
    assert!(shows(&app, STOPPING));
    testkit::feed(
        &mut app,
        [
            app::Message::Channels(Message::PlanCopy),
            app::Message::Channels(Message::AskSwap),
            app::Message::PickChannel("LIVE".into()),
        ],
    );
    assert!(shows(&app, STOPPING), "still locked");
    assert_eq!(app.context().channel.as_ref().unwrap().name, "LIVE");
    testkit::settle(&mut app, copy);
    assert!(!shows(&app, STOPPING));
    assert!(shows(&app, "Copy game files"));
}
