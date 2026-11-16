#[path = "../src/testkit.rs"]
mod testkit;

use iced::Size;
use iced_test::selector::Candidate;
use iced_test::simulator::Simulator;
use sc_app::app::{self, App, Message, Shortcut};
use sc_app::screen::{Screen, Toast};
use sc_app::screens::{config, settings};
use sc_app::theme;
use sc_core::backup::SnapshotId;
use sc_core::install::Install;
use sc_core::process::RunState;
use sc_core::settings::ThemeChoice;
use testkit::{ctrl, feed};

const FULL: Size = Size::new(1280.0, 800.0);
const SMALL: Size = Size::new(1000.0, 640.0);

fn sim(app: &App) -> Simulator<'_, Message> {
    Simulator::with_size(sc_app::settings(), FULL, app.view())
}

fn containing(needle: &'static str) -> impl FnMut(Candidate<'_>) -> Option<String> {
    move |candidate| match candidate {
        Candidate::Text { content, .. } if content.contains(needle) => Some(content.to_string()),
        _ => None,
    }
}

fn clicked(app: &mut App, label: &str) {
    let mut ui = sim(app);
    ui.click(label)
        .unwrap_or_else(|err| panic!("{label}: {err}"));
    let messages: Vec<Message> = ui.into_messages().collect();
    feed(app, messages);
}

#[test]
fn sidebar_lists_every_screen() {
    let temp = tempfile::tempdir().unwrap();
    let app = App::with(testkit::context_with_channels(
        temp.path(),
        &["LIVE", "PTU"],
    ));
    let mut ui = sim(&app);
    for screen in Screen::ALL {
        assert!(ui.find(screen.label()).is_ok(), "{}", screen.label());
    }
}

#[test]
fn clicking_a_sidebar_item_switches_screen() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::with(testkit::context_with_channels(temp.path(), &["LIVE"]));
    clicked(&mut app, "Keybinds");
    assert_eq!(app.screen(), Screen::Keybinds);
}

#[test]
fn ctrl_number_switches_screen() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::with(testkit::context_with_channels(temp.path(), &["LIVE"]));
    let three = app::shortcut(&ctrl("3")).expect("Ctrl+3 is a shortcut");
    feed(&mut app, [Message::Shortcut(three)]);
    assert_eq!(app.screen(), Screen::Keybinds);
    let zero = app::shortcut(&ctrl("0")).expect("Ctrl+0 is a shortcut");
    feed(&mut app, [Message::Shortcut(zero)]);
    assert_eq!(app.screen(), Screen::Settings);
}

#[test]
fn no_install_shows_empty_state_with_choose_folder() {
    let temp = tempfile::tempdir().unwrap();
    let app = App::with(testkit::context_without_install(temp.path()));
    let mut ui = sim(&app);
    assert!(ui.find("No Star Citizen install found").is_ok());
    assert!(ui.find("Choose folder").is_ok());
}

#[test]
fn install_without_channels_shows_empty_state() {
    let temp = tempfile::tempdir().unwrap();
    let app = App::with(testkit::context_without_channels(temp.path()));
    let mut ui = sim(&app);
    assert!(ui.find("No channels in this install").is_ok());
}

#[test]
fn top_bar_shows_install_channel_and_launch() {
    let temp = tempfile::tempdir().unwrap();
    let app = App::with(testkit::context_with_channels(
        temp.path(),
        &["LIVE", "PTU"],
    ));
    let mut ui = sim(&app);
    assert!(ui.find("LIVE").is_ok());
    assert!(ui.find("Game closed").is_ok());
    assert!(ui.find("Set up launch").is_ok());
}

#[test]
fn running_game_shows_read_only_banner() {
    let temp = tempfile::tempdir().unwrap();
    let mut ctx = testkit::context_with_channels(temp.path(), &["LIVE"]);
    ctx.game = RunState::Running;
    let app = App::with(ctx);
    let mut ui = sim(&app);
    assert!(ui.find(app::READ_ONLY_RUNNING).is_ok());
    assert!(ui.find("Game running").is_ok());
}

#[test]
fn unknown_game_state_shows_its_own_banner_and_pill() {
    let temp = tempfile::tempdir().unwrap();
    let mut ctx = testkit::context_with_channels(temp.path(), &["LIVE"]);
    ctx.game = RunState::Unknown;
    let app = App::with(ctx);
    let mut ui = sim(&app);
    assert!(ui.find(app::READ_ONLY_UNKNOWN).is_ok());
    assert!(ui.find("Game status unknown").is_ok());
}

#[test]
fn unlock_needs_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    let mut ctx = testkit::context_with_channels(temp.path(), &["LIVE"]);
    ctx.game = RunState::Running;
    let mut app = App::with(ctx);
    clicked(&mut app, app::UNLOCK_ACTION);
    assert!(!app.context().unlocked);
    assert!(app.dialog().is_some());
    assert!(sim(&app).find(app::UNLOCK_BODY).is_ok());
    clicked(&mut app, "Unlock");
    assert!(app.context().unlocked);
    assert!(app.dialog().is_none());
    assert!(sim(&app).find(app::READ_ONLY_RUNNING).is_err());
}

