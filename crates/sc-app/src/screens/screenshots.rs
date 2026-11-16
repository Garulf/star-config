pub mod grid;
mod icons;
pub mod library;
pub mod share;
mod tiles;
mod viewer;

pub use grid::Window;
pub use library::{Filter, Period, Source};
pub use share::{Clipboard, ClipboardCell, Copied, CopyError, FileAction, NO_CLIPBOARD, Services};

use super::backups::when;
use super::units;
use super::words::words;
use crate::context::Context;
use crate::screen::{Action, Confirm, Toast};
use crate::theme::ButtonKind;
use crate::work;
use grid::{GRID_ID, Plan};
use iced::keyboard::{self, Key, key};
use iced::widget::image::Handle;
use iced::widget::{operation, scrollable};
use iced::{Element, Task};
use library::{Day, file_name, file_stem, screenshots as count_of, sentence, taken};
use sc_core::screenshots::{self, Index, Shot, ShotKind};
use sc_core::settings::ArchiveMode;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const IMAGE_COPIED: &str = "Image copied. Paste it into Discord or a browser.";
pub const STAYS_ON_CLIPBOARD: &str = "It stays on the clipboard while star-config is open.";
pub const NO_ARCHIVE: &str =
    "There is no archive folder, and no pictures folder to put one in. Choose one in Settings.";
pub const MOVED_TO_TRASH: &str = "Moved to the trash";

const INDEX_FILE: &str = "screenshot-index.json";
const RENAME_ID: &str = "screenshot-rename";
const FIRST_BATCH: usize = 48;
const THUMB_CHUNK: usize = 4;
const THUMB_EDGE: u32 = 480;

/// What a rescan found. The index is read from disk only the first time.
#[derive(Debug, Clone)]
pub struct Scan {
    install: PathBuf,
    shots: Vec<Shot>,
    index: Option<Index>,
}

#[derive(Debug, Clone)]
pub struct Picture {
    width: u32,
    height: u32,
    handle: Handle,
}

