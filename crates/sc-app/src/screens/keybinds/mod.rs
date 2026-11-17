mod advanced;
mod capture;
mod inputs;
mod model;
mod source;
mod view;

pub use crate::screens::words::{GAME_DATA_LOADING, TRY_AGAIN};
pub use advanced::{
    ADVANCED_TAB, AdvancedMessage, BINDS_TAB, DevicePick, GAME_DEFAULT, NOT_A_NUMBER,
    NOT_CONNECTED, OptionField, POINT_RANGE, PointPart,
};
pub use model::CellRef;
pub use source::{CURRENT_LABEL, Source, SourceChoice, UNRECOGNISED, layout_name};

use crate::context::Context;
use crate::game_data::Loaded as GameLoad;
use crate::screen::{Action, Confirm, Origin, Toast};
use crate::screens::backups::words;
use crate::screens::edits::{CHANGED_ON_DISK, FORMAT_CHANGED};
use crate::screens::words::{GAME_RUNNING, ROLLBACK_FAILED};
use crate::theme::ButtonKind;
use crate::widgets::VirtualListState;
use crate::work;
use capture::{Capture, Outcome};
use iced::{Element, Task};
use inputs::{Known, STICKS, Typed, captures, judge};
use model::{Baseline, Line, Model};
use sc_core::actionmaps::{ActionMaps, DeviceKind, Op, Slot, layout_file_name};
use sc_core::backup::SnapshotId;
use sc_core::defaults::GameData;
use sc_core::guard::Writer;
use sc_core::safe_write::{Hash, LoadedFile};
use sc_core::staging::{Document, Staged};
use source::{Handoff, Loaded, Opened, WriteFailure};
use std::fmt;
use std::path::PathBuf;

pub const RESET_CAPTION: &str = "Removes your bind so the game's default applies again.";
pub const UNBIND_CAPTION: &str =
    "Leaves this action with no bind on this device, even if the game has a default.";
pub const SHOW_UNLISTED: &str = "Show actions the game's menu hides";
pub const NOT_IN_BUILD: &str = "Not in this game version";
pub const NO_GAME_DATA_NOTE: &str =
    "Game data unavailable: showing only the binds stored in your files.";
pub const ADD_CAPTION: &str = "For actions that have no bind in this file yet. Use the action's name as the game writes it, for example v_toggle_mining_mode.";
pub const CAPTURE_PROMPT: &str = "Press a key or a mouse button";
pub const UNKNOWN_KEY: &str = "This key has no known name. Type it below.";
pub const CHANGED_AGAIN: &str = CHANGED_ON_DISK;
pub const EMPTY_TITLE: &str = "No keybinds yet";
pub const EMPTY_BODY: &str = "Change any bind in the game once, or export a profile from Options, Keybindings, Control Profiles. It shows up here after that.";
pub const CLOSE_TO_APPLY: &str = GAME_RUNNING;
const MOUSE_ONLY: &str =
    "Press a mouse button or turn the wheel over this box. Keys go in the Keyboard column.";

/// A device slot offered in a pick list, labelled with the stick's product name when known.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotChoice {
    pub slot: Slot,
    pub label: String,
}

impl fmt::Display for SlotChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// The two views of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Binds,
    Advanced,
}

enum Doc {
    Loading,
    Empty,
    Failed(source::Failure),
    Ready {
        staged: Box<Staged<ActionMaps>>,
        writable: bool,
    },
    /// The staged document is with a background write.
    Away,
}

struct Edit {
    cell: CellRef,
    /// Where Set writes: the cell's current device, or the stick picked for it.
    slot: Slot,
    typed: String,
    listening: bool,
    hovered: bool,
    note: Option<&'static str>,
    capture: Capture,
}

#[derive(Default)]
struct AddForm {
    map: String,
    action: String,
    slot: Option<SlotChoice>,
    input: String,
}

struct SwapForm {
    a: Option<SlotChoice>,
    b: Option<SlotChoice>,
}

#[derive(Default)]
struct SaveForm {
    name: String,
}

/// The table's view of the file as loaded, kept while that file and the game data stay.
struct BaselineCache {
    file: (PathBuf, Option<Hash>),
    game: Option<GameLoad>,
    baseline: Baseline,
}

