#[path = "../src/testkit.rs"]
mod testkit;

use iced::Size;
use iced_test::selector::Candidate;
use iced_test::simulator::Simulator;
use sc_app::app::{App, Message};
use sc_app::context::Context;
use sc_app::screen::Screen;
use sc_app::screens::overview::{self, Area, Shown, UNREADABLE};
use sc_core::error::Result;
use sc_core::starstrings::Http;
use std::fs;
use std::sync::Arc;

const FULL: Size = Size::new(1280.0, 800.0);

fn open(ctx: Context) -> App {
    let mut app = App::with(ctx);
    testkit::feed(&mut app, [Message::Navigate(Screen::Overview)]);
    app
}

fn sim(app: &App) -> Simulator<'_, Message> {
    Simulator::with_size(sc_app::settings(), FULL, app.view())
}

fn texts(app: &App) -> Vec<String> {
    let mut found = Vec::new();
    let _ = sim(app).find(|candidate: Candidate<'_>| {
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

fn card(state: &overview::State, area: Area) -> Shown {
    state
        .cards()
        .into_iter()
        .find(|(card, _)| *card == area)
        .map(|(_, shown)| shown)
        .unwrap()
}

/// Loads the Overview of `ctx` straight into a screen state, the way the shell would.
fn load(ctx: &Context) -> overview::State {
    let mut state = overview::State::new();
    let task = state.reload(ctx);
    for message in run(task) {
        let _ = state.update(message, ctx);
    }
    state
}

fn run(task: iced::Task<overview::Message>) -> Vec<overview::Message> {
    use iced::futures::StreamExt;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let Some(stream) = iced_test::runtime::task::into_stream(task) else {
        return Vec::new();
    };
    runtime
        .block_on(stream.collect::<Vec<_>>())
        .into_iter()
        .filter_map(|action| match action {
            iced_test::runtime::Action::Output(message) => Some(message),
            _ => None,
        })
        .collect()
}

fn figure(shown: &Shown) -> &str {
    match shown {
        Shown::Ready(card) => &card.figure,
        other => panic!("not ready: {other:?}"),
    }
}

fn attention(shown: &Shown) -> bool {
    matches!(shown, Shown::Ready(card) if card.attention)
}

#[test]
fn cards_show_for_every_area() {
    let temp = tempfile::tempdir().unwrap();
    let app = open(testkit::demo_story(temp.path()));
    for area in Area::ALL {
        assert!(has(&app, area.title()), "{}", area.title());
        assert!(has(&app, area.button()), "{}", area.button());
    }
    assert!(has(&app, "~/Games/star-citizen, LIVE 4.3.2 (9876543)"));
    assert!(has(&app, "Game closed"));
    assert!(has(&app, "14 screenshots"));
    assert!(has(&app, "9 settings in user.cfg"));
    assert!(has(&app, "2 channels"));
    assert!(has(&app, "LIVE 4.3.2 (9876543)"));
    assert!(has(&app, "PTU 4.4.0-PTU (9912345)"));
    assert!(!has_containing(&app, overview::LOADING));
}

#[test]
fn starstrings_card_states() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::demo_base(temp.path());
    assert_eq!(
        figure(&card(&load(&ctx), Area::StarStrings)),
        "Not installed"
    );

    testkit::install_starstrings(&ctx, testkit::LIVE_CHANGE);
    let mut state = load(&ctx);
    assert_eq!(figure(&card(&state, Area::StarStrings)), "Installed");
    state.set_release(Some(testkit::demo_release()));
    let up_to_date = card(&state, Area::StarStrings);
    assert_eq!(figure(&up_to_date), "Up to date");
    assert!(!attention(&up_to_date));

    let mut newer = testkit::demo_release();
    newer.name = "SC LIVE Build (release-2026-11-12-2c6c06f)".into();
    newer.published_at = "2026-11-12T17:31:51Z".into();
    state.set_release(Some(newer));
    assert_eq!(figure(&card(&state, Area::StarStrings)), "Update available");

    testkit::write_build(
        &testkit::channel_named(&ctx, "LIVE"),
        testkit::LIVE_VERSION,
        "9999999",
    );
    let updated = card(&load(&ctx), Area::StarStrings);
    assert_eq!(figure(&updated), "Game updated since install");
    assert!(attention(&updated));
}

#[test]
fn keybinds_card_counts_binds_and_flags_conflicts() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::demo_base(temp.path());
    testkit::demo_player_files(&ctx);
    let state = load(&ctx);
    let Shown::Ready(keybinds) = card(&state, Area::Keybinds) else {
        panic!("keybinds card not ready");
    };
    assert!(
        keybinds.figure.ends_with("custom binds"),
        "{}",
        keybinds.figure
    );
    let count: usize = keybinds.figure.split(' ').next().unwrap().parse().unwrap();
    assert!(count > 50, "{count}");
    assert_eq!(keybinds.lines[0], "Keyboard, mouse, 2 joysticks");
    assert_eq!(keybinds.lines[1], "1 conflict");
    assert!(keybinds.attention);

    let maps = testkit::channel_named(&ctx, "LIVE").actionmaps_xml();
    let one = "<ActionMaps version=\"1\">\n <ActionProfiles version=\"1\" profileName=\"default\">\n  <actionmap name=\"spaceship_weapons\">\n   <action name=\"v_attack1_group1\">\n    <rebind input=\"mo1_mouse5\"/>\n   </action>\n  </actionmap>\n </ActionProfiles>\n</ActionMaps>\n";
    fs::write(&maps, one).unwrap();
    let Shown::Ready(single) = card(&load(&ctx), Area::Keybinds) else {
        panic!("keybinds card not ready");
    };
    assert_eq!(single.figure, "1 custom bind");
    assert!(!single.attention);
}

