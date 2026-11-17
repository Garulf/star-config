#[path = "../src/testkit.rs"]
mod testkit;

use iced::keyboard::key::{Code, Named, Physical};
use iced::keyboard::{Key, Location, Modifiers};
use iced::{Event, Size, event, keyboard, mouse, window};
use iced_test::selector::Candidate;
use iced_test::simulator::Simulator;
use sc_app::app::{App, Message, Shortcut};
use sc_app::context::Context;
use sc_app::screen::{Screen, ToastKind};
use sc_app::screens::keybinds::{
    self, ADVANCED_TAB, AdvancedMessage, BINDS_TAB, CAPTURE_PROMPT, CellRef, DevicePick,
    EMPTY_BODY, EMPTY_TITLE, GAME_DATA_LOADING, GAME_DEFAULT, NO_GAME_DATA_NOTE, NOT_A_NUMBER,
    NOT_CONNECTED, NOT_IN_BUILD, OptionField, POINT_RANGE, PointPart, RESET_CAPTION, SHOW_UNLISTED,
    SlotChoice, Source, SourceChoice, TRY_AGAIN, UNBIND_CAPTION, UNKNOWN_KEY, UNRECOGNISED,
};
use sc_app::theme;
use sc_core::actionmaps::{ActionMaps, AxisField, DeviceKind, GroupField, Op, Slot};
use sc_core::defaults::Unavailable;
use sc_core::process::{FixedProbe, RunState};
use sc_core::settings::ThemeChoice;
use sc_core::staging::Document;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use testkit::feed;

const FULL: Size = Size::new(1280.0, 800.0);
const DEMO: &str = include_str!("fixtures/keybinds/actionmaps.xml");
const LAYOUT: &str = include_str!("fixtures/keybinds/layout_hotas-oct_exported.xml");
const LAYOUT_FILE: &str = "layout_hotas-oct_exported.xml";
const DEVICES: &str =
    "Keyboard, Mouse, Joystick 1: VKB Gladiator NXT L, Joystick 2: VKB Gladiator NXT R";

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

fn actionmaps(ctx: &Context) -> PathBuf {
    ctx.channel.as_ref().unwrap().actionmaps_xml()
}

fn mappings(ctx: &Context) -> PathBuf {
    ctx.channel.as_ref().unwrap().mappings_dir()
}

fn open(ctx: Context) -> App {
    let mut app = App::with(ctx);
    feed(&mut app, [Message::Navigate(Screen::Keybinds)]);
    app
}

/// The demo profile in LIVE, with the Keybinds screen open. LIVE has no `Data.p4k`, so
/// the screen shows only the file's binds.
fn demo(temp: &Path) -> (App, PathBuf) {
    let ctx = testkit::context_with_channels(temp, &["LIVE"]);
    let path = actionmaps(&ctx);
    write(&path, DEMO);
    (open(ctx), path)
}

/// The demo profile in LIVE with the game's defaults read, with the Keybinds screen open.
fn demo_with_game(temp: &Path) -> (App, PathBuf) {
    let ctx = testkit::context_with_channels(temp, &["LIVE"]);
    let live = ctx.channel.clone().unwrap();
    testkit::write_build(&live, testkit::LIVE_VERSION, testkit::LIVE_CHANGE);
    testkit::seed_keybinds_game_data(&ctx);
    let path = actionmaps(&ctx);
    write(&path, DEMO);
    (open(ctx), path)
}

fn send(app: &mut App, message: keybinds::Message) {
    feed(app, [Message::Keybinds(message)]);
}

fn cell(map: &str, action: &str, kind: DeviceKind) -> CellRef {
    CellRef {
        map: map.into(),
        action: action.into(),
        kind,
    }
}

fn select(app: &mut App, map: &str, action: &str, kind: DeviceKind) {
    send(app, keybinds::Message::Select(cell(map, action, kind)));
}

fn sim(app: &App) -> Simulator<'_, Message> {
    Simulator::with_size(sc_app::settings(), FULL, app.view())
}

fn texts(app: &App) -> Vec<String> {
    let mut ui = sim(app);
    let mut found = Vec::new();
    let _ = ui.find(|candidate: Candidate<'_>| {
        if let Candidate::Text { content, .. } = candidate {
            found.push(content.to_string());
        }
        None::<()>
    });
    found
}

fn has(app: &App, text: &str) -> bool {
    sim(app).find(text).is_ok()
}

fn has_containing(app: &App, needle: &str) -> bool {
    texts(app).iter().any(|text| text.contains(needle))
}

fn clicks(app: &App, label: &str) -> Vec<Message> {
    let mut ui = sim(app);
    ui.click(label)
        .unwrap_or_else(|err| panic!("{label}: {err}"));
    ui.into_messages().collect()
}

fn clicked(app: &mut App, label: &str) {
    let messages = clicks(app, label);
    feed(app, messages);
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

fn pending(app: &App) -> usize {
    app.pending(Screen::Keybinds)
}

fn key(code: Code, key: Key, location: Location, modifiers: Modifiers) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        modified_key: key.clone(),
        key,
        physical_key: Physical::Code(code),
        location,
        modifiers,
        text: None,
        repeat: false,
    })
}

fn input(app: &mut App, event: Event) {
    feed(app, [Message::Input(event)]);
}

fn after(text: &str, op: Op) -> String {
    let mut doc = ActionMaps::parse(Path::new("actionmaps.xml"), text.as_bytes()).unwrap();
    doc.apply(&op).unwrap();
    String::from_utf8(doc.to_bytes()).unwrap()
}

fn replaced_once(haystack: &str, from: &str, to: &str) -> String {
    assert_eq!(haystack.matches(from).count(), 1, "{from:?}");
    haystack.replacen(from, to, 1)
}

fn following(texts: &[String], first: &str, count: usize) -> Vec<String> {
    let start = texts
        .iter()
        .position(|text| text == first)
        .unwrap_or_else(|| panic!("no {first:?} in {texts:?}"));
    texts[start..start + count].to_vec()
}

fn last_toast(app: &App) -> &sc_app::screen::Toast {
    app.toasts().last().expect("a toast")
}

#[test]
fn lists_action_maps_with_counts_and_devices_line() {
    let temp = tempfile::tempdir().unwrap();
    let (app, _) = demo(temp.path());
    assert!(has(&app, DEVICES));
    let texts = texts(&app);
    assert_eq!(following(&texts, "All", 2), ["All", "68"]);
    assert_eq!(
        following(&texts, "Spaceship movement", 2),
        ["Spaceship movement", "19"]
    );
    assert_eq!(
        following(&texts, "Spaceship weapons", 2),
        ["Spaceship weapons", "7"]
    );
    for map in [
        "Spaceship targeting",
        "Spaceship mining",
        "Player",
        "Vehicle driver",
    ] {
        assert!(texts.iter().any(|text| text == map), "{map}");
    }
}

#[test]
fn table_has_a_column_per_device_kind_in_order() {
    let temp = tempfile::tempdir().unwrap();
    let (app, _) = demo(temp.path());
    assert_eq!(
        following(&texts(&app), "Action", 5),
        ["Action", "Keyboard", "Mouse", "Gamepad", "Joystick"]
    );
    assert!(has(&app, "Strafe forward"));
    assert!(has(&app, "v_strafe_forward"));
}

