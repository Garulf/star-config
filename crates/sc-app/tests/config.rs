#[path = "../src/testkit.rs"]
mod testkit;

use iced::Size;
use iced::widget::text_editor::{Action as EditorAction, Edit, Motion};
use iced_test::selector::Candidate;
use iced_test::simulator::Simulator;
use sc_app::app::{self, App, Message};
use sc_app::screen::Screen;
use sc_app::screens::config::{self, Tab};
use sc_core::backup::BackupStore;
use sc_core::install::Channel;
use sc_core::settings::ThemeChoice;
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;
use testkit::{DEMO_USER_CFG, ctrl, demo_attributes, feed};

const FULL: Size = Size::new(1280.0, 800.0);
const NOT_RECOGNISED: &str =
    "This file's format is not recognised, so star-config will not change it.";

struct Fixture {
    _temp: TempDir,
    app: App,
    channel: Channel,
}

fn fixture(user_cfg: Option<&[u8]>, attributes: Option<&[u8]>) -> Fixture {
    fixture_with(user_cfg, attributes, ThemeChoice::Dark)
}

fn fixture_with(user_cfg: Option<&[u8]>, attributes: Option<&[u8]>, theme: ThemeChoice) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let mut ctx = testkit::context_with_channels(temp.path(), &["LIVE"]);
    ctx.settings.theme = theme;
    let channel = ctx.channel.clone().unwrap();
    testkit::write_channel_files(&channel, user_cfg, attributes);
    let mut app = App::with(ctx);
    feed(&mut app, [Message::Navigate(Screen::Config)]);
    Fixture {
        _temp: temp,
        app,
        channel,
    }
}

fn demo() -> Fixture {
    fixture(
        Some(DEMO_USER_CFG.as_bytes()),
        Some(demo_attributes().as_bytes()),
    )
}

fn send(fx: &mut Fixture, messages: impl IntoIterator<Item = config::Message>) {
    feed(&mut fx.app, messages.into_iter().map(Message::Config));
}

fn set(key: &str, value: &str) -> config::Message {
    config::Message::SetCvar(key.into(), value.into())
}

fn apply(fx: &mut Fixture) {
    feed(&mut fx.app, [Message::Apply]);
}

fn sim(app: &App) -> Simulator<'_, Message> {
    Simulator::with_size(sc_app::settings(), FULL, app.view())
}

fn has(app: &App, needle: &str) -> bool {
    sim(app).find(needle).is_ok()
}

fn has_part(app: &App, needle: &str) -> bool {
    sim(app)
        .find(|candidate: Candidate<'_>| match candidate {
            Candidate::Text { content, .. } if content.contains(needle) => Some(()),
            _ => None,
        })
        .is_ok()
}

fn user_cfg(fx: &Fixture) -> Vec<u8> {
    fs::read(fx.channel.user_cfg()).unwrap()
}

fn attributes(fx: &Fixture) -> String {
    fs::read_to_string(fx.channel.attributes_xml()).unwrap()
}

fn pending(fx: &Fixture) -> usize {
    fx.app.pending(Screen::Config)
}

#[test]
fn catalogue_cvars_render_with_their_groups() {
    let fx = demo();
    for text in [
        "HEAD TRACKING",
        "GRAPHICS",
        "Field of view",
        "Motion blur",
        "cl_fov",
    ] {
        assert!(has(&fx.app, text), "{text}");
    }
}

#[test]
fn toggling_a_cvar_stages_one_edit_and_marks_the_row() {
    let mut fx = demo();
    send(&mut fx, [set("r_filmgrain", "1")]);
    assert_eq!(pending(&fx), 1);
    assert!(has(&fx.app, "Was 0"));
    assert!(has(&fx.app, "1 unsaved change"));
}

#[test]
fn typing_in_one_field_is_one_edit() {
    let mut fx = demo();
    send(
        &mut fx,
        [
            set("cl_fov", "1"),
            set("cl_fov", "10"),
            set("cl_fov", "100"),
        ],
    );
    assert_eq!(pending(&fx), 1);
    assert!(has(&fx.app, "100"));
    feed(
        &mut fx.app,
        [Message::Shortcut(app::shortcut(&ctrl("z")).unwrap())],
    );
    assert_eq!(pending(&fx), 0);
}

