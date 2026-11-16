#[path = "../src/testkit.rs"]
mod testkit;

use chrono::{Local, NaiveDateTime, TimeDelta};
use iced::futures::StreamExt;
use iced::keyboard::{self, Key, Location, Modifiers, key};
use iced::widget::text;
use iced::{Element, Size, Task};
use iced_test::runtime;
use iced_test::selector::Candidate;
use iced_test::simulator::Simulator;
use sc_app::app::{self, App};
use sc_app::context::Context;
use sc_app::screen::{Action, Confirm, Screen, Toast, ToastKind};
use sc_app::screens::screenshots::{
    Clipboard, ClipboardCell, FileAction, IMAGE_COPIED, MOVED_TO_TRASH, Message, NO_ARCHIVE,
    NO_CLIPBOARD, STAYS_ON_CLIPBOARD, Services, Source, State, grid,
};
use sc_core::process::{FixedProbe, RunState};
use sc_core::settings::{ArchiveMode, ThemeChoice};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

const FULL: Size = Size::new(1280.0, 800.0);
const EMPTY_BODY: &str =
    "Press Print Screen in the game. Captures from every channel of this install show up here.";

// Fakes -----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Clip {
    Image {
        width: usize,
        height: usize,
        bytes: usize,
    },
    Text(String),
}

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<Clip>>>);

impl Clipboard for Recorder {
    fn set_image(&mut self, width: usize, height: usize, rgba: Vec<u8>) -> Result<(), String> {
        self.0.lock().unwrap().push(Clip::Image {
            width,
            height,
            bytes: rgba.len(),
        });
        Ok(())
    }

    fn set_text(&mut self, text: String) -> Result<(), String> {
        self.0.lock().unwrap().push(Clip::Text(text));
        Ok(())
    }
}

type Log = Arc<Mutex<Vec<PathBuf>>>;

fn trash_into(bin: PathBuf, log: Log) -> FileAction {
    Arc::new(move |path: &Path| {
        fs::create_dir_all(&bin).map_err(|err| err.to_string())?;
        fs::rename(path, bin.join(path.file_name().unwrap())).map_err(|err| err.to_string())?;
        log.lock().unwrap().push(path.to_path_buf());
        Ok(())
    })
}

fn record_into(log: Log) -> FileAction {
    Arc::new(move |path: &Path| {
        log.lock().unwrap().push(path.to_path_buf());
        Ok(())
    })
}

// Files -----------------------------------------------------------------------

/// Unix time of `hour:minute` local time, `days_ago` days before today.
fn local_unix(days_ago: i64, hour: u32, minute: u32) -> i64 {
    local_time(days_ago, hour, minute)
        .and_local_timezone(Local)
        .earliest()
        .unwrap()
        .timestamp()
}

fn local_time(days_ago: i64, hour: u32, minute: u32) -> NaiveDateTime {
    (Local::now().date_naive() - TimeDelta::days(days_ago))
        .and_hms_opt(hour, minute, 8)
        .unwrap()
}

fn set_time(path: &Path, unix: i64) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(unix as u64))
        .unwrap();
}

fn shot_name(days_ago: i64, hour: u32, minute: u32) -> String {
    local_time(days_ago, hour, minute)
        .format("ScreenShot-%Y-%m-%d_%H-%M-%S.jpg")
        .to_string()
}

// Harness ---------------------------------------------------------------------

#[derive(Default)]
struct Seen {
    toasts: Vec<Toast>,
    confirm: Option<Confirm<Message>>,
}

struct Harness {
    temp: tempfile::TempDir,
    ctx: Context,
    state: State,
    clips: Recorder,
    trashed: Log,
    revealed: Log,
    seen: Seen,
}

impl Harness {
    fn new() -> Self {
        Self::with_clipboard(|clips| ClipboardCell::with(clips.clone()))
    }