#[test]
fn joystick_cells_name_their_stick() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo(temp.path());
    send(&mut app, keybinds::Message::Search("v_strafe_up".into()));
    assert!(has(&app, "Button 5"));
    assert!(has(&app, "js2"));
    assert!(has(&app, "VKB Gladiator NXT R"));
}

#[test]
fn search_matches_action_name_map_and_input() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo(temp.path());

    send(&mut app, keybinds::Message::Search("decoupling".into()));
    assert!(has(&app, "Ifcs toggle vector decoupling"));
    assert!(!has(&app, "Strafe forward"));

    send(&mut app, keybinds::Message::Search("vehicle driver".into()));
    assert!(has(&app, "Drive horn"));
    assert!(!has(&app, "Jump"));

    send(&mut app, keybinds::Message::Search("js2_button14".into()));
    assert!(has(&app, "Toggle mining laser fire"));
    assert!(!has(&app, "Toggle mining mode"));

    send(
        &mut app,
        keybinds::Message::Search("nothing like this".into()),
    );
    assert!(has_containing(&app, "No binds match"));
}

#[test]
fn selecting_a_keyboard_cell_starts_capture() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo(temp.path());
    assert!(!app.wants_input());
    select(
        &mut app,
        "spaceship_movement",
        "v_strafe_forward",
        DeviceKind::Keyboard,
    );
    assert!(app.wants_input());
    assert!(has(&app, CAPTURE_PROMPT));
    assert!(has(&app, "v_strafe_forward in Spaceship movement"));
}

#[test]
fn captured_key_with_modifier_stages_a_bind() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = demo(temp.path());
    select(
        &mut app,
        "spaceship_movement",
        "v_strafe_forward",
        DeviceKind::Keyboard,
    );
    assert!(!has(&app, "F"));
    input(
        &mut app,
        key(
            Code::AltLeft,
            Key::Named(Named::Alt),
            Location::Left,
            Modifiers::ALT,
        ),
    );
    assert_eq!(pending(&app), 0);
    input(
        &mut app,
        key(
            Code::KeyF,
            Key::Character("f".into()),
            Location::Standard,
            Modifiers::ALT,
        ),
    );
    assert_eq!(pending(&app), 1);
    assert!(!app.wants_input());
    assert!(has(&app, "L Alt"));
    assert!(has(&app, "F"));
    assert!(has(&app, "Was W"));
    assert_eq!(read(&path), DEMO);

    let unknown = key(
        Code::CapsLock,
        Key::Named(Named::CapsLock),
        Location::Standard,
        Modifiers::empty(),
    );
    clicked(&mut app, "Listen again");
    input(&mut app, unknown);
    assert!(has(&app, UNKNOWN_KEY));
    assert_eq!(pending(&app), 1);
}

#[test]
fn escape_cancels_capture_without_staging() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo(temp.path());
    select(
        &mut app,
        "spaceship_movement",
        "v_strafe_forward",
        DeviceKind::Keyboard,
    );
    input(
        &mut app,
        key(
            Code::Escape,
            Key::Named(Named::Escape),
            Location::Standard,
            Modifiers::empty(),
        ),
    );
    assert_eq!(pending(&app), 0);
    assert!(!app.wants_input());
    assert!(has(&app, "Listen again"));
    assert!(!has(&app, CAPTURE_PROMPT));
}

#[test]
fn typed_input_is_validated_inline() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo(temp.path());
    select(
        &mut app,
        "spaceship_movement",
        "v_strafe_up",
        DeviceKind::Joystick,
    );
    send(&mut app, keybinds::Message::Typed("buton5".into()));
    assert!(has(&app, "\"buton5\" is not an input the game knows"));
    assert!(clicks(&app, "Set").is_empty());
    assert_eq!(pending(&app), 0);

    send(&mut app, keybinds::Message::Typed("button9".into()));
    assert!(!has_containing(&app, "is not an input the game knows"));
    clicked(&mut app, "Set");
    assert_eq!(pending(&app), 1);
    assert!(has(&app, "Button 9"));
}

#[test]
fn joystick_cell_offers_quick_picks_and_no_capture() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo(temp.path());
    select(
        &mut app,
        "spaceship_movement",
        "v_pitch",
        DeviceKind::Joystick,
    );
    assert!(!app.wants_input());
    assert!(!has(&app, CAPTURE_PROMPT));
    for pick in [
        "Buttons",
        "1",
        "32",
        "X",
        "Rot Z",
        "Slider 2",
        "Hat up",
        "Hat right",
    ] {
        assert!(has(&app, pick), "{pick}");
    }
    clicked(&mut app, "32");
    clicked(&mut app, "Set");
    assert_eq!(pending(&app), 1);
    assert!(has(&app, "Button 32"));
    assert!(has(&app, "Was Y axis"));
}

#[test]
fn reset_to_default_stages_a_reset_and_apply_removes_the_rebind() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = demo(temp.path());
    select(
        &mut app,
        "spaceship_movement",
        "v_strafe_up",
        DeviceKind::Keyboard,
    );
    assert!(has(&app, RESET_CAPTION));
    clicked(&mut app, "Reset to default");
    assert_eq!(pending(&app), 1);
    assert!(has(&app, "Was Space"));
    feed(&mut app, [Message::Apply]);

    let expected = replaced_once(
        DEMO,
        "   <action name=\"v_strafe_up\">\n    <rebind input=\"kb1_space\"/>\n",
        "   <action name=\"v_strafe_up\">\n",
    );
    assert_eq!(read(&path), expected);
    assert_eq!(pending(&app), 0);
    let toast = last_toast(&app);
    assert_eq!(toast.kind, ToastKind::Success);
    assert_eq!(toast.text, "Applied 1 keybind change");
    assert!(toast.undo.is_some());
}

#[test]
fn unbind_and_reset_are_disabled_where_they_would_do_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo(temp.path());
    select(
        &mut app,
        "spaceship_movement",
        "v_strafe_up",
        DeviceKind::Gamepad,
    );
    assert!(has(&app, UNBIND_CAPTION));
    assert!(clicks(&app, "Unbind").is_empty());
    assert!(clicks(&app, "Reset to default").is_empty());
}

#[test]
fn conflicts_filter_shows_only_conflicting_actions() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo(temp.path());
    assert!(has(&app, "Strafe forward"));
    clicked(&mut app, "Conflicts");
    assert!(has(&app, "Weapon launch missile"));
    assert!(has(&app, "Target unpin all"));
    assert!(!has(&app, "Strafe forward"));
    assert!(!has(&app, "Attack1 group1"));
    clicked(&mut app, "Conflicts");
    assert!(has(&app, "Strafe forward"));
}

#[test]
fn add_a_bind_by_name_then_apply() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = demo(temp.path());
    clicked(&mut app, "Add a bind by name");
    assert!(has(&app, keybinds::ADD_CAPTION));
    assert!(clicks(&app, "Add bind").is_empty());
    send(
        &mut app,
        keybinds::Message::AddMap("spaceship_mining".into()),
    );
    send(
        &mut app,
        keybinds::Message::AddAction("v_toggle_mining_laser_type".into()),
    );
    send(&mut app, keybinds::Message::AddInput("lalt+k".into()));
    clicked(&mut app, "Add bind");
    assert_eq!(pending(&app), 1);
    assert!(has(&app, "Toggle mining laser type"));
    feed(&mut app, [Message::Apply]);

    let written = read(&path);
    assert!(
        written.contains("<rebind input=\"kb1_lalt+k\"/>"),
        "{written}"
    );
    let expected = after(
        DEMO,
        Op::Bind {
            map: "spaceship_mining".into(),
            action: "v_toggle_mining_laser_type".into(),
            input: "kb1_lalt+k".into(),
        },
    );
    assert_eq!(written, expected);
}