#[test]
fn apply_writes_user_cfg_and_keeps_other_lines() {
    let mut fx = demo();
    send(&mut fx, [set("cl_fov", "100")]);
    apply(&mut fx);
    let want = DEMO_USER_CFG.replace("cl_fov = 90\n", "cl_fov = 100\n");
    assert_eq!(String::from_utf8(user_cfg(&fx)).unwrap(), want);
    assert_eq!(pending(&fx), 0);
}

#[test]
fn apply_returns_a_toast_with_undo() {
    let mut fx = demo();
    send(&mut fx, [set("cl_fov", "100")]);
    apply(&mut fx);
    let toast = fx.app.toasts().last().expect("a toast after apply");
    assert_eq!(toast.text, "Applied 1 change");
    assert!(toast.undo.is_some());
}

#[test]
fn remove_sets_a_cvar_back_to_not_set() {
    let mut fx = demo();
    send(
        &mut fx,
        [config::Message::RemoveCvar("r_MotionBlur".into())],
    );
    assert!(has(&fx.app, "Not set. The game uses its default."));
    assert!(has(&fx.app, "Was 0"));
    apply(&mut fx);
    let text = String::from_utf8(user_cfg(&fx)).unwrap();
    assert!(!text.contains("r_MotionBlur"), "{text}");
    assert_eq!(text, DEMO_USER_CFG.replace("r_MotionBlur = 0\n", ""));
}

#[test]
fn add_a_line_then_apply() {
    let mut fx = demo();
    send(
        &mut fx,
        [
            config::Message::NewName("r_VSync".into()),
            config::Message::NewValue("0".into()),
            config::Message::AddLine,
            config::Message::NewName("sys_MaxFPS".into()),
            config::Message::NewValue("120".into()),
            config::Message::AddLine,
        ],
    );
    assert_eq!(pending(&fx), 2);
    apply(&mut fx);
    let text = String::from_utf8(user_cfg(&fx)).unwrap();
    assert!(text.ends_with("r_VSync = 0\n"), "{text}");
    assert_eq!(text.matches("sys_MaxFPS").count(), 1, "{text}");
    assert!(text.contains("sys_MaxFPS = 120\n"), "{text}");
}

#[test]
fn missing_user_cfg_is_created_on_apply() {
    let mut fx = fixture(None, Some(demo_attributes().as_bytes()));
    assert!(has(
        &fx.app,
        "This channel has no user.cfg yet. Applying a change creates it."
    ));
    assert!(has(&fx.app, "Not set. The game uses its default."));
    send(&mut fx, [set("cl_fov", "95")]);
    apply(&mut fx);
    let text = String::from_utf8(user_cfg(&fx)).unwrap();
    assert!(text.contains("cl_fov = 95"), "{text}");
}

fn replace_text(text: &str) -> [config::Message; 2] {
    [
        config::Message::TextEdited(EditorAction::SelectAll),
        config::Message::TextEdited(EditorAction::Edit(Edit::Paste(Arc::new(text.into())))),
    ]
}

#[test]
fn text_view_edit_round_trips() {
    let mut fx = demo();
    send(&mut fx, [config::Message::TextView(true)]);
    assert!(
        !has(&fx.app, "Field of view"),
        "the text view replaces the rows"
    );
    let edited = DEMO_USER_CFG.replace("cl_fov = 90", "cl_fov = 105");
    send(&mut fx, replace_text(&edited));
    send(&mut fx, [config::Message::TextView(false)]);
    assert!(has(&fx.app, "Field of view"), "back on the settings view");
    assert!(has(&fx.app, "105"));
    assert!(has(&fx.app, "Was 90"));
    assert_eq!(pending(&fx), 1);
    apply(&mut fx);
    assert_eq!(String::from_utf8(user_cfg(&fx)).unwrap(), edited);
}

#[test]
fn unparsable_text_stays_in_text_view_with_the_line_number() {
    let mut fx = demo();
    send(&mut fx, [config::Message::TextView(true)]);
    send(&mut fx, replace_text("cl_fov = 90\nnot a setting\n"));
    send(&mut fx, [config::Message::TextView(false)]);
    assert!(has_part(&fx.app, "Line 2: this is not a setting."));
    assert!(!has(&fx.app, "Field of view"), "still in the text view");
    let before = user_cfg(&fx);
    apply(&mut fx);
    assert_eq!(user_cfg(&fx), before, "apply refuses the bad text");
    assert!(pending(&fx) > 0);
}