    fn with_clipboard(clipboard: impl FnOnce(&Recorder) -> ClipboardCell) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut ctx = testkit::context_with_channels(temp.path(), &["LIVE", "PTU"]);
        ctx.settings.screenshots.archive_dir =
            Some(testkit::home(temp.path()).join("Pictures/Star Citizen"));
        let clips = Recorder::default();
        let trashed = Log::default();
        let revealed = Log::default();
        let services = Services {
            clipboard: clipboard(&clips),
            trash: trash_into(temp.path().join("trash"), trashed.clone()),
            reveal: record_into(revealed.clone()),
            default_archive: None,
        };
        Self {
            state: State::with_services(services),
            temp,
            ctx,
            clips,
            trashed,
            revealed,
            seen: Seen::default(),
        }
    }

    fn folder(&self, channel: &str) -> PathBuf {
        self.ctx
            .channels
            .iter()
            .find(|c| c.name == channel)
            .unwrap()
            .screenshots_dir()
    }

    fn archive(&self) -> PathBuf {
        self.ctx.settings.screenshots.archive_dir.clone().unwrap()
    }

    /// A small space picture in `channel`'s folder (or the archive), taken at the given time.
    fn picture(&self, channel: &str, name: &str, unix: i64) -> PathBuf {
        let dir = if channel == "Archive" {
            self.archive()
        } else {
            self.folder(channel)
        };
        let path = dir.join(name);
        testkit::space_scene(&path, 320, 180, unix as u64);
        set_time(&path, unix);
        path
    }

    fn file(&self, channel: &str, name: &str, bytes: &[u8], unix: i64) -> PathBuf {
        let dir = self.folder(channel);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, bytes).unwrap();
        set_time(&path, unix);
        path
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
        self.texts().iter().any(|text| text.contains(needle))
    }

    /// Every text on screen, in layout order.
    fn texts(&self) -> Vec<String> {
        let found = Arc::new(Mutex::new(Vec::new()));
        let sink = found.clone();
        let _ = self.sim().find(move |candidate: Candidate<'_>| {
            if let Candidate::Text { content, .. } = candidate {
                sink.lock().unwrap().push(content.to_string());
            }
            None::<()>
        });
        found.lock().unwrap().clone()
    }

    /// The messages a click on `label` produces, without sending them.
    fn press(&self, label: &str) -> Vec<Message> {
        let mut ui = self.sim();
        ui.click(label)
            .unwrap_or_else(|err| panic!("{label}: {err}"));
        ui.into_messages().collect()
    }

    fn click(&mut self, label: &str) -> usize {
        let messages = self.press(label);
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

    fn last_toast(&self) -> &Toast {
        self.seen.toasts.last().expect("a toast")
    }

    fn clips(&self) -> Vec<Clip> {
        self.clips.0.lock().unwrap().clone()
    }
}

fn key_press(named: key::Named, code: key::Code) -> iced::Event {
    iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Named(named),
        modified_key: Key::Named(named),
        physical_key: key::Physical::Code(code),
        location: Location::Standard,
        modifiers: Modifiers::empty(),
        text: None,
        repeat: false,
    })
}

fn is_clock(text: &str) -> bool {
    text.len() == 5 && text.as_bytes()[2] == b':' && text.replace(':', "").parse::<u32>().is_ok()
}

// Grid ------------------------------------------------------------------------

#[test]
fn empty_folder_shows_empty_state() {
    let mut screen = Harness::new();
    screen.reload();
    assert!(screen.has("No screenshots yet"));
    assert!(screen.has(EMPTY_BODY));
}

#[test]
fn grid_groups_by_day_newest_first() {
    let mut screen = Harness::new();
    screen.picture("LIVE", "a.jpg", local_unix(3, 12, 0));
    screen.picture("PTU", "b.jpg", local_unix(1, 12, 0));
    screen.picture("LIVE", "c.jpg", local_unix(0, 11, 0));
    screen.picture("LIVE", "d.jpg", local_unix(0, 12, 30));
    screen.reload();
    let older = local_time(3, 12, 0).format("%A %-d %B %Y").to_string();
    let texts = screen.texts();
    let headings: Vec<&str> = texts
        .iter()
        .map(String::as_str)
        .filter(|text| ["Today", "Yesterday", older.as_str()].contains(text))
        .collect();
    assert_eq!(headings, ["Today", "Yesterday", older.as_str()]);
    let clocks: Vec<&str> = texts
        .iter()
        .map(String::as_str)
        .filter(|t| is_clock(t))
        .collect();
    assert_eq!(clocks, ["12:30", "11:00", "12:00", "12:00"]);
    assert!(screen.has("2 screenshots"));
}

