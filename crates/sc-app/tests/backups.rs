#[path = "../src/testkit.rs"]
mod testkit;

use iced::Size;
use iced_test::selector::Candidate;
use iced_test::simulator::Simulator;
use sc_app::app::{App, Message};
use sc_app::context::Context;
use sc_app::screen::{Screen, ToastKind};
use sc_app::screens::backups;
use sc_app::theme;
use sc_core::backup::{BackupStore, Scope, SnapshotFile, SnapshotId, SnapshotInfo, SnapshotState};
use sc_core::install::Channel;
use sc_core::process::{GameProbe, RunState};
use sc_core::settings::ThemeChoice;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use testkit::feed;

const FULL: Size = Size::new(1280.0, 800.0);

struct Switch(AtomicBool);

impl Switch {
    fn new(running: bool) -> Arc<Self> {
        Arc::new(Self(AtomicBool::new(running)))
    }

    fn set(&self, running: bool) {
        self.0.store(running, Ordering::SeqCst);
    }
}

impl GameProbe for Switch {
    fn game(&self) -> RunState {
        if self.0.load(Ordering::SeqCst) {
            RunState::Running
        } else {
            RunState::NotRunning
        }
    }

    fn launcher(&self) -> RunState {
        RunState::NotRunning
    }
}

fn context(temp: &Path) -> Context {
    testkit::context_with_channels(temp, &["LIVE"])
}

fn channel(ctx: &Context) -> Channel {
    ctx.channel.clone().unwrap()
}

fn scope(ctx: &Context) -> Scope {
    ctx.scope().unwrap()
}

fn store(ctx: &Context) -> BackupStore {
    ctx.writer().store().clone()
}

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn snapshot(ctx: &Context, files: &[PathBuf], label: &str) -> SnapshotInfo {
    store(ctx)
        .snapshot(&scope(ctx), &channel(ctx), files, label)
        .unwrap()
}

fn sim(app: &App) -> Simulator<'_, Message> {
    Simulator::with_size(sc_app::settings(), FULL, app.view())
}

fn texts_of<M>(mut ui: Simulator<'_, M>) -> Vec<String> {
    let mut found = Vec::new();
    let _ = ui.find(|candidate: Candidate<'_>| {
        if let Candidate::Text { content, .. } = candidate {
            found.push(content.to_string());
        }
        None::<()>
    });
    found
}

fn texts(app: &App) -> Vec<String> {
    texts_of(sim(app))
}

fn has(app: &App, text: &str) -> bool {
    sim(app).find(text).is_ok()
}

fn has_containing(app: &App, needle: &str) -> bool {
    texts(app).iter().any(|text| text.contains(needle))
}

fn clicked(app: &mut App, label: &str) {
    let mut ui = sim(app);
    ui.click(label)
        .unwrap_or_else(|err| panic!("{label}: {err}"));
    let messages: Vec<Message> = ui.into_messages().collect();
    feed(app, messages);
}

fn open_backups(app: &mut App) {
    feed(app, [Message::Navigate(Screen::Backups)]);
}

fn choose(app: &mut App, label: &str) {
    let index = app
        .dialog()
        .expect("a dialog is open")
        .actions
        .iter()
        .position(|action| action.label == label)
        .unwrap_or_else(|| panic!("no {label} action"));
    feed(app, [Message::DialogAction(index)]);
}

fn last_toast(app: &App) -> &sc_app::screen::Toast {
    app.toasts().last().expect("a toast")
}

#[test]
fn empty_store_shows_no_snapshots_yet() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::with(context(temp.path()));
    open_backups(&mut app);
    assert!(has(&app, "No snapshots yet"));
    assert!(has(
        &app,
        "star-config takes one before every change it makes."
    ));
    assert!(has(&app, "Snapshot now"));
    assert_eq!(app.pending(Screen::Backups), 0);
}