impl BaselineCache {
    fn fits(&self, loaded: &LoadedFile, game: &Option<GameLoad>) -> bool {
        let same_game = match (&self.game, game) {
            (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        same_game && self.file.0 == loaded.path && self.file.1 == loaded.hash
    }
}

enum Panel {
    Closed,
    Edit(Edit),
    Add(AddForm),
    Swap(SwapForm),
    SaveAs(SaveForm),
}

pub struct State {
    channel: Option<PathBuf>,
    sources: Vec<SourceChoice>,
    source: Option<Source>,
    doc: Doc,
    away_pending: usize,
    game: Option<GameLoad>,
    game_loading: bool,
    /// Why the game's defaults are missing, in words, once a load has said so.
    no_game_note: Option<String>,
    show_unlisted: bool,
    baseline: Option<BaselineCache>,
    model: Option<Model>,
    lines: Vec<Line>,
    map: Option<String>,
    search: String,
    conflicts_only: bool,
    panel: Panel,
    list: VirtualListState,
    raw_list: VirtualListState,
    saving: bool,
    tab: Tab,
    advanced: advanced::Advanced,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(PathBuf, Loaded),
    GameLoaded(PathBuf, GameLoad),
    RetryGameData,
    Listed(PathBuf, Vec<SourceChoice>),
    PickSource(SourceChoice),
    SwitchSource(Source),
    Opened(PathBuf, Source, Opened),
    PickMap(Option<String>),
    Search(String),
    ToggleConflicts,
    ShowUnlisted(bool),
    Scrolled(f32),
    RawScrolled(f32),
    /// The bind table's visible height, measured as it shows or resizes.
    TableResized(f32),
    Select(CellRef),
    ClosePanel,
    Input(iced::Event),
    CaptureHover(bool),
    Listen,
    Typed(String),
    QuickPick(String),
    PickStick(SlotChoice),
    Set,
    Unbind,
    Reset,
    OpenAdd,
    AddMap(String),
    AddAction(String),
    AddSlot(SlotChoice),
    AddInput(String),
    AddBind,
    OpenSwap,
    SwapA(SlotChoice),
    SwapB(SlotChoice),
    Swap,
    OpenSaveAs,
    SaveName(String),
    SaveLayout(bool),
    LayoutSaved(Origin, String, Result<SnapshotId, WriteFailure>),
    Applied(Origin, Handoff, Result<SnapshotId, WriteFailure>),
    ShowTab(Tab),
    Advanced(AdvancedMessage),
}

impl State {
    pub fn new() -> Self {
        Self {
            channel: None,
            sources: Vec::new(),
            source: None,
            doc: Doc::Loading,
            away_pending: 0,
            game: None,
            game_loading: false,
            no_game_note: None,
            show_unlisted: false,
            baseline: None,
            model: None,
            lines: Vec::new(),
            map: None,
            search: String::new(),
            conflicts_only: false,
            panel: Panel::Closed,
            list: VirtualListState::default(),
            raw_list: VirtualListState::default(),
            saving: false,
            tab: Tab::Binds,
            advanced: advanced::Advanced::new(),
        }
    }

    pub fn reload(&mut self, ctx: &Context) -> Task<Message> {
        let Some(channel) = ctx.channel.clone() else {
            *self = Self::new();
            return Task::none();
        };
        if self.channel.as_ref() != Some(&channel.dir) {
            *self = Self::new();
            self.channel = Some(channel.dir.clone());
        }
        if self.pending() > 0 {
            return Task::none();
        }
        let wanted = self.source.clone();
        let dir = channel.dir.clone();
        let game = match ctx.game_data().cached(&channel) {
            Some(game) => {
                self.take_game(game);
                Task::none()
            }
            None => self.read_game(ctx),
        };
        let file = work::background(
            move || source::load(&channel, wanted),
            move |loaded| Message::Loaded(dir, loaded),
        );
        Task::batch([file, game])
    }

    /// Reads the game's defaults in the background; the file is shown without them meanwhile.
    fn read_game(&mut self, ctx: &Context) -> Task<Message> {
        let (Some(channel), Some(dir)) = (ctx.channel.clone(), self.channel.clone()) else {
            return Task::none();
        };
        self.game_loading = true;
        let store = ctx.game_data();
        let cache_root = ctx.dirs.game_data();
        work::background(
            move || store.load(&channel, &cache_root),
            move |game| Message::GameLoaded(dir, game),
        )
    }

    /// Forgets a failed read, so a `Data.p4k` that was locked or still downloading is read again.
    fn retry_game(&mut self, ctx: &Context) -> Task<Message> {
        let Some(channel) = ctx.channel.as_ref() else {
            return Task::none();
        };
        if self.channel.as_ref() != Some(&channel.dir) || self.game_loading {
            return Task::none();
        }
        ctx.game_data().forget(channel);
        self.no_game_note = None;
        self.read_game(ctx)
    }

    fn take_game(&mut self, game: GameLoad) {
        self.game_loading = false;
        if self
            .game
            .as_ref()
            .is_some_and(|held| std::sync::Arc::ptr_eq(held, &game))
        {
            return;
        }
        self.no_game_note = game
            .as_ref()
            .as_ref()
            .err()
            .map(|unavailable| format!("{NO_GAME_DATA_NOTE} {}", unavailable.reason()));
        self.game = Some(game);
        self.refresh();
    }

    fn game(&self) -> Option<&GameData> {
        game_of(&self.game)
    }

    /// True while a keyboard or mouse cell is listening, so the shell sends raw input here
    /// and leaves its own shortcuts alone.
    pub fn wants_input(&self) -> bool {
        matches!(&self.panel, Panel::Edit(edit) if edit.listening) && self.writable()
    }

    fn writable(&self) -> bool {
        matches!(self.doc, Doc::Ready { writable: true, .. })
    }

    fn staged(&self) -> Option<&Staged<ActionMaps>> {
        match &self.doc {
            Doc::Ready { staged, .. } => Some(staged),
            _ => None,
        }
    }

    pub fn update(&mut self, message: Message, ctx: &Context) -> Action<Message> {
        match message {
            Message::Loaded(dir, loaded) => {
                self.loaded(dir, loaded);
                Action::none()
            }
            Message::GameLoaded(dir, game) => {
                if self.channel.as_ref() == Some(&dir) {
                    self.take_game(game);
                }
                Action::none()
            }
            Message::RetryGameData => Action::task(self.retry_game(ctx)),
            Message::Listed(dir, sources) => {
                if self.channel.as_ref() == Some(&dir) {
                    self.sources = sources;
                }
                Action::none()
            }
            Message::PickSource(choice) => self.pick_source(choice),
            Message::SwitchSource(source) => self.switch_source(source, ctx),
            Message::Opened(dir, source, opened) => {
                if self.channel.as_ref() == Some(&dir) && self.source.as_ref() == Some(&source) {
                    self.take_opened(opened);
                    self.refresh();
                }
                Action::none()
            }
            Message::PickMap(map) => {
                self.map = map;
                self.refilter();
                Action::none()
            }
            Message::Search(search) => {
                self.search = search;
                self.refilter();
                Action::none()
            }
            Message::ToggleConflicts => {
                self.conflicts_only = !self.conflicts_only;
                self.refilter();
                Action::none()
            }
            Message::ShowUnlisted(show) => {
                self.show_unlisted = show;
                let hidden = |model: &Model, map: &str| {
                    model
                        .maps
                        .iter()
                        .all(|entry| entry.name != map || entry.count(show) == 0)
                };
                if let (Some(model), Some(map)) = (&self.model, &self.map)
                    && hidden(model, map)
                {
                    self.map = None;
                }
                self.refresh();
                Action::none()
            }
            Message::Scrolled(offset) => {
                self.list.offset = offset;
                Action::none()
            }
            Message::RawScrolled(offset) => {
                self.raw_list.offset = offset;
                Action::none()
            }
            Message::TableResized(height) => {
                self.list.viewport_height = height;
                Action::task(self.reveal_selected())
            }
            Message::Select(cell) => {
                self.select(cell);
                Action::task(self.reveal_selected())
            }
            Message::ClosePanel => {
                self.panel = Panel::Closed;
                Action::none()
            }
            Message::Input(event) => self.input(&event),
            Message::Listen => {
                if let Panel::Edit(edit) = &mut self.panel {
                    edit.listening = captures(edit.cell.kind);
                    edit.note = None;
                    edit.capture = Capture::default();
                }
                Action::none()
            }
            Message::Set => self.set_typed(),
            Message::Unbind => self.unbind(),
            Message::Reset => self.reset(),
            Message::OpenAdd => {
                self.open_add();
                Action::none()
            }
            Message::AddBind => self.add_bind(),
            Message::OpenSwap => {
                self.open_swap();
                Action::none()
            }
            Message::Swap => self.swap(),
            Message::OpenSaveAs => {
                if self.can_save_layout() {
                    self.panel = Panel::SaveAs(SaveForm::default());
                }
                Action::none()
            }
            Message::SaveLayout(overwrite) => self.save_layout(overwrite, ctx),
            Message::LayoutSaved(origin, name, result) => {
                self.layout_saved(origin, name, result, ctx)
            }
            Message::Applied(origin, handoff, result) => self.applied(origin, handoff, result),
            Message::ShowTab(tab) => {
                self.show_tab(tab);
                Action::none()
            }
            Message::Advanced(message) => self.advanced_update(message),
            other => {
                self.edit_form(other);
                Action::none()
            }
        }
    }

    fn edit_form(&mut self, message: Message) {
        match (&mut self.panel, message) {
            (Panel::Edit(edit), Message::CaptureHover(hovered)) => edit.hovered = hovered,
            (Panel::Edit(edit), Message::Typed(typed) | Message::QuickPick(typed)) => {
                edit.typed = typed;
            }
            (Panel::Edit(edit), Message::PickStick(choice))
                if edit.cell.kind == DeviceKind::Joystick
                    && choice.slot.kind == DeviceKind::Joystick =>
            {
                edit.slot = choice.slot;
            }
            (Panel::Add(form), Message::AddMap(map)) => form.map = map,
            (Panel::Add(form), Message::AddAction(action)) => form.action = action,
            (Panel::Add(form), Message::AddSlot(slot)) => form.slot = Some(slot),
            (Panel::Add(form), Message::AddInput(input)) => form.input = input,
            (Panel::Swap(form), Message::SwapA(slot)) => form.a = Some(slot),
            (Panel::Swap(form), Message::SwapB(slot)) => form.b = Some(slot),
            (Panel::SaveAs(form), Message::SaveName(name)) => form.name = name,
            _ => {}
        }
    }

    fn loaded(&mut self, dir: PathBuf, loaded: Loaded) {
        if self.channel.as_ref() != Some(&dir) || self.pending() > 0 {
            return;
        }
        self.sources = loaded.sources;
        self.source = loaded.source;
        match loaded.opened {
            None => self.doc = Doc::Empty,
            Some(opened) => self.take_opened(opened),
        }
        self.refresh();
    }

    fn take_opened(&mut self, opened: Opened) {
        self.doc = match opened {
            Opened::Ready { doc, writable } => match doc.take() {
                Some(staged) => Doc::Ready {
                    staged: Box::new(staged),
                    writable,
                },
                None => Doc::Loading,
            },
            Opened::Failed(failure) => Doc::Failed(failure),
        };
        self.raw_list = VirtualListState::default();
    }

    /// Rebuilds the table after the document or the game's defaults changed.
    fn refresh(&mut self) {
        if let Doc::Ready { staged, .. } = &self.doc {
            let game = game_of(&self.game);
            let loaded = staged.loaded();
            if !self
                .baseline
                .as_ref()
                .is_some_and(|cache| cache.fits(loaded, &self.game))
            {
                self.baseline = Some(BaselineCache {
                    file: (loaded.path.clone(), loaded.hash),
                    game: self.game.clone(),
                    baseline: Baseline::of(game, staged.base()),
                });
            }
            if let Some(cache) = &self.baseline {
                self.model = Some(Model::build(
                    game,
                    staged.current(),
                    &cache.baseline,
                    self.show_unlisted,
                ));
            }
        } else if !matches!(self.doc, Doc::Away) {
            self.model = None;
        }
        self.rebuild_advanced();
        if let (Some(model), Some(map)) = (&self.model, &self.map)
            && !model.maps.iter().any(|entry| &entry.name == map)
        {
            self.map = None;
        }
        let keep = match (&self.panel, &self.model) {
            (Panel::Edit(edit), Some(model)) => {
                model.row(&edit.cell.map, &edit.cell.action).is_some()
            }
            (Panel::Closed, _) => true,
            (_, model) => model.is_some(),
        };
        if !keep || !self.writable() {
            self.panel = Panel::Closed;
        }
        self.refilter();
    }

    fn refilter(&mut self) {
        self.lines = match &self.model {
            Some(model) => model.lines(
                self.map.as_deref(),
                &self.search,
                self.conflicts_only,
                self.show_unlisted,
            ),
            None => Vec::new(),
        };
        self.list = VirtualListState::default();
    }

    fn pick_source(&mut self, choice: SourceChoice) -> Action<Message> {
        if self.source.as_ref() == Some(&choice.source) {
            return Action::none();
        }
        if self.pending() == 0 {
            return Action::task(Task::done(Message::SwitchSource(choice.source)));
        }
        Action::confirm(
            Confirm::new(
                "Discard unsaved changes?",
                format!(
                    "Opening {} loses your unsaved keybind changes.",
                    choice.label
                ),
            )
            .action(
                "Discard and switch",
                ButtonKind::Danger,
                Message::SwitchSource(choice.source),
            )
            .cancel("Cancel"),
        )
    }

    fn switch_source(&mut self, source: Source, ctx: &Context) -> Action<Message> {
        let (Some(channel), Some(dir)) = (ctx.channel.clone(), self.channel.clone()) else {
            return Action::none();
        };
        if matches!(self.doc, Doc::Away) {
            return Action::none();
        }
        self.discard();
        self.source = Some(source.clone());
        self.doc = Doc::Loading;
        self.panel = Panel::Closed;
        self.refresh();
        let path = source.path(&channel);
        Action::task(work::background(
            move || source::open(&path),
            move |opened| Message::Opened(dir, source, opened),
        ))
    }

    /// Scrolls the table so the row being edited stays in view above the edit panel.
    fn reveal_selected(&mut self) -> Task<Message> {
        let (Panel::Edit(edit), Some(model)) = (&self.panel, &self.model) else {
            return Task::none();
        };
        let viewport = self.list.viewport_height;
        let index = self.lines.iter().position(|line| {
            matches!(line, Line::Action(row)
                if model.rows[*row].map == edit.cell.map && model.rows[*row].action == edit.cell.action)
        });
        let Some(index) = index.filter(|_| viewport > 0.0) else {
            return Task::none();
        };
        let top = index as f32 * view::ROW_HEIGHT;
        let bottom = top + view::ROW_HEIGHT;
        let offset = if bottom > self.list.offset + viewport {
            bottom - viewport
        } else if top < self.list.offset {
            top
        } else {
            return Task::none();
        };
        self.list.offset = offset;
        iced::widget::operation::scroll_to(
            view::TABLE_ID,
            iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: offset },
        )
    }

    fn select(&mut self, cell: CellRef) {
        if !self.writable() {
            return;
        }
        let Some(current) = self.model.as_ref().and_then(|model| model.cell(&cell)) else {
            return;
        };
        let typed = current.rest().unwrap_or_default().to_string();
        let slot = current
            .slot()
            .or_else(|| current.default_slot())
            .filter(|slot| slot.kind == cell.kind)
            .unwrap_or(Slot {
                kind: cell.kind,
                instance: 1,
            });
        let listening = captures(cell.kind);
        self.panel = Panel::Edit(Edit {
            cell,
            slot,
            typed,
            listening,
            hovered: false,
            note: None,
            capture: Capture::default(),
        });
    }

    fn input(&mut self, event: &iced::Event) -> Action<Message> {
        if !self.wants_input() {
            return Action::none();
        }
        let Panel::Edit(edit) = &mut self.panel else {
            return Action::none();
        };
        let rest = match edit.capture.event(event, edit.hovered) {
            Outcome::Waiting => return Action::none(),
            Outcome::Cancelled => {
                edit.listening = false;
                edit.note = None;
                return Action::none();
            }
            Outcome::Unknown => {
                edit.note = Some(UNKNOWN_KEY);
                return Action::none();
            }
            Outcome::Done(rest) => rest,
        };
        let (slot, cell) = (edit.slot, edit.cell.clone());
        let judged = judge(slot, &rest, self.known(&cell));
        let Panel::Edit(edit) = &mut self.panel else {
            return Action::none();
        };
        match judged {
            Typed::Valid(input) => {
                edit.listening = false;
                edit.note = None;
                edit.typed = rest;
                self.stage(bind_op(&cell, input))
            }
            Typed::Empty | Typed::Invalid(_) => {
                edit.note = Some(if cell.kind == DeviceKind::Mouse {
                    MOUSE_ONLY
                } else {
                    UNKNOWN_KEY
                });
                Action::none()
            }
        }
    }

    /// What the edited cell's input is checked against: the game's key names and the
    /// cell's own default.
    fn known(&self, cell: &CellRef) -> Known<'_> {
        Known {
            game: self.game(),
            default: self
                .model
                .as_ref()
                .and_then(|model| model.cell(cell))
                .and_then(model::Cell::default_rest),
        }
    }