#[test]
fn swap_joysticks_stages_one_edit_and_swaps_prefixes_on_disk() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = demo(temp.path());
    clicked(&mut app, "Swap joysticks");
    assert!(has(
        &app,
        "Swaps every Joystick 1 bind with Joystick 2, 29 binds"
    ));
    clicked(&mut app, "Swap");
    assert_eq!(pending(&app), 1);
    feed(&mut app, [Message::Apply]);

    let written = read(&path);
    assert_eq!(written, after(DEMO, Op::SwapJoysticks { a: 1, b: 2 }));
    assert!(written.contains("<action name=\"v_pitch\">\n    <rebind input=\"js2_y\"/>"));
    assert!(
        written.contains("<options type=\"joystick\" instance=\"2\" Product=\"VKB Gladiator NXT L")
    );
    assert_eq!(
        written.matches("js1_").count(),
        DEMO.matches("js2_").count()
    );
}

#[test]
fn apply_changes_only_the_edited_binding_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = demo(temp.path());
    select(
        &mut app,
        "spaceship_weapons",
        "v_attack1_group1",
        DeviceKind::Joystick,
    );
    send(&mut app, keybinds::Message::Typed("button9".into()));
    clicked(&mut app, "Set");
    feed(&mut app, [Message::Apply]);
    assert_eq!(
        read(&path),
        replaced_once(DEMO, "\"js1_button1\"", "\"js1_button9\"")
    );
}

#[test]
fn a_file_changed_on_disk_is_replayed_before_apply() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = demo(temp.path());
    select(
        &mut app,
        "spaceship_weapons",
        "v_attack1_group1",
        DeviceKind::Joystick,
    );
    send(&mut app, keybinds::Message::Typed("button9".into()));
    clicked(&mut app, "Set");
    let elsewhere = replaced_once(DEMO, "\"kb1_h\"", "\"kb1_j\"");
    write(&path, &elsewhere);
    feed(&mut app, [Message::Apply]);
    assert_eq!(
        read(&path),
        replaced_once(&elsewhere, "\"js1_button1\"", "\"js1_button9\"")
    );
    assert_eq!(last_toast(&app).kind, ToastKind::Success);
}

fn layout_choice(ctx: &Context) -> SourceChoice {
    SourceChoice {
        source: Source::Layout(mappings(ctx).join(LAYOUT_FILE)),
        label: "hotas-oct".into(),
    }
}

#[test]
fn switching_source_with_pending_edits_asks() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::context_with_channels(temp.path(), &["LIVE"]);
    write(&actionmaps(&ctx), DEMO);
    write(&mappings(&ctx).join(LAYOUT_FILE), LAYOUT);
    let choice = layout_choice(&ctx);
    let mut app = open(ctx);
    select(
        &mut app,
        "spaceship_movement",
        "v_pitch",
        DeviceKind::Joystick,
    );
    send(&mut app, keybinds::Message::Typed("button9".into()));
    clicked(&mut app, "Set");
    assert_eq!(pending(&app), 1);

    send(&mut app, keybinds::Message::PickSource(choice));
    let dialog = app.dialog().expect("asks first");
    assert_eq!(dialog.actions[0].label, "Discard and switch");
    assert_eq!(dialog.actions[1].label, "Cancel");
    assert!(has(&app, DEVICES));
    choose(&mut app, "Discard and switch");
    assert_eq!(pending(&app), 0);
    assert!(has(
        &app,
        "Keyboard, Joystick 1: VKB Gladiator NXT L, Joystick 2: VKB Gladiator NXT R"
    ));
    assert!(has(&app, "Save as layout..."));
}

#[test]
fn save_as_layout_writes_into_mappings_and_refuses_silent_overwrite() {
    let temp = tempfile::tempdir().unwrap();
    let mut ctx =
        testkit::context_with_channels(temp.path(), &["LIVE"]).with_probe(Arc::new(FixedProbe {
            game: RunState::Running,
            launcher: RunState::NotRunning,
        }));
    ctx.game = RunState::Running;
    write(&actionmaps(&ctx), DEMO);
    write(&mappings(&ctx).join(LAYOUT_FILE), LAYOUT);
    let choice = layout_choice(&ctx);
    let target = mappings(&ctx).join("layout_hotas-nov_exported.xml");
    let mut app = open(ctx);
    assert!(!has(&app, "Save as layout..."));
    send(&mut app, keybinds::Message::PickSource(choice));
    assert!(app.dialog().is_none());

    select(
        &mut app,
        "spaceship_movement",
        "v_afterburner",
        DeviceKind::Joystick,
    );
    send(&mut app, keybinds::Message::Typed("button20".into()));
    clicked(&mut app, "Set");
    clicked(&mut app, "Save as layout...");
    send(&mut app, keybinds::Message::SaveName("hotas-nov".into()));
    clicked(&mut app, "Save layout");

    let first = replaced_once(LAYOUT, "\"js2_button2\"", "\"js2_button20\"");
    assert_eq!(read(&target), first);
    assert_eq!(read(&mappings(app.context()).join(LAYOUT_FILE)), LAYOUT);
    let toast = last_toast(&app);
    assert_eq!(
        toast.text,
        "Saved layout \"hotas-nov\". Load it in game from Options, Keybindings, Control Profiles."
    );
    assert!(toast.undo.is_some());

    select(
        &mut app,
        "spaceship_movement",
        "v_strafe_up",
        DeviceKind::Joystick,
    );
    send(&mut app, keybinds::Message::Typed("button21".into()));
    clicked(&mut app, "Set");
    clicked(&mut app, "Save as layout...");
    send(&mut app, keybinds::Message::SaveName("hotas-nov".into()));
    clicked(&mut app, "Save layout");
    let dialog = app.dialog().expect("asks before replacing");
    assert_eq!(dialog.title, "Replace layout \"hotas-nov\"?");
    assert_eq!(read(&target), first);
    choose(&mut app, "Replace");
    assert_eq!(
        read(&target),
        replaced_once(&first, "\"js2_button5\"", "\"js2_button21\"")
    );
}

#[test]
fn unrecognised_file_is_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::context_with_channels(temp.path(), &["LIVE"]);
    let path = actionmaps(&ctx);
    let odd = "<Profile name=\"mine\">\n <actionmap name=\"player\"/>\n</Profile>\n";
    write(&path, odd);
    let mut app = open(ctx);
    assert!(has_containing(&app, UNRECOGNISED));
    assert!(has_containing(&app, "Line 1"));
    assert!(has(&app, "<Profile name=\"mine\">"));
    assert!(!has_containing(&app, "Add a bind by name"));
    select(&mut app, "player", "jump", DeviceKind::Keyboard);
    assert!(!app.wants_input());
    feed(&mut app, [Message::Apply]);
    assert_eq!(pending(&app), 0);
    assert_eq!(read(&path), odd);
}