#[test]
fn snapshot_now_adds_a_row() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = context(temp.path());
    write(&channel(&ctx).user_cfg(), b"sys_MaxFPS = 60\n");
    let mut app = App::with(ctx.clone());
    open_backups(&mut app);
    clicked(&mut app, "Snapshot now");
    assert!(has(&app, backups::MANUAL_LABEL));
    let listed = store(&ctx).list(&scope(&ctx)).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].label, backups::MANUAL_LABEL);
    let managed = channel(&ctx).managed_files().len();
    assert_eq!(listed[0].files.len(), managed);
    assert!(
        listed[0]
            .files
            .iter()
            .any(|f| f.rel == "user.cfg" && f.existed)
    );
    assert_eq!(last_toast(&app).kind, ToastKind::Success);
}

#[test]
fn rows_are_newest_first_with_file_counts() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = context(temp.path());
    let live = channel(&ctx);
    write(&live.user_cfg(), b"a\n");
    snapshot(&ctx, &[live.user_cfg()], "Config changes");
    snapshot(
        &ctx,
        &[live.attributes_xml(), live.actionmaps_xml()],
        "Keybinds changes",
    );
    let mut app = App::with(ctx);
    open_backups(&mut app);
    let all = texts(&app);
    let position = |text: &str| all.iter().position(|t| t == text).unwrap();
    assert!(position("Keybinds changes") < position("Config changes"));
    assert!(all.iter().any(|t| t == "2 files"));
    assert!(all.iter().any(|t| t == "1 file"));
    assert!(all.iter().any(|t| t == "2 snapshots"));
}

#[test]
fn selecting_a_snapshot_shows_file_statuses() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = context(temp.path());
    let live = channel(&ctx);
    write(&live.user_cfg(), b"sys_MaxFPS = 60\n");
    snapshot(&ctx, &[live.user_cfg()], "Config changes");
    write(&live.attributes_xml(), b"<Attributes/>\n");
    snapshot(&ctx, &[live.attributes_xml()], "Head tracking changes");
    write(&live.user_cfg(), b"sys_MaxFPS = 144\n");
    let mut app = App::with(ctx);
    open_backups(&mut app);
    assert!(has(&app, "Same"));
    assert!(!has(&app, "Different"));
    clicked(&mut app, "Config changes");
    assert!(has(&app, "user.cfg"));
    assert!(has(&app, "Different"));
}

#[test]
fn file_deleted_since_shows_only_in_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = context(temp.path());
    let live = channel(&ctx);
    write(&live.actionmaps_xml(), b"<ActionMaps/>\n");
    snapshot(&ctx, &[live.actionmaps_xml()], "Keybinds changes");
    fs::remove_file(live.actionmaps_xml()).unwrap();
    let mut app = App::with(ctx);
    open_backups(&mut app);
    assert!(has(&app, "Only in snapshot"));
    assert!(has(&app, "USER/Client/0/Profiles/default/actionmaps.xml"));
}

#[test]
fn file_that_did_not_exist_then_says_so() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = context(temp.path());
    let live = channel(&ctx);
    snapshot(&ctx, &[live.attributes_xml()], "Head tracking changes");
    write(&live.attributes_xml(), b"<Attributes/>\n");
    let mut app = App::with(ctx);
    open_backups(&mut app);
    assert!(has(&app, "Did not exist then"));
}

#[test]
fn diff_shows_current_lines_as_removed_and_snapshot_lines_as_added() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = context(temp.path());
    let live = channel(&ctx);
    write(&live.user_cfg(), b"r_DisplayInfo = 0\nsys_MaxFPS = 60\n");
    snapshot(&ctx, &[live.user_cfg()], "Config changes");
    write(&live.user_cfg(), b"r_DisplayInfo = 0\nsys_MaxFPS = 144\n");
    let mut app = App::with(ctx);
    open_backups(&mut app);
    assert!(has(&app, backups::DIFF_CAPTION));
    let all = texts(&app);
    let sign_before = |line: &str| {
        let index = all
            .iter()
            .position(|t| t == line)
            .unwrap_or_else(|| panic!("{line} not shown"));
        all[index - 1].clone()
    };
    assert_eq!(sign_before("sys_MaxFPS = 144"), "-");
    assert_eq!(sign_before("sys_MaxFPS = 60"), "+");
    assert_eq!(sign_before("r_DisplayInfo = 0"), "");
}