#[test]
fn undecodable_file_gets_a_generic_tile_and_is_listed() {
    let mut screen = Harness::new();
    let now = local_unix(0, 10, 0);
    screen.file("LIVE", "broken.jpg", b"not a picture", now);
    screen.file("LIVE", "notes.txt", b"route to Crusader", now - 60);
    screen.picture("LIVE", "fine.jpg", now - 120);
    screen.reload();
    assert!(screen.has("broken.jpg"));
    assert!(screen.has("notes.txt"));
    assert!(!screen.has("fine.jpg"), "a decoded picture shows no name");
    assert!(screen.has_part("3 screenshots, "));
}

#[test]
fn channel_filter_and_favourites_filter_narrow_the_grid() {
    let mut screen = Harness::new();
    let first = screen.picture("LIVE", "one.jpg", local_unix(0, 12, 0));
    screen.picture("LIVE", "two.jpg", local_unix(0, 11, 0));
    screen.picture("PTU", "three.jpg", local_unix(0, 10, 0));
    screen.picture("Archive", "four.jpg", local_unix(0, 9, 0));
    screen.reload();
    assert!(screen.has_part("4 screenshots, "));
    screen.send(Message::Source(Source::Channel("PTU".into())));
    assert!(screen.has_part("1 screenshot, "));
    screen.send(Message::Source(Source::Archive));
    assert!(screen.has_part("1 screenshot, "));
    assert!(screen.has("Archive"));
    screen.send(Message::Source(Source::All));
    screen.send(Message::Open(first));
    screen.click("Favourite");
    screen.send(Message::Back);
    assert_eq!(screen.click("Favourites"), 1);
    assert!(screen.has_part("1 screenshot, "));
    assert!(screen.has("12:00"));
    assert!(!screen.has("11:00"));
}

#[test]
fn count_line_shows_number_and_total_size() {
    let mut screen = Harness::new();
    for (index, name) in ["a.txt", "b.txt", "c.txt"].into_iter().enumerate() {
        let path = screen.file("LIVE", name, b"", local_unix(0, 10, index as u32));
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(1024 * 1024)
            .unwrap();
    }
    screen.reload();
    assert!(screen.has("3 screenshots, 3.0 MB"));
}

#[test]
fn ticking_two_tiles_shows_the_selection_bar() {
    let mut screen = Harness::new();
    let a = screen.picture("LIVE", "a.jpg", local_unix(0, 12, 0));
    let b = screen.picture("PTU", "b.jpg", local_unix(0, 11, 0));
    screen.reload();
    assert!(screen.has("Favourites"));
    screen.send(Message::Tick(a, true));
    screen.send(Message::Tick(b, true));
    assert!(screen.has("2 selected"));
    for label in ["Favourite", "Archive", "Delete", "Clear selection"] {
        assert!(screen.has(label), "{label}");
    }
    assert!(
        !screen.has("Favourites"),
        "the action bar replaces the filters"
    );
    assert!(!screen.has_part("share"));
    screen.click("Clear selection");
    assert!(screen.has("Favourites"));
}

// Viewer ----------------------------------------------------------------------

#[test]
fn clicking_a_tile_opens_the_viewer_with_its_details() {
    let mut screen = Harness::new();
    let path = screen.picture("LIVE", "hangar.jpg", local_unix(0, 12, 0));
    screen.reload();
    assert_eq!(screen.click("12:00"), 1);
    assert!(screen.state.wants_input());
    assert!(screen.has("hangar.jpg"));
    assert!(screen.has("Back to all screenshots"));
    assert!(screen.has("320 x 180"));
    let size = fs::metadata(&path).unwrap().len();
    let kilobytes = size as f64 / 1024.0;
    let expected = if kilobytes < 100.0 {
        format!("{kilobytes:.1} KB")
    } else {
        format!("{kilobytes:.0} KB")
    };
    assert!(screen.has(&expected), "{expected}");
    assert!(screen.has("LIVE"));
    assert!(screen.has_part("hangar.jpg"));
    screen.click("Back to all screenshots");
    assert!(!screen.state.wants_input());
    assert!(screen.has("Refresh"));
}