#[test]
fn no_files_shows_empty_state() {
    let temp = tempfile::tempdir().unwrap();
    let app = open(testkit::context_with_channels(temp.path(), &["LIVE"]));
    assert!(has(&app, EMPTY_TITLE));
    assert!(has(&app, EMPTY_BODY));
}

#[test]
fn shell_shortcuts_do_not_fire_while_capturing() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = demo(temp.path());
    select(
        &mut app,
        "spaceship_movement",
        "v_pitch",
        DeviceKind::Joystick,
    );
    send(&mut app, keybinds::Message::Typed("button9".into()));
    clicked(&mut app, "Set");
    assert_eq!(pending(&app), 1);

    let ctrl_s = || Event::Keyboard(testkit::ctrl("s"));
    let route =
        |app: &App| app.event_handler()(ctrl_s(), event::Status::Ignored, window::Id::unique());
    assert!(matches!(
        route(&app),
        Some(Message::Shortcut(Shortcut::Apply))
    ));

    select(
        &mut app,
        "spaceship_movement",
        "v_strafe_forward",
        DeviceKind::Keyboard,
    );
    let routed = route(&app).expect("routed to the screen");
    assert!(matches!(routed, Message::Input(_)));
    feed(&mut app, [routed]);
    assert_eq!(read(&path), DEMO);
    assert!(app.toasts().is_empty());
}

#[test]
fn keybinds_pictures() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo_with_game(temp.path());
    send(
        &mut app,
        keybinds::Message::PickMap(Some("spaceship_weapons".into())),
    );
    select(
        &mut app,
        "spaceship_weapons",
        "v_attack1_group2",
        DeviceKind::Joystick,
    );
    send(&mut app, keybinds::Message::Typed("button6".into()));
    clicked(&mut app, "Set");
    assert!(has(&app, "Was Button 2"));
    select(
        &mut app,
        "spaceship_weapons",
        "v_attack1_group1",
        DeviceKind::Joystick,
    );
    assert_eq!(pending(&app), 1);
    for (choice, name) in [
        (ThemeChoice::Dark, "keybinds-dark"),
        (ThemeChoice::Light, "keybinds-light"),
    ] {
        testkit::snapshot(&mut sim(&app), &theme::theme(choice), name);
    }

    let temp = tempfile::tempdir().unwrap();
    let (app, _) = demo(temp.path());
    assert!(has_containing(&app, NO_GAME_DATA_NOTE));
    let dark = theme::theme(ThemeChoice::Dark);
    testkit::snapshot(&mut sim(&app), &dark, "keybinds-no-game-data-dark");
}

#[test]
fn selected_row_scrolls_into_view_above_the_edit_panel() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo(temp.path());
    assert!(!has(&app, "v_weapon_toggle_gimbal_mode"));
    select(
        &mut app,
        "spaceship_weapons",
        "v_master_mode_cycle",
        DeviceKind::Mouse,
    );
    send(&mut app, keybinds::Message::TableResized(240.0));
    assert!(has(&app, "v_weapon_toggle_gimbal_mode"));
}

#[test]
fn a_channel_switch_during_apply_shows_the_new_channel() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::context_with_channels(temp.path(), &["LIVE", "PTU"]);
    let live = actionmaps(&ctx);
    let ptu_channel = ctx
        .channels
        .iter()
        .find(|c| c.name == "PTU")
        .unwrap()
        .clone();
    let ptu = ptu_channel.actionmaps_xml();
    write(&live, DEMO);
    write(&ptu, DEMO);
    let mut app = open(ctx);
    select(
        &mut app,
        "spaceship_movement",
        "v_strafe_up",
        DeviceKind::Keyboard,
    );
    clicked(&mut app, "Reset to default");
    let applying = app.update(Message::Apply);
    feed(
        &mut app,
        [Message::PickChannel("PTU".into()), Message::DialogAction(0)],
    );
    testkit::settle(&mut app, applying);

    assert_ne!(read(&live), DEMO);
    assert_eq!(read(&ptu), DEMO);
    assert_eq!(pending(&app), 0);
    assert!(!has(&app, "Was Space"));
    let first = last_toast(&app).clone();
    assert_eq!(first.text, "Applied 1 keybind change");
    assert_eq!(first.undo_in[0].channel.name, "LIVE");

    select(
        &mut app,
        "spaceship_movement",
        "v_strafe_up",
        DeviceKind::Keyboard,
    );
    clicked(&mut app, "Reset to default");
    feed(&mut app, [Message::Apply]);
    assert_eq!(read(&ptu), read(&live));
    let second = last_toast(&app);
    assert_eq!(second.kind, ToastKind::Success);
    assert_eq!(second.undo_in[0].channel.name, "PTU");
}

// With the game's defaults ------------------------------------------------------

#[test]
fn every_listed_action_shows_with_its_game_label() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo_with_game(temp.path());
    assert!(!has_containing(&app, NO_GAME_DATA_NOTE));
    assert!(!has(&app, "Add a bind by name"));
    let texts = texts(&app);
    for heading in ["GENERAL", "SPACESHIP", "ON FOOT", "GROUND VEHICLES"] {
        assert!(texts.iter().any(|text| text == heading), "{heading}");
    }
    assert_eq!(
        following(&texts, "Seats and exits", 2),
        ["Seats and exits", "3"]
    );
    assert!(has(&app, "Flight movement"));

    send(
        &mut app,
        keybinds::Message::PickMap(Some("seat_general".into())),
    );
    assert!(has(&app, "Eject"));
    assert!(has(&app, "Right Alt"));
    assert!(has(&app, "Y"));
    assert!(has(&app, "Leave seat"));
    assert!(has(&app, "Y button"));
    assert!(has(&app, "Open or close all doors"));

    send(&mut app, keybinds::Message::PickMap(None));
    send(
        &mut app,
        keybinds::Message::Search("middle mouse button".into()),
    );
    assert!(has(&app, "Launch missile"));
    assert!(!has(&app, "Eject"));
}

#[test]
fn edit_panel_shows_the_default_a_reset_returns_to() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo_with_game(temp.path());
    send(
        &mut app,
        keybinds::Message::Search("v_strafe_forward".into()),
    );
    assert!(has(&app, "Strafe forward"));
    assert!(!has_containing(&app, "Was "));
    select(
        &mut app,
        "spaceship_movement",
        "v_strafe_forward",
        DeviceKind::Keyboard,
    );
    assert!(has(&app, "Game default"));
    send(&mut app, keybinds::Message::ClosePanel);
    send(&mut app, keybinds::Message::Search("decoupled".into()));
    select(
        &mut app,
        "spaceship_movement",
        "v_ifcs_toggle_vector_decoupling",
        DeviceKind::Keyboard,
    );
    assert!(has(&app, "V"), "the default the reset returns to");
}

#[test]
fn unlisted_actions_appear_only_with_the_toggle() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo_with_game(temp.path());
    send(
        &mut app,
        keybinds::Message::PickMap(Some("seat_general".into())),
    );
    assert!(has(&app, SHOW_UNLISTED));
    assert!(!has(&app, "Emergency exit"));
    send(&mut app, keybinds::Message::ShowUnlisted(true));
    assert!(has(&app, "Emergency exit"));
    assert!(has(&app, "v_emergency_exit"));
    send(&mut app, keybinds::Message::ShowUnlisted(false));
    assert!(!has(&app, "Emergency exit"));
}