    fn set_typed(&mut self) -> Action<Message> {
        let Panel::Edit(edit) = &self.panel else {
            return Action::none();
        };
        let Typed::Valid(input) = judge(edit.slot, &edit.typed, self.known(&edit.cell)) else {
            return Action::none();
        };
        let op = bind_op(&edit.cell, input);
        if let Panel::Edit(edit) = &mut self.panel {
            edit.listening = false;
        }
        self.stage(op)
    }

    /// The cell being edited, as the table shows it now.
    fn edited_cell(&self) -> Option<(&Edit, &model::Cell)> {
        let Panel::Edit(edit) = &self.panel else {
            return None;
        };
        let cell = self.model.as_ref()?.cell(&edit.cell)?;
        Some((edit, cell))
    }

    /// Removes every entry of the cell's kind from the action, whichever stick it is on.
    fn reset(&mut self) -> Action<Message> {
        let Some((edit, cell)) = self.edited_cell() else {
            return Action::none();
        };
        if !cell.resettable() {
            return Action::none();
        }
        let op = Op::Reset {
            map: edit.cell.map.clone(),
            action: edit.cell.action.clone(),
            kind: edit.cell.kind,
        };
        self.stage_from_panel(op)
    }

    /// Writes the game's "no bind" on the device the cell shows, or on the stick picked for a
    /// joystick cell, replacing every entry of its kind.
    fn unbind(&mut self) -> Action<Message> {
        let Some((edit, cell)) = self.edited_cell() else {
            return Action::none();
        };
        if !cell.unbindable() {
            return Action::none();
        }
        let slot = match edit.cell.kind {
            DeviceKind::Joystick => Some(edit.slot),
            _ => cell.slot(),
        };
        let Some(slot) = slot else {
            return Action::none();
        };
        let op = Op::Unbind {
            map: edit.cell.map.clone(),
            action: edit.cell.action.clone(),
            slot,
        };
        self.stage_from_panel(op)
    }