#[test]
fn next_and_previous_step_through_the_filtered_list() {
    let mut screen = Harness::new();
    let newest = screen.picture("LIVE", "first.jpg", local_unix(0, 12, 0));
    screen.picture("PTU", "skipped.jpg", local_unix(0, 11, 30));
    screen.picture("LIVE", "second.jpg", local_unix(0, 11, 0));
    screen.picture("LIVE", "third.jpg", local_unix(0, 10, 0));
    screen.reload();
    screen.send(Message::Source(Source::Channel("LIVE".into())));
    screen.send(Message::Open(newest));
    assert!(screen.has("1 of 3"));
    screen.send(Message::Next);
    assert!(screen.has("second.jpg"));
    let right = key_press(key::Named::ArrowRight, key::Code::ArrowRight);
    screen.send(Message::Input(right.clone()));
    assert!(screen.has("third.jpg"));
    assert!(screen.has("3 of 3"));
    screen.send(Message::Input(right));
    assert!(screen.has("third.jpg"), "stepping past the end stays put");
    screen.send(Message::Input(key_press(
        key::Named::ArrowLeft,
        key::Code::ArrowLeft,
    )));
    assert!(screen.has("second.jpg"));
    screen.send(Message::Previous);
    assert!(screen.has("first.jpg"));
    screen.send(Message::Input(key_press(
        key::Named::Escape,
        key::Code::Escape,
    )));
    assert!(!screen.state.wants_input());
    assert!(screen.has("Refresh"));
}

#[test]
fn viewer_keys_reach_the_screen_through_the_app() {
    let temp = tempfile::tempdir().unwrap();
    let ctx = testkit::context_with_channels(temp.path(), &["LIVE"]);
    let dir = ctx.channels[0].screenshots_dir();
    let newest = dir.join("newest.jpg");
    for (name, minutes) in [("newest.jpg", 0), ("older.jpg", 5)] {
        let path = dir.join(name);
        testkit::space_scene(&path, 64, 36, minutes);
        set_time(&path, local_unix(0, 12, 0) - minutes as i64 * 60);
    }
    let mut app = App::with(ctx);
    testkit::feed(
        &mut app,
        [
            app::Message::Navigate(Screen::Screenshots),
            app::Message::Screenshots(Message::Open(newest)),
            app::Message::Input(key_press(key::Named::ArrowRight, key::Code::ArrowRight)),
        ],
    );
    let mut ui = Simulator::with_size(sc_app::settings(), FULL, app.view());
    assert!(ui.find("older.jpg").is_ok());
}

// Share -----------------------------------------------------------------------

#[test]
fn copy_image_sends_rgba_to_the_clipboard() {
    let mut screen = Harness::new();
    let path = screen.picture("LIVE", "a.jpg", local_unix(0, 12, 0));
    screen.reload();
    screen.send(Message::Open(path));
    assert_eq!(screen.click("Copy image"), 1);
    assert_eq!(
        screen.clips(),
        [Clip::Image {
            width: 320,
            height: 180,
            bytes: 320 * 180 * 4
        }]
    );
    let first = screen.last_toast().clone();
    assert_eq!(first.kind, ToastKind::Success);
    if cfg!(target_os = "linux") {
        assert_eq!(first.text, format!("{IMAGE_COPIED} {STAYS_ON_CLIPBOARD}"));
    }
    screen.click("Copy image");
    assert_eq!(screen.last_toast().text, IMAGE_COPIED);
    assert_eq!(screen.clips().len(), 2);
}

#[test]
fn copy_smaller_copy_respects_the_limits() {
    let mut screen = Harness::new();
    screen.ctx.settings.screenshots.share_max_edge = 1280;
    let path = screen.folder("LIVE").join("wide.jpg");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_fn(4000, 2250, |x, y| {
        image::Rgb([(x / 16) as u8, (y / 9) as u8, 60])
    })
    .save(&path)
    .unwrap();
    set_time(&path, local_unix(0, 12, 0));
    screen.reload();
    screen.send(Message::Open(path));
    assert_eq!(screen.click("Copy smaller copy"), 1);
    let clips = screen.clips();
    let [
        Clip::Image {
            width,
            height,
            bytes,
        },
    ] = clips.as_slice()
    else {
        panic!("{clips:?}");
    };
    assert_eq!((*width, *height), (1280, 720));
    assert_eq!(*bytes, 1280 * 720 * 4);
    let toast = &screen.last_toast().text;
    assert!(toast.starts_with("Copied at 1280 x 720, "), "{toast}");
}

#[test]
fn copy_path_sends_the_path_as_text() {
    let mut screen = Harness::new();
    let path = screen.picture("PTU", "a.jpg", local_unix(0, 12, 0));
    screen.reload();
    screen.send(Message::Open(path.clone()));
    screen.click("Copy path");
    assert_eq!(screen.clips(), [Clip::Text(path.display().to_string())]);
    assert_eq!(screen.last_toast().text, "Path copied");
}

