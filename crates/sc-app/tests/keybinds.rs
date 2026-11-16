#[path = "../src/testkit.rs"]
mod testkit;

use iced::keyboard::key::{Code, Named, Physical};
use iced::keyboard::{Key, Location, Modifiers};
use iced::{Event, Size, event, keyboard, window};
use iced_test::selector::Candidate;
use iced_test::simulator::Simulator;
use sc_app::app::{App, Message, Shortcut};
use sc_app::context::Context;
use sc_app::screen::{Screen, ToastKind};
use sc_app::screens::keybinds::{
    self, CAPTURE_PROMPT, CellRef, EMPTY_BODY, EMPTY_TITLE, RESET_CAPTION, Source, SourceChoice,
    UNBIND_CAPTION, UNKNOWN_KEY, UNRECOGNISED,
};
use sc_app::theme;
use sc_core::actionmaps::{ActionMaps, DeviceKind, Op, Slot};
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

/// The demo profile in LIVE, with the Keybinds screen open.
fn demo(temp: &Path) -> (App, PathBuf) {
    let ctx = testkit::context_with_channels(temp, &["LIVE"]);
    let path = actionmaps(&ctx);
    write(&path, DEMO);
    (open(ctx), path)
}

fn send(app: &mut App, message: keybinds::Message) {
    feed(app, [Message::Keybinds(message)]);
}

fn slot(kind: DeviceKind, instance: u32) -> Slot {
    Slot { kind, instance }
}

fn cell(map: &str, action: &str, kind: DeviceKind, instance: u32) -> CellRef {
    CellRef {
        map: map.into(),
        action: action.into(),
        slot: slot(kind, instance),
    }
}

fn select(app: &mut App, map: &str, action: &str, kind: DeviceKind, instance: u32) {
    send(
        app,
        keybinds::Message::Select(cell(map, action, kind, instance)),
    );
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
    assert_eq!(following(&texts, "All", 2), ["All", "81"]);
    assert_eq!(
        following(&texts, "Spaceship movement", 2),
        ["Spaceship movement", "24"]
    );
    assert_eq!(
        following(&texts, "Spaceship weapons", 2),
        ["Spaceship weapons", "11"]
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
fn table_has_a_column_per_device_slot_in_order() {
    let temp = tempfile::tempdir().unwrap();
    let (app, _) = demo(temp.path());
    assert_eq!(
        following(&texts(&app), "Action", 5),
        ["Action", "Keyboard", "Mouse", "Joystick 1", "Joystick 2"]
    );
    assert!(has(&app, "Strafe forward"));
    assert!(has(&app, "v_strafe_forward"));
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
        1,
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
        1,
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
        1,
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
        2,
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
        1,
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
        1,
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
fn unbind_is_disabled_with_its_caption() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = demo(temp.path());
    select(
        &mut app,
        "spaceship_movement",
        "v_strafe_up",
        DeviceKind::Keyboard,
        1,
    );
    assert!(has(&app, UNBIND_CAPTION));
    assert!(clicks(&app, "Unbind").is_empty());
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
        1,
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
        1,
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
        1,
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
        2,
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
        2,
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
    select(&mut app, "player", "jump", DeviceKind::Keyboard, 1);
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
        1,
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
        1,
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
    let (mut app, _) = demo(temp.path());
    send(
        &mut app,
        keybinds::Message::PickMap(Some("spaceship_weapons".into())),
    );
    select(
        &mut app,
        "spaceship_weapons",
        "v_attack1_group1",
        DeviceKind::Mouse,
        1,
    );
    send(&mut app, keybinds::Message::Typed("mouse5".into()));
    clicked(&mut app, "Set");
    send(&mut app, keybinds::Message::ClosePanel);
    assert!(has(&app, "Was Left click"));
    for (choice, name) in [
        (ThemeChoice::Dark, "keybinds-dark"),
        (ThemeChoice::Light, "keybinds-light"),
    ] {
        let mut ui = sim(&app);
        testkit::snapshot(&mut ui, &theme::theme(choice), name);
    }

    let dark = theme::theme(ThemeChoice::Dark);
    select(
        &mut app,
        "spaceship_weapons",
        "v_weapon_launch_missile",
        DeviceKind::Mouse,
        1,
    );
    assert!(has(&app, CAPTURE_PROMPT));
    testkit::snapshot(&mut sim(&app), &dark, "keybinds-edit-dark");

    select(
        &mut app,
        "spaceship_weapons",
        "v_attack1_group2",
        DeviceKind::Joystick,
        1,
    );
    assert!(has(&app, "Buttons"));
    testkit::snapshot(&mut sim(&app), &dark, "keybinds-joystick-dark");
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
        1,
    );
    send(&mut app, keybinds::Message::TableResized(240.0));
    assert!(has(&app, "v_weapon_toggle_gimbal_mode"));
}