#[test]
fn non_utf8_file_has_a_read_only_text_view_and_row_edits_keep_its_bytes() {
    let mut bytes = b"; caf\xe9 settings\r\ncl_fov = 90\r\n".to_vec();
    let mut fx = fixture(Some(&bytes), None);
    send(&mut fx, [config::Message::TextView(true)]);
    assert!(has(
        &fx.app,
        "This file has characters that are not UTF-8, so it can only be edited as settings, not as text."
    ));
    send(&mut fx, replace_text("cl_fov = 1\n"));
    assert_eq!(pending(&fx), 0, "the text view is read-only");
    send(
        &mut fx,
        [config::Message::TextView(false), set("cl_fov", "100")],
    );
    apply(&mut fx);
    let at = bytes.windows(2).position(|w| w == b"90").unwrap();
    bytes.splice(at..at + 2, b"100".iter().copied());
    assert_eq!(user_cfg(&fx), bytes);
}

#[test]
fn search_filters_rows() {
    let mut fx = demo();
    send(&mut fx, [config::Message::Search("grain".into())]);
    assert!(has(&fx.app, "Film grain"));
    assert!(!has(&fx.app, "Field of view"));
    send(&mut fx, [config::Message::Search("pl_pit".into())]);
    assert!(has(&fx.app, "pl_pit.forceSoftwareCursor"));
    assert!(!has(&fx.app, "Film grain"));
    send(
        &mut fx,
        [config::Message::Search("server population".into())],
    );
    assert!(
        has(&fx.app, "Performance overlay"),
        "matches the description"
    );
    send(&mut fx, [config::Message::Search("zzzz".into())]);
    assert!(has(&fx.app, "Nothing matches"));
}

fn numbered_attributes(count: usize) -> String {
    let entries: Vec<(String, String)> = (0..count)
        .map(|i| (format!("Setting{i:03}"), i.to_string()))
        .collect();
    testkit::attributes_xml(&entries)
}

#[test]
fn game_settings_lists_every_attribute_and_filters() {
    let mut fx = fixture(None, Some(numbered_attributes(300).as_bytes()));
    send(&mut fx, [config::Message::Tab(Tab::Game)]);
    assert!(has(&fx.app, "300 settings"));
    assert!(has(
        &fx.app,
        "Every setting the game stores for this channel. Head tracking and comms have their own screen."
    ));
    assert!(has(&fx.app, "Setting000"));
    send(&mut fx, [config::Message::Search("setting01".into())]);
    assert!(has(&fx.app, "10 of 300"));
    assert!(has(&fx.app, "Setting015"));
    assert!(!has(&fx.app, "Setting000"));
}

#[test]
fn editing_an_attribute_changes_only_that_value_on_disk() {
    let original = demo_attributes();
    let mut fx = fixture(Some(DEMO_USER_CFG.as_bytes()), Some(original.as_bytes()));
    send(
        &mut fx,
        [
            config::Message::Tab(Tab::Game),
            config::Message::SetAttr("FOV".into(), "100".into()),
        ],
    );
    assert_eq!(pending(&fx), 1);
    apply(&mut fx);
    let want = original.replace(
        "<Attr name=\"FOV\" value=\"90\"/>",
        "<Attr name=\"FOV\" value=\"100\"/>",
    );
    assert_ne!(want, original);
    assert_eq!(attributes(&fx), want);
    assert_eq!(user_cfg(&fx), DEMO_USER_CFG.as_bytes());
}

#[test]
fn unrecognised_attributes_file_is_read_only() {
    let original = format!("<!DOCTYPE Attributes>\n{}", demo_attributes());
    let mut fx = fixture(None, Some(original.as_bytes()));
    send(&mut fx, [config::Message::Tab(Tab::Game)]);
    assert!(has(&fx.app, NOT_RECOGNISED));
    send(
        &mut fx,
        [config::Message::SetAttr("FOV".into(), "100".into())],
    );
    assert_eq!(pending(&fx), 0);
    apply(&mut fx);
    assert_eq!(attributes(&fx), original);
}