#[test]
fn unbind_stages_explicit_unbind_and_reset_stages_removal() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = demo_with_game(temp.path());
    select(&mut app, "seat_general", "v_eject", DeviceKind::Keyboard);
    assert!(has(&app, UNBIND_CAPTION));
    assert!(
        clicks(&app, "Reset to default").is_empty(),
        "nothing of the user's to reset"
    );
    clicked(&mut app, "Unbind");
    assert_eq!(pending(&app), 1);
    assert!(has(&app, "Unbound"));
    assert!(clicks(&app, "Unbind").is_empty(), "already unbound");

    select(
        &mut app,
        "spaceship_movement",
        "v_strafe_up",
        DeviceKind::Keyboard,
    );
    assert!(has(&app, RESET_CAPTION));
    clicked(&mut app, "Reset to default");
    assert_eq!(pending(&app), 2);
    feed(&mut app, [Message::Apply]);

    let unbound = after(
        DEMO,
        Op::Unbind {
            map: "seat_general".into(),
            action: "v_eject".into(),
            slot: Slot {
                kind: DeviceKind::Keyboard,
                instance: 1,
            },
        },
    );
    let expected = after(
        &unbound,
        Op::Reset {
            map: "spaceship_movement".into(),
            action: "v_strafe_up".into(),
            kind: DeviceKind::Keyboard,
        },
    );
    let written = read(&path);
    assert_eq!(written, expected);
    assert!(written.contains("<rebind input=\"kb1_ \"/>"), "{written}");
}

#[test]
fn reset_removes_every_rebind_of_the_kind() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::context_with_channels(temp.path(), &["LIVE"]);
    let path = actionmaps(&ctx);
    let two_sticks = replaced_once(
        DEMO,
        "<rebind input=\"js1_y\"/>",
        "<rebind input=\"js1_y\"/>\n    <rebind input=\"js2_ \"/>",
    );
    write(&path, &two_sticks);
    let mut app = open(ctx);
    select(
        &mut app,
        "spaceship_movement",
        "v_pitch",
        DeviceKind::Joystick,
    );
    assert!(has_containing(&app, "more than one joystick entry"));
    clicked(&mut app, "Reset to default");
    feed(&mut app, [Message::Apply]);
    let written = read(&path);
    assert!(!written.contains("js1_y"), "{written}");
    assert!(!written.contains("js2_ "), "{written}");
}

#[test]
fn joystick_bind_replaces_the_default_stick() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = demo_with_game(temp.path());
    send(
        &mut app,
        keybinds::Message::PickMap(Some("spaceship_weapons".into())),
    );
    select(
        &mut app,
        "spaceship_weapons",
        "v_attack1_group1",
        DeviceKind::Joystick,
    );
    send(&mut app, keybinds::Message::TableResized(150.0));
    assert!(has(&app, "Stick and input as the game writes it"));
    assert!(has(&app, "js1"));
    let js4 = Slot {
        kind: DeviceKind::Joystick,
        instance: 4,
    };
    send(
        &mut app,
        keybinds::Message::PickStick(SlotChoice {
            slot: js4,
            label: "Joystick 4".into(),
        }),
    );
    send(&mut app, keybinds::Message::Typed("button9".into()));
    clicked(&mut app, "Set");
    assert_eq!(pending(&app), 1);
    assert!(has(&app, "js4"));
    assert!(has(&app, "Button 9"));
    feed(&mut app, [Message::Apply]);

    let written = read(&path);
    assert_eq!(
        written,
        after(
            DEMO,
            Op::Bind {
                map: "spaceship_weapons".into(),
                action: "v_attack1_group1".into(),
                input: "js4_button9".into(),
            },
        )
    );
    let start = written.find("<action name=\"v_attack1_group1\">").unwrap();
    let end = start + written[start..].find("</action>").unwrap();
    assert_eq!(written[start..end].matches("input=\"js").count(), 1);
}

#[test]
fn mouse_button_in_keyboard_column_is_accepted() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = demo_with_game(temp.path());
    select(
        &mut app,
        "spaceship_movement",
        "v_strafe_forward",
        DeviceKind::Keyboard,
    );
    send(&mut app, keybinds::Message::Typed("mouse4".into()));
    assert!(!has_containing(&app, "is not an input the game knows"));
    send(&mut app, keybinds::Message::CaptureHover(true));
    input(
        &mut app,
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle)),
    );
    assert_eq!(pending(&app), 1);
    assert!(has(&app, "Middle mouse button"));
    feed(&mut app, [Message::Apply]);
    let expected = after(
        DEMO,
        Op::Bind {
            map: "spaceship_movement".into(),
            action: "v_strafe_forward".into(),
            input: "kb1_mouse3".into(),
        },
    );
    assert_eq!(read(&path), expected);
    assert!(expected.contains("<rebind input=\"kb1_mouse3\"/>"));
}

#[test]
fn default_clashes_are_not_flagged() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo_with_game(temp.path());
    assert_eq!(following(&texts(&app), "Conflicts", 2), ["Conflicts", "1"]);
    clicked(&mut app, "Conflicts");
    assert!(has(&app, "Launch missile"));
    assert!(has(&app, "Unpin all targets"));
    assert!(!has(&app, "Hail target"));
    assert!(!has(&app, "Next weapon preset"));
}

#[test]
fn user_clash_is_flagged() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo_with_game(temp.path());
    select(
        &mut app,
        "spaceship_targeting",
        "v_target_reticle_focus",
        DeviceKind::Keyboard,
    );
    send(&mut app, keybinds::Message::Typed("lalt+r".into()));
    clicked(&mut app, "Set");
    send(&mut app, keybinds::Message::ClosePanel);
    assert_eq!(following(&texts(&app), "Conflicts", 2), ["Conflicts", "2"]);
    clicked(&mut app, "Conflicts");
    assert!(has(&app, "Focus on target"));
    assert!(has(&app, "Previous hostile target"));
    assert!(!has(&app, "Next hostile target"));
}

#[test]
fn missing_label_falls_back_to_readable_name() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo_with_game(temp.path());
    send(&mut app, keybinds::Message::Search("cruise".into()));
    assert!(has(&app, "Toggle cruise control"));
    let texts = texts(&app);
    assert!(
        !texts
            .iter()
            .any(|text| text.contains('@') || text.contains("ui_Kb")),
        "{texts:?}"
    );
}

#[test]
fn file_only_action_shows_as_not_in_this_game_version() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo_with_game(temp.path());
    send(
        &mut app,
        keybinds::Message::PickMap(Some("spaceship_mining".into())),
    );
    assert!(has(&app, "Mining use consumable3"));
    assert!(has(&app, NOT_IN_BUILD));
    select(
        &mut app,
        "spaceship_mining",
        "v_mining_use_consumable3",
        DeviceKind::Keyboard,
    );
    send(&mut app, keybinds::Message::Typed("lalt+4".into()));
    clicked(&mut app, "Set");
    send(&mut app, keybinds::Message::TableResized(150.0));
    assert_eq!(pending(&app), 1);
    assert!(has(&app, NOT_IN_BUILD));
}