    fn stage_from_panel(&mut self, op: Op) -> Action<Message> {
        if let Panel::Edit(edit) = &mut self.panel {
            edit.listening = false;
            edit.typed.clear();
        }
        self.stage(op)
    }

    fn stage(&mut self, op: Op) -> Action<Message> {
        let Doc::Ready {
            staged,
            writable: true,
        } = &mut self.doc
        else {
            return Action::none();
        };
        match staged.stage(op) {
            Ok(()) => {
                self.refresh();
                Action::none()
            }
            Err(err) => Action::toast(Toast::error(format!(
                "Could not make that change: {}",
                words(&err)
            ))),
        }
    }

    fn slot_choice(&self, slot: Slot) -> SlotChoice {
        let label = inputs::slot_label(slot);
        let product = self
            .model
            .as_ref()
            .and_then(|model| model.products.get(&slot));
        let label = match product {
            Some(product) if slot.kind == DeviceKind::Joystick => format!("{label}: {product}"),
            _ => label,
        };
        SlotChoice { slot, label }
    }

    /// Devices offered by the add form: keyboard, mouse and gamepad, then each known stick.
    fn slot_choices(&self) -> Vec<SlotChoice> {
        let kinds = [DeviceKind::Keyboard, DeviceKind::Mouse, DeviceKind::Gamepad];
        let mut slots: Vec<Slot> = kinds
            .into_iter()
            .map(|kind| Slot { kind, instance: 1 })
            .collect();
        let mut sticks = self
            .model
            .as_ref()
            .map(Model::joysticks)
            .unwrap_or_default();
        if sticks.is_empty() {
            sticks.push(Slot {
                kind: DeviceKind::Joystick,
                instance: 1,
            });
        }
        slots.extend(sticks);
        slots
            .into_iter()
            .map(|slot| self.slot_choice(slot))
            .collect()
    }