#[test]
fn large_or_binary_file_shows_status_only_message() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = context(temp.path());
    let live = channel(&ctx);
    write(&live.user_cfg(), b"\xff\xfe\x00binary");
    snapshot(&ctx, &[live.user_cfg()], "Config changes");
    write(&live.user_cfg(), b"\xff\xfe\x00other");
    let mut app = App::with(ctx);
    open_backups(&mut app);
    assert!(has(&app, "Different"));
    assert!(has(&app, backups::NOT_TEXT));
}

fn restore_setup(temp: &Path, probe: Arc<Switch>) -> (Context, App, PathBuf) {
    let ctx = context(temp).with_probe(probe);
    let cfg = channel(&ctx).user_cfg();
    write(&cfg, b"sys_MaxFPS = 60\n");
    snapshot(&ctx, std::slice::from_ref(&cfg), "Config changes");
    write(&cfg, b"sys_MaxFPS = 144\n");
    let mut app = App::with(ctx.clone());
    open_backups(&mut app);
    (ctx, app, cfg)
}

fn restore_through_the_screen(app: &mut App) {
    clicked(app, "Restore");
    let dialog = app.dialog().expect("restore asks first");
    assert!(
        dialog.title.starts_with("Restore 1 file to how it was on "),
        "{}",
        dialog.title
    );
    assert_eq!(dialog.body, "Your current files are snapshotted first.");
    choose(app, "Restore");
}

#[test]
fn restore_asks_then_restores_and_offers_undo() {
    let temp = tempfile::tempdir().unwrap();
    let (ctx, mut app, cfg) = restore_setup(temp.path(), Switch::new(false));
    restore_through_the_screen(&mut app);
    assert_eq!(fs::read(&cfg).unwrap(), b"sys_MaxFPS = 60\n");
    let toast = last_toast(&app);
    assert_eq!(toast.kind, ToastKind::Success);
    let undo = toast.undo.clone().expect("the toast offers undo");
    let listed = store(&ctx).list(&scope(&ctx)).unwrap();
    assert_eq!(listed[0].id, undo);
    assert!(has_containing(&app, "Before restoring"));
}

#[test]
fn undo_after_restore_brings_back_the_newer_content() {
    let temp = tempfile::tempdir().unwrap();
    let (_ctx, mut app, cfg) = restore_setup(temp.path(), Switch::new(false));
    restore_through_the_screen(&mut app);
    let index = app.toasts().len() - 1;
    feed(&mut app, [Message::UndoToast(index)]);
    assert_eq!(fs::read(&cfg).unwrap(), b"sys_MaxFPS = 144\n");
    assert_eq!(last_toast(&app).kind, ToastKind::Success);
}

#[test]
fn restore_disabled_when_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let mut ctx = context(temp.path()).with_probe(Switch::new(true));
    ctx.game = RunState::Running;
    let cfg = channel(&ctx).user_cfg();
    write(&cfg, b"old\n");
    snapshot(&ctx, std::slice::from_ref(&cfg), "Config changes");
    write(&cfg, b"new\n");
    let mut app = App::with(ctx);
    open_backups(&mut app);
    clicked(&mut app, "Restore");
    assert!(app.dialog().is_none());
    assert_eq!(fs::read(&cfg).unwrap(), b"new\n");
    clicked(&mut app, "Snapshot now");
    assert!(has(&app, backups::MANUAL_LABEL));
}

#[test]
fn restore_is_refused_by_the_core_when_the_game_runs() {
    let temp = tempfile::tempdir().unwrap();
    let mut ctx = context(temp.path()).with_probe(Switch::new(true));
    ctx.game = RunState::Running;
    let cfg = channel(&ctx).user_cfg();
    write(&cfg, b"old\n");
    let info = snapshot(&ctx, std::slice::from_ref(&cfg), "Config changes");
    write(&cfg, b"new\n");
    let mut app = App::with(ctx);
    open_backups(&mut app);
    feed(
        &mut app,
        [Message::Backups(backups::Message::Restore(info.id))],
    );
    assert_eq!(fs::read(&cfg).unwrap(), b"new\n");
    let toast = last_toast(&app);
    assert_eq!(toast.kind, ToastKind::Error);
    assert_eq!(toast.text, backups::CLOSE_GAME);
}