#[test]
fn without_game_data_the_screen_works_as_before_with_a_note() {
    let temp = tempfile::tempdir().unwrap();
    let (app, _) = demo(temp.path());
    let note = format!("{NO_GAME_DATA_NOTE} {}", Unavailable::NoArchive.reason());
    assert!(has(&app, &note), "{note}");
    assert_eq!(
        texts(&app)
            .iter()
            .filter(|text| text.contains("only the binds"))
            .count(),
        1
    );
    assert!(!has_containing(&app, "only your own binds"));
    assert!(has(&app, "Add a bind by name"));
    assert!(!has(&app, SHOW_UNLISTED));
    assert!(has(&app, "Strafe forward"));
    assert!(has(&app, "W"));
}

#[test]
fn try_again_reads_game_data_that_has_appeared_since() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo(temp.path());
    assert!(has_containing(&app, NO_GAME_DATA_NOTE));
    assert!(!has(&app, SHOW_UNLISTED));

    let ctx = app.context().clone();
    let live = ctx.channel.clone().unwrap();
    testkit::write_build(&live, testkit::LIVE_VERSION, testkit::LIVE_CHANGE);
    testkit::seed_keybinds_game_data(&ctx);
    clicked(&mut app, TRY_AGAIN);
    assert!(!has_containing(&app, NO_GAME_DATA_NOTE));
    assert!(!has(&app, TRY_AGAIN));
    assert!(has(&app, SHOW_UNLISTED));
    assert!(has(&app, "Seats and exits"));
}

#[test]
fn unbind_writes_the_stick_the_picker_shows() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = demo_with_game(temp.path());
    send(
        &mut app,
        keybinds::Message::PickMap(Some("spaceship_weapons".into())),
    );
    select(
        &mut app,
        "spaceship_weapons",
        "v_attack1_group1",
        DeviceKind::Joystick,
    );
    let js4 = Slot {
        kind: DeviceKind::Joystick,
        instance: 4,
    };
    send(
        &mut app,
        keybinds::Message::PickStick(SlotChoice {
            slot: js4,
            label: "Joystick 4".into(),
        }),
    );
    clicked(&mut app, "Unbind");
    assert_eq!(pending(&app), 1);
    feed(&mut app, [Message::Apply]);
    let written = read(&path);
    assert_eq!(
        written,
        after(
            DEMO,
            Op::Unbind {
                map: "spaceship_weapons".into(),
                action: "v_attack1_group1".into(),
                slot: js4,
            },
        )
    );
    assert!(written.contains("<rebind input=\"js4_ \"/>"), "{written}");
}

#[test]
fn game_data_loading_shows_while_it_reads() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::context_with_channels(temp.path(), &["LIVE"]);
    write(&actionmaps(&ctx), DEMO);
    let mut app = App::with(ctx);
    let task = app.update(Message::Navigate(Screen::Keybinds));
    assert!(has(&app, GAME_DATA_LOADING));
    testkit::settle(&mut app, task);
    assert!(!has(&app, GAME_DATA_LOADING));
    assert!(has(&app, "Strafe forward"));
}

// Advanced controls ---------------------------------------------------------------

const ADVANCED: &str = include_str!("fixtures/keybinds/advanced.xml");
const STICK_L: &str = "VKB Gladiator NXT L  {0200231D-0000-0000-0000-504944564944}";
const X52: &str = "X52 H.O.T.A.S.  {075C06A3-0000-0000-0000-504944564944}";

/// The advanced controls demo profile in LIVE, with the Advanced controls view open.
fn advanced_demo(temp: &Path, with_game: bool) -> (App, PathBuf) {
    let ctx = testkit::context_with_channels(temp, &["LIVE"]);
    if with_game {
        let live = ctx.channel.clone().unwrap();
        testkit::write_build(&live, testkit::LIVE_VERSION, testkit::LIVE_CHANGE);
        testkit::seed_keybinds_game_data(&ctx);
    }
    let path = actionmaps(&ctx);
    write(&path, ADVANCED);
    let mut app = open(ctx);
    clicked(&mut app, ADVANCED_TAB);
    (app, path)
}

fn adv(app: &mut App, message: AdvancedMessage) {
    send(app, keybinds::Message::Advanced(message));
}

fn pick_device(app: &mut App, device: DevicePick) {
    adv(app, AdvancedMessage::Pick(device));
}

fn group_field(group: &str, field: GroupField) -> OptionField {
    OptionField::Group {
        group: group.into(),
        field,
    }
}

fn axis_field(product: &str, input: &str, field: AxisField) -> OptionField {
    OptionField::Axis {
        product: product.into(),
        input: input.into(),
        field,
    }
}

/// What each text field shows: its value, or its placeholder when empty.
fn field_texts(app: &App) -> Vec<String> {
    let mut ui = sim(app);
    let mut found = Vec::new();
    let _ = ui.find(|candidate: Candidate<'_>| {
        if let Candidate::TextInput { state, .. } = candidate {
            found.push(state.text().to_string());
        }
        None::<()>
    });
    found
}

fn count(app: &App, text: &str) -> usize {
    texts(app).iter().filter(|found| *found == text).count()
}

fn applied(app: &mut App, path: &Path) -> String {
    feed(app, [Message::Apply]);
    assert_eq!(pending(app), 0);
    read(path)
}

fn curve_of(text: &str, kind: DeviceKind, instance: u32, group: &str) -> Vec<(String, String)> {
    let doc = ActionMaps::parse(Path::new("actionmaps.xml"), text.as_bytes()).unwrap();
    doc.group_options()
        .iter()
        .find(|slot| slot.kind == kind && slot.instance == instance)
        .and_then(|slot| slot.groups.iter().find(|g| g.group == group))
        .and_then(|g| g.curve.clone())
        .unwrap_or_default()
}

fn points(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

#[test]
fn groups_show_only_flagged_controls() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::Joystick(1));
    for label in [
        "All devices",
        "Flight move",
        "Pitch",
        "Space brake",
        "Free look",
    ] {
        assert!(has(&app, label), "{label}");
    }
    assert_eq!(count(&app, "Invert"), 6);
    assert_eq!(count(&app, "Response curve"), 7);
    assert_eq!(count(&app, "Exponent"), 7);
    assert_eq!(count(&app, "Sensitivity"), 0);

    pick_device(&mut app, DevicePick::Gamepad);
    assert_eq!(count(&app, "Sensitivity"), 1);
    assert_eq!(count(&app, "Invert"), 2);
    assert_eq!(count(&app, "Response curve"), 4);

    pick_device(&mut app, DevicePick::KeyboardMouse);
    assert!(has(&app, "Mouse response"));
    assert_eq!(count(&app, "Invert"), 4);
    assert_eq!(count(&app, "Response curve"), 3);
    assert!(!texts(&app).iter().any(|t| t.contains('@')));
}

#[test]
fn values_come_from_the_file_then_the_default() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::Gamepad);
    let fields = field_texts(&app);
    assert!(fields.contains(&"1".to_string()), "{fields:?}");
    assert!(fields.contains(&"1.5".to_string()), "{fields:?}");
    assert!(has(&app, "Game default: 2"));
    assert!(has(&app, GAME_DEFAULT));
}