#[test]
fn missing_clipboard_shows_the_plain_message() {
    let mut screen = Harness::with_clipboard(|_| ClipboardCell::missing());
    let path = screen.picture("LIVE", "a.jpg", local_unix(0, 12, 0));
    screen.reload();
    screen.send(Message::Open(path));
    screen.click("Copy image");
    assert_eq!(screen.last_toast().kind, ToastKind::Error);
    assert_eq!(screen.last_toast().text, NO_CLIPBOARD);
    screen.click("Copy path");
    assert_eq!(screen.last_toast().text, NO_CLIPBOARD);
    assert!(screen.has("Copy image"), "the button is usable again");
}

#[test]
fn save_smaller_copy_writes_to_the_shared_folder_and_reveals_it() {
    let mut screen = Harness::new();
    let path = screen.picture("LIVE", "a.jpg", local_unix(0, 12, 0));
    screen.reload();
    screen.send(Message::Open(path));
    screen.click("Save smaller copy");
    let saved = screen.archive().join("shared/a-share.jpg");
    assert!(saved.exists());
    assert_eq!(
        *screen.revealed.lock().unwrap(),
        [screen.archive().join("shared")]
    );
    assert!(
        screen
            .last_toast()
            .text
            .starts_with("Saved a smaller copy to ~")
    );
    screen.click("Show in folder");
    assert_eq!(
        screen.revealed.lock().unwrap().last(),
        Some(&screen.folder("LIVE"))
    );
}

// Manage ----------------------------------------------------------------------

#[test]
fn favourite_toggles_and_survives_a_rescan() {
    let mut screen = Harness::new();
    let path = screen.picture("LIVE", "a.jpg", local_unix(0, 12, 0));
    screen.picture("LIVE", "b.jpg", local_unix(0, 11, 0));
    screen.reload();
    screen.send(Message::Open(path.clone()));
    screen.click("Favourite");
    assert!(screen.has("Remove favourite"));
    screen.click("Remove favourite");
    assert!(screen.has("Favourite"));
    screen.click("Favourite");
    screen.send(Message::Back);
    screen.reload();
    screen.send(Message::Favourites(true));
    assert!(screen.has_part("1 screenshot, "));

    let mut fresh = Harness::new();
    fresh.temp = screen.temp;
    fresh.ctx = screen.ctx.clone();
    fresh.reload();
    fresh.send(Message::Favourites(true));
    assert!(
        fresh.has_part("1 screenshot, "),
        "favourites are read back from disk"
    );
    fresh.send(Message::Open(path));
    assert!(fresh.has("Remove favourite"));
}

#[test]
fn rename_changes_the_file_and_refuses_an_existing_name() {
    let mut screen = Harness::new();
    let path = screen.picture("LIVE", "a.jpg", local_unix(0, 12, 0));
    let taken = screen.picture("LIVE", "taken.jpg", local_unix(0, 11, 0));
    screen.reload();
    screen.send(Message::Open(path.clone()));
    screen.click("Rename");
    assert!(screen.has(".jpg"));
    screen.send(Message::RenameInput("taken".into()));
    screen.send(Message::RenameSave);
    assert!(screen.has("A file with that name already exists."));
    assert!(path.exists() && taken.exists());
    screen.send(Message::RenameInput("Hangar at dawn".into()));
    screen.send(Message::RenameSave);
    let renamed = screen.folder("LIVE").join("Hangar at dawn.jpg");
    assert!(renamed.exists());
    assert!(!path.exists());
    assert!(screen.has("Hangar at dawn.jpg"));
    assert!(screen.has("Rename"), "the editor closes");
    screen.send(Message::Back);
    assert!(screen.has_part("2 screenshots, "));
}

#[test]
fn archive_moves_or_copies_per_setting() {
    let mut screen = Harness::new();
    let moved = screen.picture("LIVE", "moved.jpg", local_unix(0, 12, 0));
    let copied = screen.picture("PTU", "copied.jpg", local_unix(0, 11, 0));
    screen.reload();
    assert_eq!(
        screen.ctx.settings.screenshots.archive_mode,
        ArchiveMode::Move
    );
    screen.send(Message::Open(moved.clone()));
    screen.click("Archive");
    assert!(!moved.exists());
    assert!(screen.archive().join("moved.jpg").exists());
    assert_eq!(screen.last_toast().text, "Moved to the archive");
    assert!(
        screen.has("Archive, from LIVE"),
        "the viewer follows the file"
    );

    screen.ctx.settings.screenshots.archive_mode = ArchiveMode::Copy;
    screen.send(Message::Open(copied.clone()));
    screen.click("Archive");
    assert!(copied.exists());
    assert!(screen.archive().join("copied.jpg").exists());
    assert_eq!(screen.last_toast().text, "Copied to the archive");
    screen.send(Message::Back);
    assert!(screen.has_part("3 screenshots, "));
}

