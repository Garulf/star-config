#[path = "../src/testkit.rs"]
mod testkit;

use iced::Size;
use iced_test::simulator::Simulator;
use sc_app::app::{App, Message};
use sc_app::screen::Screen;
use sc_app::screens::{comms, config};
use sc_core::install::Channel;
use sc_core::settings::ThemeChoice;
use std::fs;
use tempfile::TempDir;
use testkit::{demo_attributes, feed};

const FULL: Size = Size::new(1280.0, 800.0);
const NOT_RECOGNISED: &str =
    "This file's format is not recognised, so star-config will not change it.";
const SOURCE_HINT: &str = "Which tracker the game uses. What each number means has not been confirmed, so this is the number stored in the file.";

struct Fixture {
    _temp: TempDir,
    app: App,
    channel: Channel,
}

fn fixture_with(attributes: Option<&str>, theme: ThemeChoice) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let mut ctx = testkit::context_with_channels(temp.path(), &["LIVE"]);
    ctx.settings.theme = theme;
    let channel = ctx.channel.clone().unwrap();
    testkit::write_channel_files(&channel, None, attributes.map(str::as_bytes));
    let mut app = App::with(ctx);
    feed(&mut app, [Message::Navigate(Screen::Comms)]);
    Fixture {
        _temp: temp,
        app,
        channel,
    }
}

fn game_fixture(attributes: &str) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::context_with_channels(temp.path(), &["LIVE"]);
    let channel = ctx.channel.clone().unwrap();
    testkit::write_build(&channel, "4.1.0", "1234");
    testkit::seed_game_data_with(
        &ctx,
        include_str!("fixtures/config_game/attributes.xml"),
        include_str!("fixtures/config_game/global.ini"),
    );
    testkit::write_channel_files(&channel, None, Some(attributes.as_bytes()));
    let mut app = App::with(ctx);
    feed(&mut app, [Message::Navigate(Screen::Comms)]);
    Fixture {
        _temp: temp,
        app,
        channel,
    }
}

fn fixture(attributes: &str) -> Fixture {
    fixture_with(Some(attributes), ThemeChoice::Dark)
}

fn send(fx: &mut Fixture, messages: impl IntoIterator<Item = comms::Message>) {
    feed(&mut fx.app, messages.into_iter().map(Message::Comms));
}

fn set(name: &str, value: &str) -> comms::Message {
    comms::Message::Set(name.into(), value.into())
}

fn apply(fx: &mut Fixture) {
    feed(&mut fx.app, [Message::Apply]);
}

fn has(app: &App, needle: &str) -> bool {
    Simulator::with_size(sc_app::settings(), FULL, app.view())
        .find(needle)
        .is_ok()
}

fn on_disk(fx: &Fixture) -> String {
    fs::read_to_string(fx.channel.attributes_xml()).unwrap()
}

fn without_line(text: &str, needle: &str) -> String {
    text.lines()
        .filter(|line| !line.contains(needle))
        .map(|line| format!("{line}\n"))
        .collect()
}

fn pending(fx: &Fixture) -> usize {
    fx.app.pending(Screen::Comms)
}

#[test]
fn catalogue_groups_and_typed_controls_render() {
    let fx = fixture(&demo_attributes());
    for text in [
        "HEAD TRACKING",
        "HEAD TRACKING: WHEN IT IS OFF",
        "HEAD TRACKING: THIRD PERSON",
        "Position smoothing",
        "Off in mobiGlas",
        "HeadtrackingGlobalSmoothingPosition",
        "0.25",
        "These are the game's own settings. star-config does not track your head. It sets what the game does with a tracker you already use.",
    ] {
        assert!(has(&fx.app, text), "{text}");
    }
}

#[test]
fn comms_labels_survive_without_game_data() {
    let fx = fixture(&demo_attributes());
    for text in [
        "Source",
        "Enabled",
        "Position smoothing",
        "Off while seated",
        "Off in inventory",
        "HEAD TRACKING: THIRD PERSON",
    ] {
        assert!(has(&fx.app, text), "{text}");
    }
}

#[test]
fn comms_takes_in_game_labels_and_defaults_from_the_build() {
    let fx = game_fixture(&demo_attributes());
    assert!(has(&fx.app, "Tracking device"), "label from the build");
    assert!(has(&fx.app, "Smoothing of position"));
    assert!(
        has(&fx.app, "Enabled"),
        "no label key: our fallback wording"
    );
    assert!(!has(&fx.app, "Source"));
    assert!(has(&fx.app, "HEAD TRACKING: WHEN IT IS OFF"));
}