#[test]
fn undo_from_a_toast_is_refused_when_the_game_runs() {
    let temp = tempfile::tempdir().unwrap();
    let probe = Switch::new(false);
    let (_ctx, mut app, cfg) = restore_setup(temp.path(), probe.clone());
    restore_through_the_screen(&mut app);
    probe.set(true);
    let index = app.toasts().len() - 1;
    feed(&mut app, [Message::UndoToast(index)]);
    assert_eq!(fs::read(&cfg).unwrap(), b"sys_MaxFPS = 60\n");
    let toast = last_toast(&app);
    assert_eq!(toast.kind, ToastKind::Error);
    assert_eq!(toast.text, backups::CLOSE_GAME);
}

#[test]
fn delete_asks_then_removes() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = context(temp.path());
    snapshot(&ctx, &[channel(&ctx).user_cfg()], "Config changes");
    let mut app = App::with(ctx.clone());
    open_backups(&mut app);
    clicked(&mut app, "Delete");
    let dialog = app.dialog().expect("delete asks first");
    assert_eq!(dialog.title, "Delete this snapshot?");
    assert!(dialog.body.ends_with("This cannot be undone."));
    choose(&mut app, "Delete");
    assert!(store(&ctx).list(&scope(&ctx)).unwrap().is_empty());
    assert!(has(&app, "No snapshots yet"));
}

fn interrupted_setup(temp: &Path) -> (Context, PathBuf, SnapshotInfo) {
    let ctx = context(temp);
    let cfg = channel(&ctx).user_cfg();
    write(&cfg, b"old\n");
    let begun = store(&ctx)
        .begin(
            &scope(&ctx),
            &channel(&ctx),
            std::slice::from_ref(&cfg),
            "Config changes",
        )
        .unwrap();
    write(&cfg, b"half written\n");
    (ctx, cfg, begun)
}

#[test]
fn interrupted_snapshot_prompts_at_startup() {
    let temp = tempfile::tempdir().unwrap();
    let (ctx, _cfg, _begun) = interrupted_setup(temp.path());
    let app = App::with(ctx);
    assert!(has(&app, "A change was interrupted"));
    let dialog = app.dialog().unwrap();
    assert!(
        dialog
            .body
            .starts_with("star-config was writing 1 file on ")
    );
    assert!(
        dialog
            .body
            .ends_with(" and did not finish. Restore it to how it was?")
    );
}

#[test]
fn restore_from_the_startup_prompt_puts_the_files_back() {
    let temp = tempfile::tempdir().unwrap();
    let (ctx, cfg, _begun) = interrupted_setup(temp.path());
    let mut app = App::with(ctx.clone());
    choose(&mut app, "Restore");
    assert_eq!(fs::read(&cfg).unwrap(), b"old\n");
    assert!(store(&ctx).interrupted().unwrap().is_empty());
    assert_eq!(last_toast(&app).kind, ToastKind::Success);
}

#[test]
fn keep_as_is_marks_snapshot_done() {
    let temp = tempfile::tempdir().unwrap();
    let (ctx, cfg, begun) = interrupted_setup(temp.path());
    let mut app = App::with(ctx.clone());
    choose(&mut app, "Keep as is");
    assert!(app.dialog().is_none());
    assert!(store(&ctx).interrupted().unwrap().is_empty());
    let listed = store(&ctx).list(&scope(&ctx)).unwrap();
    assert_eq!(listed[0].id, begun.id);
    assert_eq!(listed[0].state, SnapshotState::Done);
    assert_eq!(fs::read(&cfg).unwrap(), b"half written\n");
}