#[test]
fn archive_without_a_folder_points_at_settings() {
    let mut screen = Harness::new();
    let path = screen.picture("LIVE", "a.jpg", local_unix(0, 12, 0));
    screen.ctx.settings.screenshots.archive_dir = None;
    screen.reload();
    screen.send(Message::Open(path.clone()));
    screen.click("Archive");
    assert_eq!(screen.last_toast().text, NO_ARCHIVE);
    assert!(path.exists());
}

#[test]
fn delete_one_goes_to_trash_without_asking() {
    let mut screen = Harness::new();
    let first = screen.picture("LIVE", "first.jpg", local_unix(0, 12, 0));
    let second = screen.picture("LIVE", "second.jpg", local_unix(0, 11, 0));
    screen.reload();
    screen.send(Message::Open(first.clone()));
    screen.click("Delete");
    assert!(screen.seen.confirm.is_none());
    assert_eq!(
        screen.trashed.lock().unwrap().as_slice(),
        std::slice::from_ref(&first)
    );
    assert!(!first.exists());
    assert_eq!(screen.last_toast().text, MOVED_TO_TRASH);
    assert!(
        screen.has("second.jpg"),
        "the viewer moves on to the next one"
    );

    screen.send(Message::Back);
    screen.send(Message::Tick(second.clone(), true));
    screen.click("Delete");
    assert!(screen.seen.confirm.is_none());
    assert_eq!(screen.trashed.lock().unwrap().len(), 2);
    assert!(screen.has("No screenshots yet"));
}

#[test]
fn delete_many_asks_first() {
    let mut screen = Harness::new();
    let paths: Vec<_> = (0..3)
        .map(|i| screen.picture("LIVE", &format!("{i}.jpg"), local_unix(0, 12, i)))
        .collect();
    screen.reload();
    for path in &paths {
        screen.send(Message::Tick(path.clone(), true));
    }
    screen.click("Delete");
    let confirm = screen.seen.confirm.as_ref().expect("a confirm");
    assert_eq!(confirm.title, "Move 3 screenshots to the trash?");
    assert!(screen.trashed.lock().unwrap().is_empty());
    assert!(paths.iter().all(|path| path.exists()));
    screen.accept();
    assert_eq!(screen.trashed.lock().unwrap().len(), 3);
    assert_eq!(screen.last_toast().text, "Moved 3 screenshots to the trash");
    assert!(screen.has("No screenshots yet"));
}

// Scale and locks -------------------------------------------------------------

#[test]
fn two_thousand_files_build_only_visible_rows() {
    let built = Arc::new(Mutex::new(0usize));
    let counter = built.clone();
    let days = [700usize, 800, 500];
    let list: Element<'_, ()> = grid::virtual_grid(
        &days,
        0.0,
        |_| (),
        |_| text("day").into(),
        move |position, _| {
            *counter.lock().unwrap() += 1;
            text(position.to_string()).into()
        },
    );
    let _ui = Simulator::with_size(sc_app::settings(), Size::new(1000.0, 700.0), list);
    let built = *built.lock().unwrap();
    assert!(built > 0);
    assert!(built <= 40, "built {built} tiles");

    let mut screen = Harness::new();
    let start = local_unix(0, 12, 0);
    for index in 0..2000 {
        screen.file("LIVE", &format!("{index:04}.jpg"), b"", start - index);
    }
    screen.reload();
    let texts = screen.texts();
    assert!(
        texts.iter().any(|t| t == "2,000 screenshots, 0 bytes"),
        "{texts:?}"
    );
    let tiles = texts.iter().filter(|t| is_clock(t)).count();
    assert!(tiles > 0 && tiles <= 60, "built {tiles} tiles");
}