#[test]
fn headtracking_source_is_a_plain_number_with_the_note() {
    let fx = fixture(&demo_attributes());
    assert!(has(&fx.app, SOURCE_HINT));
    assert!(has(&fx.app, "Source"));
    for invented in ["TrackIR", "Tobii", "Faceware"] {
        assert!(!has(&fx.app, invented), "{invented}");
    }
}

#[test]
fn setting_a_missing_attribute_adds_the_element() {
    let original = without_line(&demo_attributes(), "HeadtrackingInactivityTime");
    let mut fx = fixture(&original);
    assert!(has(&fx.app, "Not set. The game uses its default."));
    send(&mut fx, [set("HeadtrackingInactivityTime", "3.5")]);
    assert!(has(&fx.app, "Was not set"));
    apply(&mut fx);
    let written = on_disk(&fx);
    assert!(written.contains("<Attr name=\"HeadtrackingInactivityTime\" value=\"3.5\"/>"));
    assert_eq!(
        without_line(&written, "HeadtrackingInactivityTime"),
        original
    );
}

#[test]
fn other_settings_lists_only_comms_related_names() {
    let fx = fixture(&demo_attributes());
    assert!(has(&fx.app, "OTHER SETTINGS IN THIS FILE"));
    for name in [
        "VoipPushToTalk",
        "MicSensitivity",
        "FoipEnabled",
        "HmdCursorSensitivity",
    ] {
        assert!(has(&fx.app, name), "{name}");
    }
    for name in ["Brightness", "FOV", "MouseYawSensitivity"] {
        assert!(!has(&fx.app, name), "{name}");
    }
}

#[test]
fn add_a_setting_then_apply() {
    let original = demo_attributes();
    let mut fx = fixture(&original);
    send(
        &mut fx,
        [
            comms::Message::OpenAdd,
            comms::Message::AddName("VoipNoiseGate".into()),
            comms::Message::AddValue("0.2".into()),
            comms::Message::Add,
        ],
    );
    assert_eq!(pending(&fx), 1);
    assert!(has(&fx.app, "VoipNoiseGate"));
    apply(&mut fx);
    let written = on_disk(&fx);
    assert!(written.contains("<Attr name=\"VoipNoiseGate\" value=\"0.2\"/>"));
    assert_eq!(without_line(&written, "VoipNoiseGate"), original);
}

#[test]
fn unrecognised_file_is_read_only() {
    let original = format!("<!DOCTYPE Attributes>\n{}", demo_attributes());
    let mut fx = fixture(&original);
    assert!(has(&fx.app, NOT_RECOGNISED));
    assert!(has(&fx.app, "Position smoothing"));
    send(&mut fx, [set("HeadtrackingToggle", "0")]);
    assert_eq!(pending(&fx), 0);
    apply(&mut fx);
    assert_eq!(on_disk(&fx), original);
}

#[test]
fn missing_file_shows_the_empty_state() {
    let fx = fixture_with(None, ThemeChoice::Dark);
    assert!(has(&fx.app, "No game settings yet"));
    assert!(!fx.channel.attributes_xml().exists());
}

#[test]
fn edits_survive_the_other_screen_applying_first() {
    let mut fx = fixture(&demo_attributes());
    send(&mut fx, [set("HeadtrackingToggle", "0")]);
    feed(
        &mut fx.app,
        [
            Message::Navigate(Screen::Config),
            Message::Config(config::Message::SetAttr("FOV".into(), "100".into())),
            Message::Apply,
            Message::Navigate(Screen::Comms),
        ],
    );
    assert!(on_disk(&fx).contains("<Attr name=\"FOV\" value=\"100\"/>"));
    assert_eq!(pending(&fx), 1, "comms edits were kept");
    apply(&mut fx);
    let written = on_disk(&fx);
    assert!(
        written.contains("<Attr name=\"FOV\" value=\"100\"/>"),
        "{written}"
    );
    assert!(
        written.contains("<Attr name=\"HeadtrackingToggle\" value=\"0\"/>"),
        "{written}"
    );
    assert_eq!(pending(&fx), 0);
    assert!(fx.app.toasts().last().unwrap().undo.is_some());
}

#[test]
fn snapshots() {
    for (theme, name) in [(ThemeChoice::Dark, "dark"), (ThemeChoice::Light, "light")] {
        let mut fx = fixture_with(Some(&demo_attributes()), theme);
        send(
            &mut fx,
            [
                set("HeadtrackingGlobalSmoothingPosition", "0.35"),
                set("HeadtrackingDisableDuringWalking", "0"),
            ],
        );
        let mut ui = Simulator::with_size(sc_app::settings(), FULL, fx.app.view());
        testkit::snapshot(&mut ui, &fx.app.theme(), &format!("comms-{name}"));
    }
}