    /// Every stick the game can tell apart, for the joystick cell's picker.
    fn stick_choices(&self) -> Vec<SlotChoice> {
        (1..=STICKS)
            .map(|instance| {
                self.slot_choice(Slot {
                    kind: DeviceKind::Joystick,
                    instance,
                })
            })
            .collect()
    }

    fn joystick_choices(&self) -> Vec<SlotChoice> {
        self.model
            .as_ref()
            .map(Model::joysticks)
            .unwrap_or_default()
            .into_iter()
            .map(|slot| self.slot_choice(slot))
            .collect()
    }

    fn open_add(&mut self) {
        if !self.writable() || self.game().is_some() || self.game_loading {
            return;
        }
        let map = self.map.clone().unwrap_or_default();
        let slot = self.slot_choices().into_iter().next();
        self.panel = Panel::Add(AddForm {
            map,
            slot,
            ..AddForm::default()
        });
    }

    fn add_bind(&mut self) -> Action<Message> {
        let Panel::Add(form) = &self.panel else {
            return Action::none();
        };
        let Some((slot, input)) = add_input(form) else {
            return Action::none();
        };
        let cell = CellRef {
            map: form.map.trim().to_string(),
            action: form.action.trim().to_string(),
            kind: slot.kind,
        };
        let action = self.stage(bind_op(&cell, input));
        if action.toast.is_none() {
            self.map = Some(cell.map.clone());
            self.search.clear();
            self.conflicts_only = false;
            self.refilter();
            self.select(cell);
            if let Panel::Edit(edit) = &mut self.panel {
                edit.listening = false;
            }
        }
        action
    }