#[derive(Debug, Clone)]
pub struct Moved {
    from: PathBuf,
    to: PathBuf,
    channel: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Archived {
    mode: ArchiveMode,
    moved: Vec<Moved>,
    asked: usize,
    failure: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Trashed {
    done: Vec<PathBuf>,
    asked: usize,
    failure: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Scanned(Box<Scan>),
    Refresh,
    Thumbnails(Vec<(PathBuf, Option<Handle>)>),
    Scrolled(Window),
    Source(Source),
    Favourites(bool),
    Period(Period),
    ShowAll,
    Tick(PathBuf, bool),
    ClearSelection,
    FavouriteSelected,
    ArchiveSelected,
    DeleteSelected,
    DeleteConfirmed(Vec<PathBuf>),
    Open(PathBuf),
    Loaded(PathBuf, Result<Picture, String>),
    Back,
    Previous,
    Next,
    CopyImage,
    CopySmaller,
    SaveSmaller,
    CopyPath,
    ShowInFolder,
    Copied(Result<Copied, CopyError>),
    Saved(Result<PathBuf, String>),
    Revealed(Result<(), String>),
    ToggleFavourite,
    Rename,
    RenameInput(String),
    RenameSave,
    RenameCancel,
    Renamed(PathBuf, Result<PathBuf, String>),
    Archive,
    Archived(Box<Archived>),
    Delete,
    Trashed(Box<Trashed>),
    IndexSaved(Result<(), String>),
    Input(iced::Event),
}

enum Thumb {
    Pending,
    Ready(Handle),
    Unreadable,
}

enum Stage {
    Loading,
    Ready(Picture),
    Unreadable,
}

struct Rename {
    stem: String,
    error: Option<String>,
}

struct Viewer {
    path: PathBuf,
    stage: Stage,
    rename: Option<Rename>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Busy {
    CopyImage,
    CopySmaller,
    SaveSmaller,
    CopyPath,
    Renaming,
    Archiving,
    Deleting,
}

pub struct State {
    services: Services,
    shots: Vec<Shot>,
    index: Option<Index>,
    loaded: bool,
    scanning: bool,
    filter: Filter,
    shown: Vec<usize>,
    days: Vec<Day>,
    day_sizes: Vec<usize>,
    shown_bytes: u64,
    thumbs: HashMap<PathBuf, Thumb>,
    window: Window,
    selected: BTreeSet<PathBuf>,
    viewer: Option<Viewer>,
    busy: Option<Busy>,
    told_clipboard_lifetime: bool,
}

impl State {
    pub fn new() -> Self {
        Self::with_services(Services::system())
    }

    pub fn with_services(services: Services) -> Self {
        Self {
            services,
            shots: Vec::new(),
            index: None,
            loaded: false,
            scanning: false,
            filter: Filter::default(),
            shown: Vec::new(),
            days: Vec::new(),
            day_sizes: Vec::new(),
            shown_bytes: 0,
            thumbs: HashMap::new(),
            window: Window::default(),
            selected: BTreeSet::new(),
            viewer: None,
            busy: None,
            told_clipboard_lifetime: false,
        }
    }

    pub fn reload(&mut self, ctx: &Context) -> Task<Message> {
        let Some(install) = ctx.install.as_ref().map(|install| install.root.clone()) else {
            return Task::none();
        };
        self.scanning = true;
        let channels = ctx.channels.clone();
        let archive = self.archive_dir(ctx);
        let index = self.index.is_none().then(|| index_path(ctx));
        work::background(
            move || Scan {
                install,
                shots: screenshots::scan(&channels, archive.as_deref()),
                index: index.map(|path| Index::load(&path)),
            },
            |scan| Message::Scanned(Box::new(scan)),
        )
    }

    /// True while the viewer is open: it steps with the arrow keys and closes with Escape.
    pub fn wants_input(&self) -> bool {
        self.viewer.is_some()
    }

    pub fn update(&mut self, message: Message, ctx: &Context) -> Action<Message> {
        match message {
            Message::Scanned(scan) => {
                if ctx.install.as_ref().map(|install| &install.root) != Some(&scan.install) {
                    return Action::none();
                }
                self.take(*scan);
                Action::task(self.request_thumbs(ctx))
            }
            Message::Refresh => Action::task(self.reload(ctx)),
            Message::Thumbnails(done) => {
                for (path, handle) in done {
                    self.thumbs
                        .insert(path, handle.map_or(Thumb::Unreadable, Thumb::Ready));
                }
                Action::none()
            }
            Message::Scrolled(window) => {
                self.window = window;
                Action::task(self.request_thumbs(ctx))
            }
            Message::Source(source) => self.set_filter(
                ctx,
                Filter {
                    source,
                    ..self.filter.clone()
                },
            ),
            Message::Favourites(favourites) => self.set_filter(
                ctx,
                Filter {
                    favourites,
                    ..self.filter.clone()
                },
            ),
            Message::Period(period) => self.set_filter(
                ctx,
                Filter {
                    period,
                    ..self.filter.clone()
                },
            ),
            Message::ShowAll => self.set_filter(ctx, Filter::default()),
            Message::Tick(path, on) => {
                if on {
                    self.selected.insert(path);
                } else {
                    self.selected.remove(&path);
                }
                Action::none()
            }
            Message::ClearSelection => {
                self.selected.clear();
                Action::none()
            }
            Message::FavouriteSelected => {
                let paths: Vec<PathBuf> = self.selected.iter().cloned().collect();
                let on = !self.all_favourites(&paths);
                self.set_favourites(ctx, paths, on)
            }
            Message::ArchiveSelected => {
                let paths = self.archivable(self.selected.iter());
                self.archive(ctx, paths)
            }
            Message::DeleteSelected => self.ask_delete(),
            Message::DeleteConfirmed(paths) => self.delete(paths),
            Message::Open(path) => self.open(path),
            Message::Loaded(path, result) => {
                if let Some(viewer) = self.viewer.as_mut().filter(|v| v.path == path) {
                    viewer.stage = result.map_or(Stage::Unreadable, Stage::Ready);
                }
                Action::none()
            }
            Message::Back => self.back(ctx),
            Message::Previous => self.step(-1),
            Message::Next => self.step(1),
            Message::CopyImage => self.copy_image(),
            Message::CopySmaller => self.copy_smaller(ctx),
            Message::SaveSmaller => self.save_smaller(ctx),
            Message::CopyPath => self.copy_path(),
            Message::ShowInFolder => self.show_in_folder(),
            Message::Copied(result) => {
                self.busy = None;
                Action::toast(self.copied_toast(result))
            }
            Message::Saved(result) => {
                self.busy = None;
                Action::toast(match result {
                    Ok(path) => Toast::success(format!(
                        "Saved a smaller copy to {}",
                        ctx.display_path(&path)
                    )),
                    Err(err) => {
                        Toast::error(format!("Could not save a smaller copy. {}", sentence(&err)))
                    }
                })
            }
            Message::Revealed(result) => match result {
                Ok(()) => Action::none(),
                Err(err) => Action::toast(Toast::error(format!(
                    "Could not open the folder. {}",
                    sentence(&err)
                ))),
            },
            Message::ToggleFavourite => match self.current() {
                Some(path) => {
                    let on = !self.is_favourite(&path);
                    self.set_favourites(ctx, vec![path], on)
                }
                None => Action::none(),
            },
            Message::Rename => self.start_rename(),
            Message::RenameInput(stem) => {
                if let Some(rename) = self.viewer.as_mut().and_then(|v| v.rename.as_mut()) {
                    rename.stem = stem;
                    rename.error = None;
                }
                Action::none()
            }
            Message::RenameSave => self.rename(),
            Message::RenameCancel => {
                if let Some(viewer) = self.viewer.as_mut() {
                    viewer.rename = None;
                }
                Action::none()
            }
            Message::Renamed(from, result) => self.renamed(ctx, from, result),
            Message::Archive => {
                let paths = self.archivable(self.current().iter());
                self.archive(ctx, paths)
            }
            Message::Archived(outcome) => self.archived(ctx, *outcome),
            Message::Delete => match self.current() {
                Some(path) => self.delete(vec![path]),
                None => Action::none(),
            },
            Message::Trashed(outcome) => self.trashed(ctx, *outcome),
            Message::IndexSaved(result) => match result {
                Ok(()) => Action::none(),
                Err(err) => Action::toast(Toast::error(format!(
                    "Could not save your favourites. {}",
                    sentence(&err)
                ))),
            },
            Message::Input(event) => self.input(ctx, event),
        }
    }

    pub fn view<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        match &self.viewer {
            Some(viewer) => self.viewer_page(ctx, viewer),
            None => self.grid_page(ctx),
        }
    }

    pub fn pending(&self) -> usize {
        0
    }

    pub fn apply(&mut self, _ctx: &Context) -> Action<Message> {
        Action::none()
    }

    pub fn discard(&mut self) {}

    pub fn undo(&mut self, _ctx: &Context) -> Action<Message> {
        Action::none()
    }

    // Scanning and the shown list

    fn take(&mut self, scan: Scan) {
        self.shots = scan.shots;
        if self.index.is_none() {
            self.index = scan.index;
        }
        self.loaded = true;
        self.scanning = false;
        let present: HashSet<&PathBuf> = self.shots.iter().map(|shot| &shot.path).collect();
        self.selected.retain(|path| present.contains(path));
        if self
            .viewer
            .as_ref()
            .is_some_and(|viewer| !present.contains(&viewer.path))
        {
            self.viewer = None;
        }
        self.refresh();
    }

    fn refresh(&mut self) {
        let today = when::now().date();
        let mut shown = Vec::new();
        let mut dates = Vec::new();
        let mut bytes = 0;
        for (position, shot) in self.shots.iter().enumerate() {
            let date = taken(shot).date();
            if self
                .filter
                .keeps(shot, self.is_favourite(&shot.path), date, today)
            {
                shown.push(position);
                dates.push(date);
                bytes += shot.size;
            }
        }
        self.days = library::days(&dates, today);
        self.day_sizes = self.days.iter().map(|day| day.len).collect();
        self.shown = shown;
        self.shown_bytes = bytes;
    }

    fn set_filter(&mut self, ctx: &Context, filter: Filter) -> Action<Message> {
        self.filter = filter;
        self.refresh();
        self.window.offset = 0.0;
        Action::task(Task::batch([
            operation::snap_to(GRID_ID, scrollable::RelativeOffset::START),
            self.request_thumbs(ctx),
        ]))
    }

    fn request_thumbs(&mut self, ctx: &Context) -> Task<Message> {
        if self.viewer.is_some() {
            return Task::none();
        }
        let range = if self.window.width > 0.0 {
            let plan = Plan::new(&self.day_sizes, self.window.width);
            plan.tiles_in(plan.visible(self.window.offset, self.window.height))
        } else {
            0..FIRST_BATCH
        };
        let end = range.end.min(self.shown.len());
        let start = range.start.min(end);
        let mut wanted = Vec::new();
        for &position in &self.shown[start..end] {
            let shot = &self.shots[position];
            if self.thumbs.contains_key(&shot.path) {
                continue;
            }
            if shot.kind == ShotKind::Image {
                self.thumbs.insert(shot.path.clone(), Thumb::Pending);
                wanted.push(shot.path.clone());
            } else {
                self.thumbs.insert(shot.path.clone(), Thumb::Unreadable);
            }
        }
        let cache = ctx.dirs.thumbnails();
        Task::batch(wanted.chunks(THUMB_CHUNK).map(|chunk| {
            let chunk = chunk.to_vec();
            let cache = cache.clone();
            work::background(move || make_thumbs(chunk, &cache), Message::Thumbnails)
        }))
    }

    fn shot(&self, path: &Path) -> Option<&Shot> {
        self.shots.iter().find(|shot| shot.path == path)
    }

    fn is_favourite(&self, path: &Path) -> bool {
        self.index
            .as_ref()
            .is_some_and(|index| index.favourites.contains(path))
    }

    fn all_favourites(&self, paths: &[PathBuf]) -> bool {
        !paths.is_empty() && paths.iter().all(|path| self.is_favourite(path))
    }

    fn archive_dir(&self, ctx: &Context) -> Option<PathBuf> {
        ctx.settings
            .screenshots
            .archive_dir
            .clone()
            .or_else(|| self.services.default_archive.clone())
    }

    fn archivable<'p>(&self, paths: impl Iterator<Item = &'p PathBuf>) -> Vec<PathBuf> {
        paths
            .filter(|path| self.shot(path).is_some_and(|shot| shot.channel.is_some()))
            .cloned()
            .collect()
    }

    // Viewer

    fn current(&self) -> Option<PathBuf> {
        self.viewer.as_ref().map(|viewer| viewer.path.clone())
    }

    fn position(&self, path: &Path) -> Option<usize> {
        self.shown
            .iter()
            .position(|&index| self.shots[index].path == path)
    }

    fn neighbour(&self, path: &Path, delta: isize) -> Option<PathBuf> {
        let next = self.position(path)?.checked_add_signed(delta)?;
        self.shown
            .get(next)
            .map(|&index| self.shots[index].path.clone())
    }

    fn open(&mut self, path: PathBuf) -> Action<Message> {
        let readable = self
            .shot(&path)
            .is_some_and(|shot| shot.kind == ShotKind::Image);
        self.viewer = Some(Viewer {
            path: path.clone(),
            stage: if readable {
                Stage::Loading
            } else {
                Stage::Unreadable
            },
            rename: None,
        });
        if !readable {
            return Action::none();
        }
        let key = path.clone();
        Action::task(work::background(
            move || load_picture(&path),
            move |result| Message::Loaded(key, result),
        ))
    }

    fn step(&mut self, delta: isize) -> Action<Message> {
        if self.busy == Some(Busy::Renaming) {
            return Action::none();
        }
        match self.current().and_then(|path| self.neighbour(&path, delta)) {
            Some(next) => self.open(next),
            None => Action::none(),
        }
    }

    fn back(&mut self, ctx: &Context) -> Action<Message> {
        self.viewer = None;
        let restore = operation::scroll_to(
            GRID_ID,
            scrollable::AbsoluteOffset {
                x: 0.0,
                y: self.window.offset,
            },
        );
        Action::task(Task::batch([restore, self.request_thumbs(ctx)]))
    }

    fn input(&mut self, ctx: &Context, event: iced::Event) -> Action<Message> {
        let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event
        else {
            return Action::none();
        };
        if self.viewer.is_none() || !modifiers.is_empty() {
            return Action::none();
        }
        match key.as_ref() {
            Key::Named(key::Named::ArrowLeft) => self.step(-1),
            Key::Named(key::Named::ArrowRight) => self.step(1),
            Key::Named(key::Named::Escape) => {
                match self
                    .viewer
                    .as_mut()
                    .filter(|viewer| viewer.rename.is_some())
                {
                    Some(viewer) => {
                        viewer.rename = None;
                        Action::none()
                    }
                    None => self.back(ctx),
                }
            }
            _ => Action::none(),
        }
    }

    // Share

    fn copy(
        &mut self,
        busy: Busy,
        job: impl FnOnce(&ClipboardCell) -> Result<Copied, CopyError> + Send + 'static,
    ) -> Action<Message> {
        if self.busy.is_some() {
            return Action::none();
        }
        self.busy = Some(busy);
        let clipboard = self.services.clipboard.clone();
        Action::task(work::background(move || job(&clipboard), Message::Copied))
    }

    fn copy_image(&mut self) -> Action<Message> {
        match self.current() {
            Some(path) => self.copy(Busy::CopyImage, move |board| {
                share::copy_image(board, &path)
            }),
            None => Action::none(),
        }
    }

    fn copy_smaller(&mut self, ctx: &Context) -> Action<Message> {
        let limits = &ctx.settings.screenshots;
        let (max_bytes, max_edge) = (limits.share_max_bytes, limits.share_max_edge);
        match self.current() {
            Some(path) => self.copy(Busy::CopySmaller, move |board| {
                share::copy_smaller(board, &path, max_bytes, max_edge)
            }),
            None => Action::none(),
        }
    }

    fn copy_path(&mut self) -> Action<Message> {
        match self.current() {
            Some(path) => self.copy(Busy::CopyPath, move |board| share::copy_path(board, &path)),
            None => Action::none(),
        }
    }

    fn copied_toast(&mut self, result: Result<Copied, CopyError>) -> Toast {
        match result {
            Ok(Copied::Image) => Toast::success(self.image_copied(IMAGE_COPIED.to_string())),
            Ok(Copied::Smaller {
                width,
                height,
                bytes,
            }) => Toast::success(self.image_copied(format!(
                "Copied at {width} x {height}, {}.",
                units::bytes(bytes as u64)
            ))),
            Ok(Copied::Path) => Toast::success("Path copied"),
            Err(CopyError::NoClipboard) => Toast::error(NO_CLIPBOARD),
            Err(CopyError::Failed(err)) => {
                Toast::error(format!("Could not copy the image. {}", sentence(&err)))
            }
        }
    }

    fn image_copied(&mut self, text: String) -> String {
        if cfg!(target_os = "linux") && !self.told_clipboard_lifetime {
            self.told_clipboard_lifetime = true;
            format!("{text} {STAYS_ON_CLIPBOARD}")
        } else {
            text
        }
    }

    fn save_smaller(&mut self, ctx: &Context) -> Action<Message> {
        let Some(path) = self.current() else {
            return Action::none();
        };
        if self.busy.is_some() {
            return Action::none();
        }
        let Some(archive) = self.archive_dir(ctx) else {
            return Action::toast(Toast::error(NO_ARCHIVE));
        };
        self.busy = Some(Busy::SaveSmaller);
        let limits = &ctx.settings.screenshots;
        let (max_bytes, max_edge) = (limits.share_max_bytes, limits.share_max_edge);
        let reveal = self.services.reveal.clone();
        Action::task(work::background(
            move || -> Result<PathBuf, String> {
                let saved =
                    decoded(|| screenshots::save_share_copy(&path, &archive, max_bytes, max_edge))
                        .map_err(|err| words(&err))?;
                if let Some(folder) = saved.parent() {
                    let _ = reveal(folder);
                }
                Ok(saved)
            },
            Message::Saved,
        ))
    }

    fn show_in_folder(&mut self) -> Action<Message> {
        let Some(folder) = self
            .current()
            .and_then(|path| path.parent().map(Path::to_path_buf))
        else {
            return Action::none();
        };
        let reveal = self.services.reveal.clone();
        Action::task(work::background(move || reveal(&folder), Message::Revealed))
    }

    // Manage

    fn set_favourites(&mut self, ctx: &Context, paths: Vec<PathBuf>, on: bool) -> Action<Message> {
        let index = self.index.get_or_insert_with(Index::default);
        for path in paths {
            if on {
                index.favourites.insert(path);
            } else {
                index.favourites.remove(&path);
            }
        }
        self.refresh();
        self.save_index(ctx)
    }

    fn save_index(&self, ctx: &Context) -> Action<Message> {
        let Some(index) = self.index.clone() else {
            return Action::none();
        };
        let path = index_path(ctx);
        Action::task(work::background(
            move || index.save(&path).map_err(|err| words(&err)),
            Message::IndexSaved,
        ))
    }

    fn start_rename(&mut self) -> Action<Message> {
        let Some(viewer) = self.viewer.as_mut() else {
            return Action::none();
        };
        viewer.rename = Some(Rename {
            stem: file_stem(&viewer.path),
            error: None,
        });
        Action::task(operation::focus(RENAME_ID).chain(operation::select_all(RENAME_ID)))
    }

    fn rename(&mut self) -> Action<Message> {
        if self.busy.is_some() {
            return Action::none();
        }
        let Some(viewer) = self.viewer.as_mut() else {
            return Action::none();
        };
        let Some(rename) = viewer.rename.as_ref() else {
            return Action::none();
        };
        let stem = rename.stem.trim().to_string();
        if stem == file_stem(&viewer.path) {
            viewer.rename = None;
            return Action::none();
        }
        self.busy = Some(Busy::Renaming);
        let path = viewer.path.clone();
        let from = path.clone();
        Action::task(work::background(
            move || screenshots::rename(&path, &stem).map_err(|err| sentence(&words(&err))),
            move |result| Message::Renamed(from, result),
        ))
    }

    fn renamed(
        &mut self,
        ctx: &Context,
        from: PathBuf,
        result: Result<PathBuf, String>,
    ) -> Action<Message> {
        self.busy = None;
        let to = match result {
            Ok(to) => to,
            Err(err) => {
                if let Some(rename) = self.viewer.as_mut().and_then(|v| v.rename.as_mut()) {
                    rename.error = Some(err);
                }
                return Action::none();
            }
        };
        if let Some(index) = self.index.as_mut() {
            index.renamed(&from, &to);
        }
        if self.selected.remove(&from) {
            self.selected.insert(to.clone());
        }
        if let Some(thumb) = self.thumbs.remove(&from) {
            self.thumbs.insert(to.clone(), thumb);
        }
        for shot in self.shots.iter_mut().filter(|shot| shot.path == from) {
            shot.path = to.clone();
        }
        if let Some(viewer) = self.viewer.as_mut().filter(|v| v.path == from) {
            viewer.path = to.clone();
            viewer.rename = None;
        }
        self.refresh();
        self.save_index(ctx)
            .with_task(self.reload(ctx))
            .with_toast(Toast::success(format!("Renamed to {}", file_name(&to))))
    }

    fn archive(&mut self, ctx: &Context, paths: Vec<PathBuf>) -> Action<Message> {
        if paths.is_empty() || self.busy.is_some() {
            return Action::none();
        }
        let Some(archive) = self.archive_dir(ctx) else {
            return Action::toast(Toast::error(NO_ARCHIVE));
        };
        let mode = ctx.settings.screenshots.archive_mode;
        let items: Vec<(PathBuf, Option<String>)> = paths
            .into_iter()
            .map(|path| {
                let channel = self.shot(&path).and_then(|shot| shot.channel.clone());
                (path, channel)
            })
            .collect();
        self.busy = Some(Busy::Archiving);
        Action::task(work::background(
            move || archive_all(items, &archive, mode),
            |outcome| Message::Archived(Box::new(outcome)),
        ))
    }

    fn archived(&mut self, ctx: &Context, outcome: Archived) -> Action<Message> {
        self.busy = None;
        let index = self.index.get_or_insert_with(Index::default);
        for moved in &outcome.moved {
            if outcome.mode == ArchiveMode::Move {
                index.renamed(&moved.from, &moved.to);
            }
            if let Some(channel) = &moved.channel {
                index.origin.insert(moved.to.clone(), channel.clone());
            }
            self.selected.remove(&moved.from);
            if let Some(viewer) = self.viewer.as_mut().filter(|v| v.path == moved.from)
                && outcome.mode == ArchiveMode::Move
            {
                viewer.path = moved.to.clone();
            }
        }
        let toast = archived_toast(&outcome);
        self.save_index(ctx)
            .with_task(self.reload(ctx))
            .with_toast(toast)
    }

    fn ask_delete(&mut self) -> Action<Message> {
        let paths: Vec<PathBuf> = self.selected.iter().cloned().collect();
        if paths.len() <= 1 {
            return self.delete(paths);
        }
        Action::confirm(
            Confirm::new(
                format!("Move {} to the trash?", count_of(paths.len())),
                "You can get them back from the system trash.",
            )
            .action(
                "Move to trash",
                ButtonKind::Danger,
                Message::DeleteConfirmed(paths),
            )
            .cancel("Cancel"),
        )
    }

    fn delete(&mut self, paths: Vec<PathBuf>) -> Action<Message> {
        if paths.is_empty() || self.busy.is_some() {
            return Action::none();
        }
        self.busy = Some(Busy::Deleting);
        let trash = self.services.trash.clone();
        Action::task(work::background(
            move || trash_all(paths, trash.as_ref()),
            |outcome| Message::Trashed(Box::new(outcome)),
        ))
    }

    fn trashed(&mut self, ctx: &Context, outcome: Trashed) -> Action<Message> {
        self.busy = None;
        let next = self
            .current()
            .filter(|path| outcome.done.contains(path))
            .map(|path| {
                self.neighbour(&path, 1)
                    .filter(|next| !outcome.done.contains(next))
                    .or_else(|| self.neighbour(&path, -1))
            });
        let gone: HashSet<&PathBuf> = outcome.done.iter().collect();
        if let Some(index) = self.index.as_mut() {
            for path in &outcome.done {
                index.removed(path);
            }
        }
        self.shots.retain(|shot| !gone.contains(&shot.path));
        self.selected.retain(|path| !gone.contains(path));
        self.thumbs.retain(|path, _| !gone.contains(path));
        self.refresh();
        let mut action = match next {
            Some(Some(path)) => self.open(path),
            Some(None) => self.back(ctx),
            None => Action::none(),
        };
        if !outcome.done.is_empty() {
            action = action.with_task(self.save_index(ctx).task);
        }
        action.with_toast(trashed_toast(&outcome))
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs an image decoder, which can panic on a damaged file, and treats a panic as a file
/// that cannot be read.
pub(crate) fn decoded<T>(
    decode: impl FnOnce() -> sc_core::error::Result<T>,
) -> sc_core::error::Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(decode)).unwrap_or_else(|_| {
        Err(sc_core::error::Error::Invalid(
            "the picture is damaged and cannot be read".into(),
        ))
    })
}

fn index_path(ctx: &Context) -> PathBuf {
    ctx.dirs.data.join(INDEX_FILE)
}

pub(crate) fn make_thumbs(paths: Vec<PathBuf>, cache: &Path) -> Vec<(PathBuf, Option<Handle>)> {
    paths
        .into_iter()
        .map(|path| {
            let handle = decoded(|| {
                screenshots::thumbnail(&path, cache, THUMB_EDGE)
                    .and_then(|thumb| screenshots::load_rgba(&thumb))
            })
            .ok()
            .map(|rgba| Handle::from_rgba(rgba.width, rgba.height, rgba.pixels));
            (path, handle)
        })
        .collect()
}

fn load_picture(path: &Path) -> Result<Picture, String> {
    let rgba = decoded(|| screenshots::load_rgba(path)).map_err(|err| words(&err))?;
    Ok(Picture {
        width: rgba.width,
        height: rgba.height,
        handle: Handle::from_rgba(rgba.width, rgba.height, rgba.pixels),
    })
}

fn archive_all(
    items: Vec<(PathBuf, Option<String>)>,
    archive: &Path,
    mode: ArchiveMode,
) -> Archived {
    let asked = items.len();
    let mut moved = Vec::new();
    for (from, channel) in items {
        match screenshots::archive(&from, archive, mode) {
            Ok(to) => moved.push(Moved { from, to, channel }),
            Err(err) => {
                return Archived {
                    mode,
                    moved,
                    asked,
                    failure: Some(format!(
                        "Could not archive {}. {}",
                        file_name(&from),
                        sentence(&words(&err))
                    )),
                };
            }
        }
    }
    Archived {
        mode,
        moved,
        asked,
        failure: None,
    }
}

fn trash_all(
    paths: Vec<PathBuf>,
    trash: &(dyn Fn(&Path) -> Result<(), String> + Send + Sync),
) -> Trashed {
    let asked = paths.len();
    let mut done = Vec::new();
    for path in paths {
        if let Err(err) = trash(&path) {
            return Trashed {
                done,
                asked,
                failure: Some(format!(
                    "Could not move {} to the trash. {}",
                    file_name(&path),
                    sentence(&err)
                )),
            };
        }
        done.push(path);
    }
    Trashed {
        done,
        asked,
        failure: None,
    }
}

fn archived_toast(outcome: &Archived) -> Toast {
    let verb = match outcome.mode {
        ArchiveMode::Move => "Moved",
        ArchiveMode::Copy => "Copied",
    };
    let done = outcome.moved.len();
    let what = if outcome.asked == 1 {
        String::new()
    } else {
        format!(" {}", count_of(done))
    };
    match &outcome.failure {
        None => Toast::success(format!("{verb}{what} to the archive")),
        Some(failure) if done == 0 => Toast::error(failure.clone()),
        Some(failure) => Toast::error(format!("{verb} {done} of {}. {failure}", outcome.asked)),
    }
}

fn trashed_toast(outcome: &Trashed) -> Toast {
    let done = outcome.done.len();
    match &outcome.failure {
        None if outcome.asked == 1 => Toast::success(MOVED_TO_TRASH),
        None => Toast::success(format!("Moved {} to the trash", count_of(done))),
        Some(failure) if done == 0 => Toast::error(failure.clone()),
        Some(failure) => Toast::error(format!(
            "Moved {done} of {} to the trash. {failure}",
            outcome.asked
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::decoded;

    #[test]
    fn a_decoder_that_panics_reads_as_a_damaged_picture() {
        let result: sc_core::error::Result<()> = decoded(|| panic!("bad huffman table"));
        let err = result.unwrap_err();
        assert!(crate::screens::words::words(&err).contains("the picture is damaged"));
    }
}