#[test]
fn decide_later_leaves_it_and_the_list_shows_interrupted() {
    let temp = tempfile::tempdir().unwrap();
    let (ctx, _cfg, _begun) = interrupted_setup(temp.path());
    let mut app = App::with(ctx.clone());
    choose(&mut app, "Decide later");
    assert!(app.dialog().is_none());
    assert_eq!(store(&ctx).interrupted().unwrap().len(), 1);
    open_backups(&mut app);
    assert!(has(&app, "Interrupted"));
}

#[test]
fn snapshots_beyond_the_keep_count_are_pruned() {
    let temp = tempfile::tempdir().unwrap();
    let mut ctx = context(temp.path());
    ctx.settings.snapshot_keep = 5;
    let cfg = channel(&ctx).user_cfg();
    write(&cfg, b"old\n");
    for label in ["One", "Two", "Three", "Four", "Five", "Six"] {
        snapshot(&ctx, std::slice::from_ref(&cfg), label);
    }
    let mut app = App::with(ctx.clone());
    open_backups(&mut app);
    clicked(&mut app, "Snapshot now");
    let listed = store(&ctx).list(&scope(&ctx)).unwrap();
    let labels: Vec<&str> = listed.iter().map(|info| info.label.as_str()).collect();
    assert_eq!(
        labels,
        [backups::MANUAL_LABEL, "Six", "Five", "Four", "Three"]
    );
    assert!(has(&app, "5 snapshots"));
}

#[test]
fn a_keep_count_below_the_minimum_still_keeps_the_minimum() {
    let temp = tempfile::tempdir().unwrap();
    let mut ctx = context(temp.path());
    ctx.settings.snapshot_keep = 0;
    let cfg = channel(&ctx).user_cfg();
    write(&cfg, b"old\n");
    for label in ["One", "Two", "Three", "Four", "Five", "Six"] {
        snapshot(&ctx, std::slice::from_ref(&cfg), label);
    }
    let mut app = App::with(ctx.clone());
    open_backups(&mut app);
    clicked(&mut app, "Snapshot now");
    let listed = store(&ctx).list(&scope(&ctx)).unwrap();
    assert_eq!(listed.len(), 5);
    assert_eq!(listed[0].label, backups::MANUAL_LABEL);
}

#[test]
fn a_restore_prunes_too() {
    let temp = tempfile::tempdir().unwrap();
    let mut ctx = context(temp.path());
    ctx.settings.snapshot_keep = 5;
    let cfg = channel(&ctx).user_cfg();
    write(&cfg, b"old\n");
    for label in ["One", "Two", "Three", "Four", "Five"] {
        snapshot(&ctx, std::slice::from_ref(&cfg), label);
    }
    write(&cfg, b"new\n");
    let mut app = App::with(ctx.clone());
    open_backups(&mut app);
    clicked(&mut app, "Restore");
    choose(&mut app, "Restore");
    let listed = store(&ctx).list(&scope(&ctx)).unwrap();
    assert_eq!(listed.len(), 5);
    assert!(listed[0].label.starts_with("Before restore of "));
    assert_eq!(listed[1].label, "Five");
    assert_eq!(listed[4].label, "Two");
}

fn fake(index: usize) -> SnapshotInfo {
    SnapshotInfo {
        id: SnapshotId(format!("20261115T120000Z-{index}")),
        created_unix: 1_763_208_000 - index as i64 * 60,
        label: format!("Snapshot {index}"),
        state: SnapshotState::Done,
        files: vec![SnapshotFile {
            rel: "user.cfg".into(),
            existed: true,
            size: 1,
        }],
    }
}

#[test]
fn five_thousand_snapshots_build_only_visible_rows() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = context(temp.path());
    let mut state = backups::State::new();
    let _ = state.reload(&ctx);
    let _ = state.update(
        backups::Message::Loaded(scope(&ctx), Ok((0..5000).map(fake).collect())),
        &ctx,
    );
    let ui = Simulator::with_size(sc_app::settings(), FULL, state.view(&ctx));
    let all = texts_of(ui);
    let rows: std::collections::HashSet<&String> = all
        .iter()
        .filter(|text| text.starts_with("Snapshot "))
        .collect();
    assert!(all.iter().any(|t| t == "5000 snapshots"));
    assert!(all.iter().any(|t| t == "Snapshot 0"));
    assert!(!all.iter().any(|t| t == "Snapshot 4999"));
    assert!(rows.len() <= 20, "built {} rows", rows.len());
}