    fn open_swap(&mut self) {
        let joysticks = self.joystick_choices();
        if !self.writable() || joysticks.len() < 2 {
            return;
        }
        self.panel = Panel::Swap(SwapForm {
            a: joysticks.first().cloned(),
            b: joysticks.get(1).cloned(),
        });
    }

    fn swap(&mut self) -> Action<Message> {
        let Panel::Swap(SwapForm {
            a: Some(a),
            b: Some(b),
        }) = &self.panel
        else {
            return Action::none();
        };
        if a.slot == b.slot {
            return Action::none();
        }
        let op = Op::SwapJoysticks {
            a: a.slot.instance,
            b: b.slot.instance,
        };
        self.panel = Panel::Closed;
        self.stage(op)
    }

    fn save_layout(&mut self, overwrite: bool, ctx: &Context) -> Action<Message> {
        let Panel::SaveAs(form) = &self.panel else {
            return Action::none();
        };
        let (Some(origin), Some(staged)) = (Origin::of(ctx), self.staged()) else {
            return Action::none();
        };
        let (scope, channel) = (origin.scope.clone(), origin.channel.clone());
        if self.saving || !self.can_save_layout() {
            return Action::none();
        }
        let name = form.name.trim().to_string();
        let Ok(file_name) = layout_file_name(&name) else {
            return Action::none();
        };
        let bytes = staged.current().to_bytes();
        let target = channel.mappings_dir().join(file_name);
        let writer = layout_writer(ctx);
        self.saving = true;
        Action::task(work::background(
            move || source::save_layout(bytes, target, overwrite, writer, scope, channel),
            move |result| Message::LayoutSaved(origin, name, result),
        ))
    }