#[test]
fn one_apply_with_both_files_changed_makes_one_snapshot() {
    let mut fx = demo();
    send(
        &mut fx,
        [
            set("cl_fov", "100"),
            config::Message::SetAttr("FOV".into(), "100".into()),
        ],
    );
    assert_eq!(pending(&fx), 2);
    apply(&mut fx);
    let ctx = fx.app.context();
    let snapshots = BackupStore::new(ctx.dirs.backups())
        .list(&ctx.scope().unwrap())
        .unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].label, "Config changes");
    assert_eq!(snapshots[0].files.len(), 2);
    assert!(
        String::from_utf8(user_cfg(&fx))
            .unwrap()
            .contains("cl_fov = 100")
    );
    assert!(attributes(&fx).contains("<Attr name=\"FOV\" value=\"100\"/>"));
    assert_eq!(fx.app.toasts().last().unwrap().text, "Applied 2 changes");
}

#[test]
fn apply_replays_edits_when_user_cfg_changed_on_disk() {
    let mut fx = demo();
    send(&mut fx, [set("cl_fov", "100")]);
    let outside = format!("{DEMO_USER_CFG}r_ssdo = 2\n");
    fs::write(fx.channel.user_cfg(), &outside).unwrap();
    apply(&mut fx);
    let text = String::from_utf8(user_cfg(&fx)).unwrap();
    assert_eq!(text, outside.replace("cl_fov = 90", "cl_fov = 100"));
    assert_eq!(pending(&fx), 0);
}

#[test]
fn discard_clears_both_tabs() {
    let mut fx = demo();
    send(
        &mut fx,
        [
            set("cl_fov", "100"),
            config::Message::SetAttr("FOV".into(), "100".into()),
        ],
    );
    feed(&mut fx.app, [Message::Discard]);
    assert_eq!(pending(&fx), 0);
    assert!(!has(&fx.app, "Was 90"));
    assert_eq!(user_cfg(&fx), DEMO_USER_CFG.as_bytes());
}

#[test]
fn ctrl_z_undoes_last_edit() {
    let mut fx = demo();
    let undo = Message::Shortcut(app::shortcut(&ctrl("z")).unwrap());
    send(&mut fx, [set("cl_fov", "100"), set("r_filmgrain", "1")]);
    feed(&mut fx.app, [undo.clone()]);
    assert_eq!(pending(&fx), 1);
    assert!(has(&fx.app, "Was 90"), "the earlier edit stays");
    assert!(!has(&fx.app, "Was 0"));

    send(
        &mut fx,
        [
            config::Message::Tab(Tab::Game),
            config::Message::SetAttr("FOV".into(), "100".into()),
        ],
    );
    assert_eq!(pending(&fx), 2);
    feed(&mut fx.app, [undo]);
    assert_eq!(
        pending(&fx),
        1,
        "undo on the game tab takes back the attribute"
    );
}

#[test]
fn snapshots() {
    let edits = [
        set("cl_fov", "100"),
        set("g_headtrackingToggleAutoRecalibrate", "0"),
        set("g_headtrackingGlobalSmoothingRotation", "0.55"),
    ];
    for (theme, name) in [(ThemeChoice::Dark, "dark"), (ThemeChoice::Light, "light")] {
        let mut fx = fixture_with(
            Some(DEMO_USER_CFG.as_bytes()),
            Some(demo_attributes().as_bytes()),
            theme,
        );
        send(&mut fx, edits.clone());
        {
            let mut ui = sim(&fx.app);
            testkit::snapshot(&mut ui, &fx.app.theme(), &format!("config-{name}"));
        }
        if theme == ThemeChoice::Dark {
            send(
                &mut fx,
                [
                    config::Message::Tab(Tab::Game),
                    config::Message::SetAttr("AudioMusicVolume".into(), "0.5".into()),
                ],
            );
            let mut ui = sim(&fx.app);
            testkit::snapshot(&mut ui, &fx.app.theme(), "config-game-settings-dark");
        }
    }
}

