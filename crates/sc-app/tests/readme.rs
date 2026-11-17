//! Pictures of every screen from the demo story. Both tests are ignored so normal runs stay
//! fast; each picture takes 20 to 30 seconds to render.
//!
//! `cargo test -p sc-app --test readme readme_pictures -- --ignored` writes the README set
//! into `docs/screenshots/`. `every_screen` renders each screen dark and light into
//! `target/snapshots/` for a side by side look.

#[path = "../src/testkit.rs"]
mod testkit;

use iced::Size;
use iced_test::simulator::Simulator;
use sc_app::app::{App, Message};
use sc_app::screen::Screen;
use sc_app::screens::{backups, channels, comms, config, keybinds, maintenance, starstrings};
use sc_app::theme;
use sc_core::actionmaps::DeviceKind;
use sc_core::channels::{CopyPlan, PlannedFile};
use sc_core::settings::ThemeChoice;
use std::fs;
use std::path::{Path, PathBuf};

const FULL: Size = Size::new(1280.0, 800.0);
const GB: u64 = 1024 * 1024 * 1024;
const MB: u64 = 1024 * 1024;

fn feed(app: &mut App, messages: impl IntoIterator<Item = Message>) {
    testkit::feed(app, messages);
}

fn open(app: &mut App, screen: Screen) {
    feed(app, [Message::Navigate(screen)]);
}

/// Renders the app as it stands into `target/snapshots/<name>-tiny-skia.png`.
fn shoot(app: &App, choice: ThemeChoice, name: &str) -> PathBuf {
    let mut ui = Simulator::with_size(sc_app::settings(), FULL, app.view());
    testkit::snapshot(&mut ui, &theme::theme(choice), name);
    testkit::snapshot_dir().join(format!("{name}-tiny-skia.png"))
}

fn newer_release() -> sc_core::starstrings::Release {
    sc_core::starstrings::Release {
        name: "SC LIVE Build (release-2026-11-12-2c6c06f)".into(),
        published_at: "2026-11-12T17:31:51Z".into(),
        ..testkit::demo_release()
    }
}

/// Each screen in its most telling state, given the demo story.
fn stage(app: &mut App, screen: Screen, temp: &Path) {
    open(app, screen);
    match screen {
        Screen::Config => feed(
            app,
            [
                ("cl_fov", "100"),
                ("r_DepthOfField", "0"),
                ("r_vignetteBlur", "0"),
            ]
            .map(|(key, value)| {
                Message::Config(config::Message::SetCvar(key.into(), value.into()))
            }),
        ),
        Screen::Keybinds => {
            let cell = |action: &str, kind| {
                Message::Keybinds(keybinds::Message::Select(keybinds::CellRef {
                    map: "spaceship_weapons".into(),
                    action: action.into(),
                    kind,
                }))
            };
            feed(
                app,
                [
                    Message::Keybinds(keybinds::Message::PickMap(Some("spaceship_weapons".into()))),
                    cell("v_attack1_group1", DeviceKind::Mouse),
                    Message::Keybinds(keybinds::Message::Typed("mouse5".into())),
                    Message::Keybinds(keybinds::Message::Set),
                    cell("v_weapon_launch_missile", DeviceKind::Joystick),
                ],
            );
        }
        Screen::Comms => feed(
            app,
            [
                ("HeadtrackingGlobalSmoothingPosition", "0.35"),
                ("HeadtrackingDisableDuringWalking", "0"),
            ]
            .map(|(name, value)| Message::Comms(comms::Message::Set(name.into(), value.into()))),
        ),
        Screen::StarStrings => feed(
            app,
            [Message::StarStrings(starstrings::Message::Checked(Ok(
                Some(newer_release()),
            )))],
        ),
        Screen::Backups => feed(
            app,
            [Message::Backups(backups::Message::OpenFile(
                "user.cfg".into(),
            ))],
        ),
        Screen::Maintenance => {
            let older = testkit::shader_cache_dir(temp).join(testkit::SHADER_OLDER);
            feed(
                app,
                ["shaders", "vulkanshadercache"].map(|kind| {
                    Message::Maintenance(maintenance::Message::Tick(older.join(kind), true))
                }),
            );
        }
        Screen::Channels => show_plan(app),
        _ => {}
    }
}

/// A plan that reads like a real LIVE to PTU copy, with the big archive first.
fn show_plan(app: &mut App) {
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
    let plan = Box::new(CopyPlan {
        source: root.join("LIVE"),
        target: root.join("PTU"),
        files,
        bytes_to_copy: 96 * GB + 410 * MB,
        largest_copy: 95 * GB,
        free_bytes: Some(148 * GB + 100 * MB),
        needs_in_place: false,
    });
    let _ = app.update(Message::Channels(channels::Message::PlanCopy));
    let _ = app.update(Message::Channels(channels::Message::CopyPlanned(
        "LIVE".into(),
        "PTU".into(),
        false,
        Ok(plan),
    )));
}

fn demo_app(temp: &Path) -> App {
    let ctx = testkit::demo_story(temp);
    testkit::seed_demo_game_data(&ctx);
    let mut app = App::with(ctx);
    open(&mut app, Screen::Overview);
    app
}

fn screenshots_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots")
}

/// (screen, README file name) for the dark-only pictures.
const README: [(Screen, &str); 8] = [
    (Screen::Config, "config"),
    (Screen::Keybinds, "keybinds"),
    (Screen::Comms, "comms"),
    (Screen::StarStrings, "starstrings"),
    (Screen::Screenshots, "screenshots"),
    (Screen::Backups, "backups"),
    (Screen::Maintenance, "maintenance"),
    (Screen::Channels, "channels"),
];

#[test]
#[ignore = "renders the README pictures; about 30 s each"]
fn readme_pictures() {
    let out = screenshots_dir();
    fs::create_dir_all(&out).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut app = demo_app(temp.path());
    for (choice, name) in [
        (ThemeChoice::Dark, "overview-dark"),
        (ThemeChoice::Light, "overview-light"),
    ] {
        let shot = shoot(&app, choice, &format!("readme-{name}"));
        fs::copy(shot, out.join(format!("{name}.png"))).unwrap();
    }
    for (screen, name) in README {
        stage(&mut app, screen, temp.path());
        let shot = shoot(&app, ThemeChoice::Dark, &format!("readme-{name}"));
        fs::copy(shot, out.join(format!("{name}.png"))).unwrap();
        feed(&mut app, [Message::Discard]);
    }
}

#[test]
#[ignore = "renders every screen dark and light for review; about 10 minutes"]
fn every_screen() {
    let only = std::env::var("SCREENS").unwrap_or_default();
    for choice in [ThemeChoice::Dark, ThemeChoice::Light] {
        let temp = tempfile::tempdir().unwrap();
        let mut app = demo_app(temp.path());
        let theme_name = if choice == ThemeChoice::Dark {
            "dark"
        } else {
            "light"
        };
        for screen in Screen::ALL {
            let name = screen.label().to_lowercase().replace(' ', "-");
            if !only.is_empty() && !only.split(',').any(|wanted| wanted == name) {
                continue;
            }
            stage(&mut app, screen, temp.path());
            shoot(&app, choice, &format!("all-{name}-{theme_name}"));
            feed(&mut app, [Message::Discard]);
        }
    }
}