    fn can_save_layout(&self) -> bool {
        self.writable() && self.source.as_ref().is_some_and(Source::is_layout)
    }

    fn layout_saved(
        &mut self,
        origin: Origin,
        name: String,
        result: Result<SnapshotId, WriteFailure>,
        ctx: &Context,
    ) -> Action<Message> {
        let here = self.channel.as_ref() == Some(&origin.channel.dir);
        if here {
            self.saving = false;
        }
        match result {
            Ok(snapshot) => {
                let toast = Toast::success(format!(
                    "Saved layout \"{name}\". Load it in game from Options, Keybindings, Control Profiles."
                ))
                .with_undo_in(vec![origin.undo(snapshot)]);
                if !here {
                    return Action::toast(toast);
                }
                self.panel = Panel::Closed;
                Action::task(self.relist(ctx)).with_toast(toast)
            }
            Err(WriteFailure::Exists) if !here => Action::none(),
            Err(WriteFailure::Exists) => Action::confirm(
                Confirm::new(
                    format!("Replace layout \"{name}\"?"),
                    "A layout with this name is already in Controls/Mappings. Saving replaces it with these binds. A snapshot is taken first.",
                )
                .action("Replace", ButtonKind::Danger, Message::SaveLayout(true))
                .cancel("Cancel"),
            ),
            Err(failure) => Action::toast(failure_toast("Could not save the layout", &failure)),
        }
    }

    fn relist(&self, ctx: &Context) -> Task<Message> {
        let (Some(channel), Some(dir)) = (ctx.channel.clone(), self.channel.clone()) else {
            return Task::none();
        };
        work::background(
            move || source::list(&channel),
            move |sources| Message::Listed(dir, sources),
        )
    }

    fn applied(
        &mut self,
        origin: Origin,
        handoff: Handoff,
        result: Result<SnapshotId, WriteFailure>,
    ) -> Action<Message> {
        let applied = handoff.count();
        let here =
            self.channel.as_ref() == Some(&origin.channel.dir) && matches!(self.doc, Doc::Away);
        if here {
            self.away_pending = 0;
            if let Some(staged) = handoff.take() {
                let writable = source::round_trips(&staged);
                self.doc = Doc::Ready {
                    staged: Box::new(staged),
                    writable,
                };
            }
            self.refresh();
        }
        let toast = match result {
            Ok(snapshot) => {
                Toast::success(applied_text(applied)).with_undo_in(vec![origin.undo(snapshot)])
            }
            Err(failure) => failure_toast("Could not apply the keybind changes", &failure),
        };
        Action::toast(toast)
    }

    pub fn view<'a>(&'a self, _ctx: &'a Context) -> Element<'a, Message> {
        self.screen()
    }

    pub fn pending(&self) -> usize {
        match &self.doc {
            Doc::Ready { staged, .. } => staged.pending(),
            Doc::Away => self.away_pending,
            _ => 0,
        }
    }

    pub fn apply(&mut self, ctx: &Context) -> Action<Message> {
        let pending = self.pending();
        let (Some(scope), Some(channel)) = (ctx.scope(), ctx.channel.clone()) else {
            return Action::none();
        };
        if pending == 0 || !self.writable() {
            return Action::none();
        }
        let layout = self.source.as_ref().is_some_and(Source::is_layout);
        if ctx.read_only() && !layout {
            return Action::toast(Toast::error(CLOSE_TO_APPLY));
        }
        let Doc::Ready { staged, .. } = std::mem::replace(&mut self.doc, Doc::Away) else {
            return Action::none();
        };
        self.away_pending = pending;
        let writer = if layout {
            layout_writer(ctx)
        } else {
            ctx.writer()
        };
        let origin = Origin {
            scope: scope.clone(),
            channel: channel.clone(),
        };
        Action::task(work::background(
            move || source::apply(*staged, pending, writer, scope, channel),
            move |(handoff, result)| Message::Applied(origin, handoff, result),
        ))
    }

    pub fn discard(&mut self) {
        if let Doc::Ready { staged, .. } = &mut self.doc {
            staged.discard();
            self.panel = Panel::Closed;
            self.refresh();
        }
    }