#[test]
fn first_launch_of_a_script_asks_and_mentions_wine() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::with(testkit::context_with_wine_script(temp.path()));
    clicked(&mut app, "Launch");
    let dialog = app
        .dialog()
        .expect("a confirm is shown before anything runs");
    assert!(dialog.body.contains("sc-launch.sh"), "{}", dialog.body);
    let mut ui = sim(&app);
    let found = ui
        .find(containing(
            "closes every program running in this Wine prefix",
        ))
        .expect("the warning is on screen");
    assert!(found.contains(app::WINE_WARNING));
    assert!(app.toasts().is_empty());
}

#[test]
fn launch_is_disabled_while_game_runs() {
    let temp = tempfile::tempdir().unwrap();
    let mut ctx = testkit::context_with_wine_script(temp.path());
    ctx.game = RunState::Running;
    let app = App::with(ctx);
    let mut ui = sim(&app);
    assert!(ui.find("Running").is_ok());
    ui.click("Running").unwrap();
    assert_eq!(ui.into_messages().count(), 0);
}

#[test]
fn bottom_bar_hidden_without_pending_edits() {
    let temp = tempfile::tempdir().unwrap();
    let app = App::with(testkit::context_with_channels(temp.path(), &["LIVE"]));
    let mut ui = sim(&app);
    assert!(ui.find(containing("unsaved change")).is_err());
}

#[test]
fn bottom_bar_shows_pending_edits_and_applies() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::with(testkit::context_with_channels(temp.path(), &["LIVE"]));
    feed(
        &mut app,
        [
            Message::Navigate(Screen::Settings),
            Message::Settings(settings::Message::Keep(80)),
        ],
    );
    assert!(sim(&app).find("1 unsaved change").is_ok());
    let save = app::shortcut(&ctrl("s")).unwrap();
    assert_eq!(save, Shortcut::Apply);
    feed(&mut app, [Message::Shortcut(save)]);
    assert_eq!(app.context().settings.snapshot_keep, 80);
    assert!(sim(&app).find(containing("unsaved change")).is_err());
}

fn shot(app: &App, size: Size, name: &str) {
    let mut ui = Simulator::with_size(sc_app::settings(), size, app.view());
    testkit::snapshot(&mut ui, &app.theme(), name);
}

fn with_theme(mut ctx: sc_app::context::Context, choice: ThemeChoice) -> sc_app::context::Context {
    ctx.settings.theme = choice;
    ctx
}

#[test]
fn snapshot_renders_dark_and_light() {
    let temp = tempfile::tempdir().unwrap();
    for (choice, name) in [(ThemeChoice::Dark, "dark"), (ThemeChoice::Light, "light")] {
        let ctx = with_theme(
            testkit::context_with_channels(temp.path(), &["LIVE", "PTU"]),
            choice,
        );
        let mut app = App::with(ctx);
        shot(&app, FULL, &format!("shell-{name}"));
        shot(&app, SMALL, &format!("shell-{name}-small"));

        feed(
            &mut app,
            [
                Message::Navigate(Screen::Settings),
                Message::Settings(settings::Message::Keep(80)),
                Message::Settings(settings::Message::Command(
                    "lutris lutris:rungame/star-citizen".into(),
                )),
            ],
        );
        shot(&app, FULL, &format!("settings-{name}"));
        shot(&app, SMALL, &format!("settings-{name}-small"));
    }
    assert!(
        theme::tokens(&theme::theme(ThemeChoice::Light)).text
            != theme::palette(ThemeChoice::Dark).text
    );
}

#[test]
fn snapshot_states() {
    let temp = tempfile::tempdir().unwrap();
    for (choice, name) in [(ThemeChoice::Dark, "dark"), (ThemeChoice::Light, "light")] {
        let mut ctx = with_theme(testkit::context_with_wine_script(temp.path()), choice);
        ctx.game = RunState::Running;
        let mut app = App::with(ctx);
        app.push_toast(
            Toast::success("Applied 3 changes to user.cfg").with_undo(SnapshotId("a".into())),
        );
        app.push_toast(Toast::info("Starting the RSI Launcher"));
        app.push_toast(Toast::error(
            "Could not write attributes.xml: permission denied",
        ));
        shot(&app, FULL, &format!("read-only-toasts-{name}"));
        shot(&app, SMALL, &format!("read-only-toasts-{name}-small"));

        let mut ctx = with_theme(testkit::context_with_wine_script(temp.path()), choice);
        ctx.game = RunState::NotRunning;
        let mut app = App::with(ctx);
        feed(&mut app, [Message::Launch]);
        shot(&app, FULL, &format!("confirm-{name}"));

        let app = App::with(with_theme(
            testkit::context_without_install(temp.path()),
            choice,
        ));
        shot(&app, FULL, &format!("no-install-{name}"));
    }
}