#[test]
fn set_exponent_stages_set_group() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::Joystick(1));
    let field = group_field("flight_move_yaw", GroupField::Exponent);
    adv(
        &mut app,
        AdvancedMessage::Typed(field.clone(), "1.5".into()),
    );
    assert_eq!(pending(&app), 0);
    adv(&mut app, AdvancedMessage::Submit(field));
    assert_eq!(pending(&app), 1);
    assert!(has(&app, "Was 2"));
    let expected = after(
        ADVANCED,
        Op::SetGroup {
            kind: DeviceKind::Joystick,
            instance: 1,
            group: "flight_move_yaw".into(),
            field: GroupField::Exponent,
            value: "1.5".into(),
        },
    );
    assert_eq!(applied(&mut app, &path), expected);
    assert_eq!(
        expected,
        replaced_once(
            ADVANCED,
            "<flight_move_yaw exponent=\"2\"/>",
            "<flight_move_yaw exponent=\"1.5\"/>"
        )
    );
}

#[test]
fn a_bad_number_is_not_staged() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::Joystick(1));
    let field = group_field("flight_move_yaw", GroupField::Exponent);
    adv(
        &mut app,
        AdvancedMessage::Typed(field.clone(), "fast".into()),
    );
    adv(&mut app, AdvancedMessage::Submit(field));
    assert_eq!(pending(&app), 0);
    assert!(has(&app, NOT_A_NUMBER));
}

#[test]
fn invert_toggle_stages_set_group() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::Joystick(1));
    adv(
        &mut app,
        AdvancedMessage::Invert("flight_move_roll".into(), true),
    );
    assert_eq!(pending(&app), 1);
    let expected = after(
        ADVANCED,
        Op::SetGroup {
            kind: DeviceKind::Joystick,
            instance: 1,
            group: "flight_move_roll".into(),
            field: GroupField::Invert,
            value: "1".into(),
        },
    );
    assert_eq!(applied(&mut app, &path), expected);
}

#[test]
fn sensitivity_slider_stages_on_release_within_the_tree_range() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::Gamepad);
    let field = group_field("master", GroupField::Sensitivity);
    adv(&mut app, AdvancedMessage::Slide(field.clone(), 1.2));
    adv(&mut app, AdvancedMessage::Slide(field.clone(), 1.25));
    assert_eq!(pending(&app), 0);
    adv(&mut app, AdvancedMessage::Release);
    assert_eq!(pending(&app), 1);
    adv(&mut app, AdvancedMessage::Slide(field, 9.0));
    adv(&mut app, AdvancedMessage::Release);
    let expected = after(
        &after(
            ADVANCED,
            Op::SetGroup {
                kind: DeviceKind::Gamepad,
                instance: 1,
                group: "master".into(),
                field: GroupField::Sensitivity,
                value: "1.25".into(),
            },
        ),
        Op::SetGroup {
            kind: DeviceKind::Gamepad,
            instance: 1,
            group: "master".into(),
            field: GroupField::Sensitivity,
            value: "2".into(),
        },
    );
    assert_eq!(applied(&mut app, &path), expected);
}

#[test]
fn reset_removes_the_value() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::Joystick(2));
    adv(
        &mut app,
        AdvancedMessage::Reset(group_field("flight_move_space_brake", GroupField::Invert)),
    );
    assert_eq!(pending(&app), 1);
    let text = applied(&mut app, &path);
    assert_eq!(
        text,
        after(
            ADVANCED,
            Op::ResetGroup {
                kind: DeviceKind::Joystick,
                instance: 2,
                group: "flight_move_space_brake".into(),
                field: GroupField::Invert,
            }
        )
    );
    assert!(!text.contains("flight_move_space_brake"));
}

#[test]
fn reset_does_nothing_where_the_file_holds_no_value() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::Joystick(1));
    adv(
        &mut app,
        AdvancedMessage::Reset(group_field("flight_move_roll", GroupField::Invert)),
    );
    assert_eq!(pending(&app), 0);
}

#[test]
fn axis_edit_updates_both_copies() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::Joystick(1));
    assert!(has(&app, "X axis"));
    let field = axis_field(STICK_L, "x", AxisField::Saturation);
    adv(&mut app, AdvancedMessage::Slide(field, 0.95));
    adv(&mut app, AdvancedMessage::Release);
    assert_eq!(pending(&app), 1);
    let line = "<option input=\"x\" saturation=\"0.89099997\"/>";
    assert_eq!(ADVANCED.matches(line).count(), 2);
    assert_eq!(
        applied(&mut app, &path),
        ADVANCED.replace(line, "<option input=\"x\" saturation=\"0.95\"/>")
    );
}

#[test]
fn acceleration_is_a_number_field_and_reset_clears_an_axis_value() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::Joystick(1));
    let field = axis_field(STICK_L, "rotz", AxisField::Acceleration);
    adv(
        &mut app,
        AdvancedMessage::Typed(field.clone(), "0.5".into()),
    );
    adv(&mut app, AdvancedMessage::Submit(field));
    adv(
        &mut app,
        AdvancedMessage::Reset(axis_field(STICK_L, "rotz", AxisField::Deadzone)),
    );
    assert_eq!(pending(&app), 2);
    assert_eq!(
        applied(&mut app, &path),
        replaced_once(
            ADVANCED,
            "<option input=\"rotz\" deadzone=\"0.05\"/>",
            "<option input=\"rotz\" acceleration=\"0.5\"/>"
        )
    );
}

#[test]
fn curve_points_stage_in_order() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::Joystick(1));
    adv(
        &mut app,
        AdvancedMessage::OpenCurve("flight_move_pitch".into()),
    );
    assert!(has(&app, "Add point"));
    adv(&mut app, AdvancedMessage::AddPoint);
    assert_eq!(pending(&app), 1);
    adv(
        &mut app,
        AdvancedMessage::PointTyped(0, PointPart::In, "0.9".into()),
    );
    adv(&mut app, AdvancedMessage::SubmitPoints);
    assert_eq!(pending(&app), 2);
    adv(&mut app, AdvancedMessage::RemovePoint(1));
    assert_eq!(pending(&app), 3);
    let text = applied(&mut app, &path);
    assert_eq!(
        curve_of(&text, DeviceKind::Joystick, 1, "flight_move_pitch"),
        points(&[("0.25", "0.1"), ("0.75", "0.6"), ("0.9", "0.05")])
    );
}

#[test]
fn a_point_outside_zero_to_one_is_not_staged() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::Joystick(1));
    adv(
        &mut app,
        AdvancedMessage::OpenCurve("flight_move_pitch".into()),
    );
    adv(
        &mut app,
        AdvancedMessage::PointTyped(1, PointPart::Out, "1.5".into()),
    );
    adv(&mut app, AdvancedMessage::SubmitPoints);
    assert_eq!(pending(&app), 0);
    assert!(has(&app, POINT_RANGE));
}