#[test]
fn apply_result_after_a_channel_switch_stays_with_the_first_channel() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::context_with_channels(temp.path(), &["LIVE", "PTU"]);
    let live = ctx.channel.clone().unwrap();
    let ptu = ctx
        .channels
        .iter()
        .find(|c| c.name == "PTU")
        .unwrap()
        .clone();
    testkit::write_channel_files(&live, Some(b"r_displayInfo = 0\n"), None);
    testkit::write_channel_files(&ptu, Some(b"r_displayInfo = 2\n"), None);
    let mut app = App::with(ctx);
    feed(&mut app, [Message::Navigate(Screen::Config)]);
    feed(&mut app, [Message::Config(set("r_displayInfo", "1"))]);
    let applying = app.update(Message::Apply);
    feed(
        &mut app,
        [Message::PickChannel("PTU".into()), Message::DialogAction(0)],
    );
    assert_eq!(app.context().channel.as_ref().unwrap().name, "PTU");
    testkit::settle(&mut app, applying);

    assert_eq!(
        fs::read_to_string(live.user_cfg()).unwrap(),
        "r_displayInfo = 1\n"
    );
    assert_eq!(
        fs::read_to_string(ptu.user_cfg()).unwrap(),
        "r_displayInfo = 2\n"
    );
    assert_eq!(app.pending(Screen::Config), 0);
    let index = app.toasts().len() - 1;
    let toast = &app.toasts()[index];
    assert_eq!(toast.undo_in.len(), 1);
    assert_eq!(toast.undo_in[0].channel.name, "LIVE");

    let id = app.toast_id(index).unwrap();

    feed(&mut app, [Message::UndoToast(id)]);
    assert_eq!(
        fs::read_to_string(live.user_cfg()).unwrap(),
        "r_displayInfo = 0\n"
    );
    assert_eq!(
        app.toasts().last().unwrap().text,
        "Restored the previous files"
    );
}

#[test]
fn text_view_edit_never_overwrites_an_outside_change() {
    let mut fx = fixture(Some(b"cl_fov = 90\n"), None);
    send(&mut fx, [config::Message::TextView(true)]);
    send(&mut fx, replace_text("cl_fov = 100\n"));
    let outside = "cl_fov = 90\ng_language = english\n";
    fs::write(fx.channel.user_cfg(), outside).unwrap();
    apply(&mut fx);
    assert_eq!(String::from_utf8(user_cfg(&fx)).unwrap(), outside);
    assert!(pending(&fx) > 0, "the edits are kept");
    assert_eq!(
        fx.app.toasts().last().unwrap().text,
        "The file changed on disk. Your edits were kept. Check them and apply again."
    );
}

#[test]
fn text_view_edit_keeps_line_endings_and_the_bom() {
    let original = b"\xEF\xBB\xBF; header\r\nr_VSync = 1\r\n\r\ncl_fov = 90\r\n";
    let mut fx = fixture(Some(original), None);
    send(
        &mut fx,
        [
            config::Message::TextView(true),
            config::Message::TextEdited(EditorAction::Move(Motion::Down)),
            config::Message::TextEdited(EditorAction::Move(Motion::End)),
            config::Message::TextEdited(EditorAction::Edit(Edit::Insert('0'))),
        ],
    );
    assert_eq!(pending(&fx), 1);
    apply(&mut fx);
    assert_eq!(
        user_cfg(&fx),
        b"\xEF\xBB\xBF; header\r\nr_VSync = 10\r\n\r\ncl_fov = 90\r\n"
    );
}

#[test]
fn a_failed_apply_that_returns_after_a_reload_says_the_edits_were_dropped() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::context_like_startup(temp.path(), &["LIVE"]);
    let cfg = ctx.channel.as_ref().unwrap().user_cfg();
    fs::write(&cfg, "cl_fov = 90\n").unwrap();
    let mut app = App::with(ctx);
    feed(&mut app, [Message::Navigate(Screen::Config)]);
    feed(&mut app, [Message::Config(set("cl_fov", "100"))]);
    let applying = app.update(Message::Apply);
    fs::remove_file(&cfg).unwrap();
    fs::create_dir(&cfg).unwrap();
    feed(&mut app, [Message::Rescan]);
    testkit::settle(&mut app, applying);
    let toast = app.toasts().last().unwrap();
    assert_eq!(toast.kind, sc_app::screen::ToastKind::Error);
    assert_eq!(
        toast.text,
        "The changes were not applied, and the files were loaded again meanwhile, so your edits were dropped."
    );
    assert_eq!(app.pending(Screen::Config), 0);
}