fn set_time(ctx: &Context, info: &SnapshotInfo, unix: i64) {
    let manifest = ctx
        .dirs
        .backups()
        .join(&scope(ctx).install_key)
        .join(&scope(ctx).channel)
        .join(&info.id.0)
        .join("manifest.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    let rewritten: Vec<String> = text
        .lines()
        .map(|line| {
            if line.starts_with("created_unix = ") {
                format!("created_unix = {unix}")
            } else {
                line.to_string()
            }
        })
        .collect();
    fs::write(&manifest, rewritten.join("\n") + "\n").unwrap();
}

const USER_CFG_THEN: &str = "\
; Display
r_DisplayInfo = 0
r_VSync = 0
sys_MaxFPS = 60
r_Gamma = 1.0
cl_fov = 90
g_language = english
";

const USER_CFG_NOW: &str = "\
; Display
r_DisplayInfo = 0
r_VSync = 0
sys_MaxFPS = 144
r_Gamma = 1.0
cl_fov = 100
g_language = english
";

fn demo(temp: &Path) -> App {
    let ctx = context(temp);
    let mut app = App::with(ctx.clone());
    let live = channel(&ctx);
    let now = chrono::Utc::now().timestamp();
    let restored_at = chrono::Utc::now() - chrono::Duration::days(15);
    let restored_label = format!("Before restore of {}", restored_at.format("%Y%m%dT%H%M%SZ"));
    write(&live.user_cfg(), USER_CFG_THEN.as_bytes());
    write(
        &live.attributes_xml(),
        b"<Attributes Version=\"1\">\n <Attr name=\"HeadtrackingEnableRoll\" value=\"1\"/>\n</Attributes>\n",
    );
    write(&live.actionmaps_xml(), b"<ActionMaps version=\"1\"/>\n");
    let older = [
        (
            vec![
                live.user_cfg(),
                live.attributes_xml(),
                live.actionmaps_xml(),
            ],
            restored_label.as_str(),
            12 * 86_400,
            false,
        ),
        (
            vec![live.global_ini()],
            "StarStrings install",
            9 * 86_400,
            true,
        ),
        (
            vec![live.attributes_xml()],
            "Head tracking and comms changes",
            3 * 86_400 + 4_000,
            false,
        ),
        (
            live.managed_files(),
            backups::MANUAL_LABEL,
            86_400 + 2_000,
            false,
        ),
        (
            vec![live.actionmaps_xml()],
            "Keybinds changes",
            3 * 3_600,
            false,
        ),
    ];
    for (files, label, age, interrupted) in older {
        let info = if interrupted {
            store(&ctx)
                .begin(&scope(&ctx), &live, &files, label)
                .unwrap()
        } else {
            snapshot(&ctx, &files, label)
        };
        set_time(&ctx, &info, now - age);
    }
    let selected = snapshot(
        &ctx,
        &[
            live.user_cfg(),
            live.attributes_xml(),
            live.actionmaps_xml(),
        ],
        "Config changes",
    );
    set_time(&ctx, &selected, now - 25 * 60);
    write(&live.user_cfg(), USER_CFG_NOW.as_bytes());
    fs::remove_file(live.actionmaps_xml()).unwrap();
    open_backups(&mut app);
    app
}

#[test]
fn backups_pictures() {
    let temp = tempfile::tempdir().unwrap();
    let app = demo(temp.path());
    assert!(has(&app, "Config changes"));
    assert!(has(&app, "Different"));
    assert!(has(&app, "Same"));
    assert!(has(&app, "Only in snapshot"));
    assert!(has(&app, "sys_MaxFPS = 144"));
    for (choice, name) in [
        (ThemeChoice::Dark, "backups-dark"),
        (ThemeChoice::Light, "backups-light"),
    ] {
        let mut ui = sim(&app);
        testkit::snapshot(&mut ui, &theme::theme(choice), name);
    }
}