#[test]
fn keyboard_and_mouse_curve_goes_to_the_keyboard_slot() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::KeyboardMouse);
    adv(
        &mut app,
        AdvancedMessage::OpenCurve("flight_move_pitch".into()),
    );
    adv(&mut app, AdvancedMessage::AddPoint);
    assert_eq!(pending(&app), 1);
    let set = applied(&mut app, &path);
    assert_eq!(
        set,
        after(
            ADVANCED,
            Op::SetCurve {
                kind: DeviceKind::Keyboard,
                instance: 1,
                group: "flight_move_pitch".into(),
                points: points(&[("0.5", "0.5")]),
            }
        )
    );
    assert_eq!(
        curve_of(&set, DeviceKind::Keyboard, 1, "flight_move_pitch"),
        points(&[("0.5", "0.5")])
    );
    adv(
        &mut app,
        AdvancedMessage::Reset(OptionField::Curve {
            group: "flight_move_pitch".into(),
        }),
    );
    assert_eq!(pending(&app), 1);
    assert_eq!(applied(&mut app, &path), ADVANCED);
}

#[test]
fn orphan_device_is_listed() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = advanced_demo(temp.path(), true);
    assert!(has(&app, "X52 H.O.T.A.S."));
    assert!(has(&app, NOT_CONNECTED));
    pick_device(&mut app, DevicePick::Unmapped(X52.into()));
    assert!(has(&app, "Z rotation"));
    let field = axis_field(X52, "rotz", AxisField::Deadzone);
    adv(&mut app, AdvancedMessage::Slide(field, 0.3));
    adv(&mut app, AdvancedMessage::Release);
    assert_eq!(
        applied(&mut app, &path),
        replaced_once(ADVANCED, "deadzone=\"0.44549999\"", "deadzone=\"0.3\"")
    );
}

#[test]
fn deviceoptions_pair_with_their_slot_by_guid() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = advanced_demo(temp.path(), true);
    assert!(has(&app, "Joystick 2"));
    assert!(has(&app, "Thrustmaster T-Pedals"));
    assert!(!has(&app, "T-Pedals"));
    pick_device(&mut app, DevicePick::Joystick(2));
    assert!(has(&app, "Z axis"));
    assert!(has(&app, "Z rotation"));
}

#[test]
fn file_only_group_is_listed_as_not_in_this_game_version() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::Joystick(2));
    assert!(has(&app, NOT_IN_BUILD));
    assert!(has(&app, "Flight strafe old"));
    adv(
        &mut app,
        AdvancedMessage::Invert("flight_strafe_old".into(), false),
    );
    assert_eq!(pending(&app), 1);
    assert_eq!(
        applied(&mut app, &path),
        replaced_once(
            ADVANCED,
            "<flight_strafe_old invert=\"1\"/>",
            "<flight_strafe_old invert=\"0\"/>"
        )
    );
}

#[test]
fn without_game_data_advanced_shows_what_the_file_holds() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = advanced_demo(temp.path(), false);
    assert!(has_containing(&app, NO_GAME_DATA_NOTE));
    pick_device(&mut app, DevicePick::Joystick(1));
    assert!(has(&app, "Flight move pitch"));
    assert!(has(&app, "Flight view"));
    assert_eq!(count(&app, "Exponent"), 3);
    assert_eq!(count(&app, "Invert"), 0);
    assert_eq!(count(&app, "Response curve"), 1);
    assert!(has(&app, "Y axis"));
    pick_device(&mut app, DevicePick::Joystick(2));
    assert_eq!(count(&app, "Invert"), 2);
    assert!(!has(&app, NOT_IN_BUILD));
}

#[test]
fn typed_text_survives_an_edit_elsewhere() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, path) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::Joystick(1));
    let exponent = group_field("flight_move_yaw", GroupField::Exponent);
    adv(
        &mut app,
        AdvancedMessage::Typed(exponent.clone(), "3.25".into()),
    );
    adv(
        &mut app,
        AdvancedMessage::OpenCurve("flight_move_pitch".into()),
    );
    adv(
        &mut app,
        AdvancedMessage::PointTyped(0, PointPart::In, "0.33".into()),
    );
    adv(
        &mut app,
        AdvancedMessage::Invert("flight_move_roll".into(), true),
    );
    assert_eq!(pending(&app), 1);
    let fields = field_texts(&app);
    assert!(fields.contains(&"3.25".to_string()), "{fields:?}");
    assert!(fields.contains(&"0.33".to_string()), "{fields:?}");
    assert!(has(&app, "Set"));

    adv(&mut app, AdvancedMessage::Submit(exponent));
    assert_eq!(pending(&app), 2);
    assert!(!has(&app, "Set"), "a staged draft is done with");
    adv(&mut app, AdvancedMessage::SubmitPoints);
    assert_eq!(pending(&app), 3);
    let text = applied(&mut app, &path);
    assert!(
        text.contains("<flight_move_yaw exponent=\"3.25\"/>"),
        "{text}"
    );
    assert_eq!(
        curve_of(&text, DeviceKind::Joystick, 1, "flight_move_pitch"),
        points(&[("0.33", "0.1"), ("0.5", "0.3"), ("0.75", "0.6")])
    );
}

#[test]
fn switching_device_drops_typed_text() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::Joystick(1));
    let exponent = group_field("flight_move_yaw", GroupField::Exponent);
    adv(&mut app, AdvancedMessage::Typed(exponent, "3.25".into()));
    pick_device(&mut app, DevicePick::Joystick(3));
    assert!(!field_texts(&app).contains(&"3.25".to_string()));
}

#[test]
fn gamepad_and_keyboard_show_their_product_names() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = advanced_demo(temp.path(), true);
    assert!(has(&app, "Controller (Gamepad)"));
    assert!(has(&app, "Keyboard"));
    pick_device(&mut app, DevicePick::Gamepad);
    assert_eq!(count(&app, "Controller (Gamepad)"), 2, "list and header");
}

#[test]
fn unset_values_say_game_default_everywhere() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::Joystick(1));
    assert!(!has(&app, "Default"));
    assert!(has(&app, GAME_DEFAULT));
    assert!(has(
        &app,
        "Game default assumed to be 0 for deadzone and 1 for saturation"
    ));
    pick_device(&mut app, DevicePick::Joystick(2));
    assert!(!has(&app, "Default"));
}

#[test]
fn tabs_switch_between_binds_and_advanced_controls() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = advanced_demo(temp.path(), true);
    assert!(has(&app, "Keyboard and mouse"));
    assert!(!has(&app, "Conflicts"));
    clicked(&mut app, BINDS_TAB);
    assert!(!has(&app, "Keyboard and mouse"));
    assert!(has(&app, "Conflicts"));
    clicked(&mut app, ADVANCED_TAB);
    assert!(has(&app, "Keyboard and mouse"));
}

#[test]
fn advanced_controls_pictures() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = advanced_demo(temp.path(), true);
    pick_device(&mut app, DevicePick::Joystick(1));
    let field = group_field("flight_move_pitch", GroupField::Exponent);
    adv(
        &mut app,
        AdvancedMessage::Typed(field.clone(), "2.5".into()),
    );
    adv(&mut app, AdvancedMessage::Submit(field));
    assert!(has(&app, "Was 2"));
    adv(
        &mut app,
        AdvancedMessage::OpenCurve("flight_move_pitch".into()),
    );
    assert_eq!(pending(&app), 1);
    for (choice, name) in [
        (ThemeChoice::Dark, "advanced-controls-dark"),
        (ThemeChoice::Light, "advanced-controls-light"),
    ] {
        testkit::snapshot(&mut sim(&app), &theme::theme(choice), name);
    }
}