#[test]
fn missing_files_show_the_not_yet_states_not_errors() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::demo_base(temp.path());
    let state = load(&ctx);
    let expected = [
        (Area::StarStrings, "Not installed"),
        (Area::Config, "No user.cfg yet"),
        (Area::Keybinds, "No custom binds yet"),
        (Area::Comms, "Not set up"),
        (Area::Screenshots, "No screenshots yet"),
        (Area::Backups, "No snapshots yet"),
        (Area::Shaders, "No shader caches"),
        (Area::Channels, "2 channels"),
    ];
    for (area, text) in expected {
        let shown = card(&state, area);
        assert_eq!(figure(&shown), text, "{area:?}");
        assert!(!attention(&shown), "{area:?}");
    }
}

#[test]
fn interrupted_snapshot_marks_backups_card_for_attention() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::demo_base(temp.path());
    testkit::demo_player_files(&ctx);
    testkit::demo_backups(&ctx);
    let calm = card(&load(&ctx), Area::Backups);
    assert!(figure(&calm).starts_with("Last snapshot "));
    assert!(!attention(&calm));

    let live = testkit::channel_named(&ctx, "LIVE");
    sc_core::backup::BackupStore::new(ctx.dirs.backups())
        .begin(
            &ctx.scope().unwrap(),
            &live,
            &[live.user_cfg()],
            "Config changes",
        )
        .unwrap();
    let Shown::Ready(flagged) = card(&load(&ctx), Area::Backups) else {
        panic!("backups card not ready");
    };
    assert!(flagged.attention);
    assert_eq!(flagged.lines[0], "6 snapshots");
    assert_eq!(flagged.lines[1], "1 change was interrupted");
}

#[test]
fn attention_cards_sort_first() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::demo_story(temp.path());
    let state = load(&ctx);
    let cards = state.cards();
    let flags: Vec<bool> = cards.iter().map(|(_, shown)| attention(shown)).collect();
    let first_calm = flags.iter().position(|flag| !flag).unwrap();
    assert!(first_calm >= 2, "{flags:?}");
    assert!(flags[first_calm..].iter().all(|flag| !flag), "{flags:?}");
    let order: Vec<Area> = cards.iter().map(|(area, _)| *area).collect();
    assert_eq!(&order[..2], &[Area::StarStrings, Area::Keybinds]);
}

#[test]
fn card_button_switches_screen() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = open(testkit::demo_story(temp.path()));
    let mut ui = sim(&app);
    ui.click("Edit keybinds").unwrap();
    let messages: Vec<Message> = ui.into_messages().collect();
    testkit::feed(&mut app, messages);
    assert_eq!(app.screen(), Screen::Keybinds);
    assert!(has(&app, "Spaceship weapons"));
}

#[test]
fn one_failing_card_does_not_blank_the_page() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::demo_base(temp.path());
    testkit::demo_player_files(&ctx);
    let live = testkit::channel_named(&ctx, "LIVE");
    fs::write(
        live.actionmaps_xml(),
        "<ActionMaps>\n <actionmap name=\"x\">\n",
    )
    .unwrap();
    let app = open(ctx);
    assert!(has(&app, UNREADABLE));
    assert!(has_containing(&app, "actionmaps.xml"));
    assert!(has_containing(&app, "in user.cfg"));
    assert!(has(&app, "Head tracking on"));
}

struct PanicHttp;

impl Http for PanicHttp {
    fn get_text(&self, url: &str) -> Result<Option<String>> {
        panic!("the Overview asked the network for {url}");
    }

    fn get_bytes(&self, url: &str, _max_bytes: u64) -> Result<Vec<u8>> {
        panic!("the Overview asked the network for {url}");
    }
}

#[test]
fn no_network_call_is_made() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::demo_story(temp.path());
    let mut app = App::with_http(ctx, Arc::new(PanicHttp));
    testkit::feed(&mut app, [Message::Navigate(Screen::StarStrings)]);
    testkit::feed(&mut app, [Message::Navigate(Screen::Overview)]);
    assert!(has(&app, "Game updated since install"));
}

#[test]
fn overview_pictures() {
    let temp = tempfile::tempdir().unwrap();
    let app = open(testkit::demo_story(temp.path()));
    for (choice, name) in [
        (sc_core::settings::ThemeChoice::Dark, "overview-dark"),
        (sc_core::settings::ThemeChoice::Light, "overview-light"),
    ] {
        let mut ui = sim(&app);
        testkit::snapshot(&mut ui, &sc_app::theme::theme(choice), name);
    }
    let mut small = Simulator::with_size(sc_app::settings(), Size::new(1000.0, 640.0), app.view());
    testkit::snapshot(
        &mut small,
        &sc_app::theme::theme(sc_core::settings::ThemeChoice::Dark),
        "overview-dark-small",
    );
}