fn stage_fov(app: &mut App) {
    feed(app, [Message::Navigate(Screen::Config)]);
    feed(
        app,
        [Message::Config(config::Message::SetCvar(
            "cl_fov".into(),
            "100".into(),
        ))],
    );
    assert_eq!(app.pending(Screen::Config), 1);
}

#[test]
fn launch_asks_about_edits_on_any_screen_and_never_launches_after_a_failed_apply() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::context_with_wine_script(temp.path());
    let cfg = ctx.channel.as_ref().unwrap().user_cfg();
    std::fs::write(&cfg, "cl_fov = 90\n").unwrap();
    let mut app = App::with(ctx);
    stage_fov(&mut app);
    feed(&mut app, [Message::Navigate(Screen::Keybinds)]);
    feed(&mut app, [Message::Launch]);
    let dialog = app.dialog().expect("asks first");
    assert_eq!(dialog.title, "Apply changes first?");
    assert_eq!(
        dialog.body,
        "Config has 1 unsaved change. Apply them before the game reads its files?"
    );

    feed(
        &mut app,
        [
            Message::Probed(RunState::Running, RunState::NotRunning),
            Message::DialogAction(0),
        ],
    );
    assert!(app.dialog().is_none(), "the launcher was not started");
    assert_eq!(app.pending(Screen::Config), 1);
    assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "cl_fov = 90\n");

    feed(
        &mut app,
        [
            Message::Probed(RunState::NotRunning, RunState::NotRunning),
            Message::Launch,
            Message::DialogAction(0),
        ],
    );
    assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "cl_fov = 100\n");
    assert_eq!(app.pending(Screen::Config), 0);
    assert_eq!(app.dialog().expect("launches").title, "Start the launcher?");
}

#[test]
fn rescan_keeps_the_install_and_staged_edits() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::context_like_startup(temp.path(), &["LIVE"]);
    let root = ctx.install.as_ref().unwrap().root.clone();
    std::fs::write(ctx.channel.as_ref().unwrap().user_cfg(), "cl_fov = 90\n").unwrap();
    let mut app = App::with(ctx);
    stage_fov(&mut app);
    feed(&mut app, [Message::Rescan]);
    assert_eq!(app.context().install.as_ref().unwrap().root, root);
    assert_eq!(app.pending(Screen::Config), 1);
    assert!(app.dialog().is_none());
}

#[cfg(unix)]
#[test]
fn rescan_keeps_an_install_reached_through_a_symlink() {
    let temp = tempfile::tempdir().unwrap();
    let real = testkit::make_install(&temp.path().join("real"), &["LIVE"]);
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(temp.path().join("real"), &link).unwrap();
    let install = Install::from_dir(&link).unwrap();
    let linked = install.root.clone();
    let mut ctx = testkit::context_for(temp.path(), vec![install]);
    ctx.settings.installs = vec![real, link];
    std::fs::write(ctx.channel.as_ref().unwrap().user_cfg(), "cl_fov = 90\n").unwrap();
    let mut app = App::with(ctx);
    stage_fov(&mut app);
    feed(&mut app, [Message::Rescan]);
    assert_eq!(app.context().install.as_ref().unwrap().root, linked);
    assert_eq!(app.pending(Screen::Config), 1);
    assert!(app.dialog().is_none());
}

#[test]
fn removing_the_selected_install_asks_before_dropping_edits() {
    let temp = tempfile::tempdir().unwrap();
    let a = testkit::make_install(&temp.path().join("a"), &["LIVE"]);
    let b = testkit::make_install(&temp.path().join("b"), &["LIVE"]);
    let installs = vec![
        Install::from_dir(&a).unwrap(),
        Install::from_dir(&b).unwrap(),
    ];
    let mut ctx = testkit::context_for(temp.path(), installs);
    ctx.settings.installs = vec![a.clone(), b.clone()];
    std::fs::write(ctx.channel.as_ref().unwrap().user_cfg(), "cl_fov = 90\n").unwrap();
    let mut app = App::with(ctx);
    stage_fov(&mut app);
    feed(
        &mut app,
        [Message::Settings(settings::Message::RemoveFolder(
            a.clone(),
        ))],
    );
    let dialog = app.dialog().expect("asks first");
    assert_eq!(dialog.title, "Discard unsaved changes?");
    assert_eq!(app.context().install.as_ref().unwrap().root, a);
    assert_eq!(app.pending(Screen::Config), 1);
    feed(&mut app, [Message::DialogAction(0)]);
    assert_eq!(app.context().install.as_ref().unwrap().root, b);
    assert_eq!(app.pending(Screen::Config), 0);
}