    pub fn undo(&mut self, _ctx: &Context) -> Action<Message> {
        if let Doc::Ready { staged, .. } = &mut self.doc
            && staged.undo_last()
        {
            self.refresh();
        }
        Action::none()
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

fn game_of(game: &Option<GameLoad>) -> Option<&GameData> {
    game.as_deref().and_then(|loaded| loaded.as_ref().ok())
}

fn bind_op(cell: &CellRef, input: String) -> Op {
    Op::Bind {
        map: cell.map.clone(),
        action: cell.action.clone(),
        input,
    }
}

/// The slot and whole input the add form would stage, when every field is usable.
fn add_input(form: &AddForm) -> Option<(Slot, String)> {
    let slot = form.slot.as_ref()?.slot;
    if form.map.trim().is_empty() || form.action.trim().is_empty() {
        return None;
    }
    match judge(slot, &form.input, Known::default()) {
        Typed::Valid(input) => Some((slot, input)),
        _ => None,
    }
}

/// Exported layouts are safe to write while the game runs, so this writer does not wait
/// for it to close. It still snapshots first.
fn layout_writer(ctx: &Context) -> Writer {
    Writer::new(ctx.writer().store().clone(), ctx.probe(), true)
}

fn applied_text(count: usize) -> String {
    if count == 1 {
        "Applied 1 keybind change".to_string()
    } else {
        format!("Applied {count} keybind changes")
    }
}

fn failure_toast(what: &str, failure: &WriteFailure) -> Toast {
    match failure {
        WriteFailure::RollbackFailed => Toast::error(ROLLBACK_FAILED),
        WriteFailure::GameRunning => Toast::error(CLOSE_TO_APPLY),
        WriteFailure::ChangedAgain => Toast::info(CHANGED_AGAIN),
        WriteFailure::Exists => {
            Toast::error(format!("{what}: a layout with this name already exists."))
        }
        WriteFailure::NoLongerApply(count) => Toast::info(removed_text(*count)),
        WriteFailure::Unrecognised => Toast::error(FORMAT_CHANGED),
        WriteFailure::Failed(reason) => Toast::error(format!("{what}: {reason}")),
    }
}

fn removed_text(count: usize) -> String {
    if count == 1 {
        "The file changed on disk and 1 of your edits no longer fits it, so it was removed. Check the rest and apply again.".to_string()
    } else {
        format!(
            "The file changed on disk and {count} of your edits no longer fit it, so they were removed. Check the rest and apply again."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit;

    const DEMO: &str = include_str!("../../../tests/fixtures/keybinds/actionmaps.xml");

    #[test]
    fn the_file_shows_before_the_game_data_arrives() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = testkit::context_with_channels(temp.path(), &["LIVE"]);
        let live = ctx.channel.clone().unwrap();
        testkit::write_build(&live, testkit::LIVE_VERSION, testkit::LIVE_CHANGE);
        testkit::seed_keybinds_game_data(&ctx);
        let path = live.actionmaps_xml();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, DEMO).unwrap();

        let mut state = State::new();
        let _ = state.reload(&ctx);
        assert!(state.game_loading);
        let dir = live.dir.clone();
        let _ = state.update(
            Message::Loaded(dir.clone(), source::load(&live, None)),
            &ctx,
        );
        let without = state.model.as_ref().expect("the file is shown").rows.len();
        assert!(state.game().is_none());
        assert!(
            state.game_loading,
            "the game's defaults are still on their way"
        );

        let game = ctx.game_data().load(&live, &ctx.dirs.game_data());
        let _ = state.update(Message::GameLoaded(dir, game), &ctx);
        assert!(!state.game_loading);
        assert!(state.game().is_some());
        let with = state.model.as_ref().unwrap().rows.len();
        assert!(
            with > without,
            "{with} rows with the game's actions, {without} without"
        );
    }

    #[test]
    fn edits_that_no_longer_fit_are_called_removed() {
        let toast = failure_toast("x", &WriteFailure::NoLongerApply(2));
        assert!(
            toast.text.contains("so they were removed"),
            "{}",
            toast.text
        );
        let toast = failure_toast("x", &WriteFailure::NoLongerApply(1));
        assert!(toast.text.contains("so it was removed"), "{}", toast.text);
    }

    #[test]
    fn an_existing_layout_is_not_called_a_change_on_disk() {
        let toast = failure_toast("Could not save the layout", &WriteFailure::Exists);
        assert_eq!(
            toast.text,
            "Could not save the layout: a layout with this name already exists."
        );
    }

    #[test]
    fn a_file_that_no_longer_round_trips_is_not_written() {
        let toast = failure_toast("x", &WriteFailure::Unrecognised);
        assert_eq!(toast.text, FORMAT_CHANGED);
    }
}