#[test]
fn read_only_does_not_disable_anything() {
    let mut screen = Harness::new();
    screen.ctx = screen.ctx.clone().with_probe(Arc::new(FixedProbe {
        game: RunState::Running,
        launcher: RunState::NotRunning,
    }));
    screen.ctx.game = RunState::Running;
    assert!(screen.ctx.read_only());
    let path = screen.picture("LIVE", "a.jpg", local_unix(0, 12, 0));
    screen.reload();
    screen.send(Message::Tick(path.clone(), true));
    for label in ["Favourite", "Archive", "Delete", "Clear selection"] {
        assert_eq!(screen.press(label).len(), 1, "{label}");
    }
    screen.send(Message::ClearSelection);
    screen.send(Message::Open(path));
    for label in [
        "Copy image",
        "Copy smaller copy",
        "Save smaller copy",
        "Copy path",
        "Show in folder",
        "Favourite",
        "Rename",
        "Archive",
        "Delete",
    ] {
        assert_eq!(screen.press(label).len(), 1, "{label}");
    }
    assert_eq!(screen.state.pending(), 0);
}

// Pictures --------------------------------------------------------------------

struct Demo {
    channel: &'static str,
    days_ago: i64,
    hour: u32,
    minute: u32,
}

const fn demo(channel: &'static str, days_ago: i64, hour: u32, minute: u32) -> Demo {
    Demo {
        channel,
        days_ago,
        hour,
        minute,
    }
}

const DEMO: [Demo; 14] = [
    demo("LIVE", 0, 21, 42),
    demo("LIVE", 0, 21, 38),
    demo("PTU", 0, 21, 17),
    demo("LIVE", 0, 20, 56),
    demo("LIVE", 0, 20, 51),
    demo("PTU", 0, 20, 3),
    demo("LIVE", 1, 23, 10),
    demo("LIVE", 1, 22, 47),
    demo("Archive", 1, 22, 45),
    demo("PTU", 1, 22, 31),
    demo("LIVE", 1, 19, 2),
    demo("Archive", 1, 18, 58),
    demo("LIVE", 1, 18, 40),
    demo("LIVE", 1, 18, 12),
];

/// Fourteen captures over two days in LIVE, PTU and the archive. Returns the newest.
fn demo_files(ctx: &Context) -> PathBuf {
    let archive = ctx.settings.screenshots.archive_dir.clone().unwrap();
    let mut paths = Vec::new();
    for (index, shot) in DEMO.iter().enumerate() {
        let dir = match shot.channel {
            "Archive" => archive.clone(),
            name => ctx
                .channels
                .iter()
                .find(|c| c.name == name)
                .unwrap()
                .screenshots_dir(),
        };
        let path = dir.join(shot_name(shot.days_ago, shot.hour, shot.minute));
        let (width, height) = if index == 0 {
            (1920, 1080)
        } else {
            (1280, 720)
        };
        testkit::space_scene(&path, width, height, 11 + index as u64);
        set_time(&path, local_unix(shot.days_ago, shot.hour, shot.minute));
        paths.push(path);
    }
    paths.swap_remove(0)
}

fn demo_app(temp: &Path, choice: ThemeChoice) -> App {
    let mut ctx = testkit::context_with_channels(temp, &["LIVE", "PTU"]);
    ctx.settings.theme = choice;
    ctx.settings.screenshots.archive_dir = Some(testkit::home(temp).join("Pictures/Star Citizen"));
    App::with(ctx)
}

fn shot(app: &App, name: &str) {
    let mut ui = Simulator::with_size(sc_app::settings(), FULL, app.view());
    testkit::snapshot(&mut ui, &app.theme(), name);
}

#[test]
fn screenshots_pictures() {
    let temp = tempfile::tempdir().unwrap();
    let mut newest = None;
    for (choice, name) in [(ThemeChoice::Dark, "dark"), (ThemeChoice::Light, "light")] {
        let mut app = demo_app(temp.path(), choice);
        let favourite = newest
            .get_or_insert_with(|| demo_files(app.context()))
            .clone();
        let open = app::Message::Screenshots(Message::Open(favourite.clone()));
        testkit::feed(&mut app, [app::Message::Navigate(Screen::Screenshots)]);
        if choice == ThemeChoice::Dark {
            testkit::feed(
                &mut app,
                [
                    open.clone(),
                    app::Message::Screenshots(Message::ToggleFavourite),
                    app::Message::Screenshots(Message::Back),
                ],
            );
        }
        shot(&app, &format!("screenshots-{name}"));
        if choice == ThemeChoice::Dark {
            testkit::feed(&mut app, [open]);
            shot(&app, "screenshots-viewer-dark");
        }
    }
}
