mod capture;
mod inputs;
mod model;
mod source;
mod view;

pub use model::CellRef;
pub use source::{CURRENT_LABEL, Source, SourceChoice, UNRECOGNISED, layout_name};

use crate::context::Context;
use crate::screen::{Action, Confirm, Toast};
use crate::screens::backups::words;
use crate::theme::ButtonKind;
use crate::widgets::VirtualListState;
use crate::work;
use capture::{Capture, Outcome};
use iced::{Element, Task};
use inputs::{Typed, captures, judge};
use model::{Line, Model};
use sc_core::actionmaps::{ActionMaps, DeviceKind, Op, Slot, layout_file_name};
use sc_core::backup::SnapshotId;
use sc_core::guard::Writer;
use sc_core::staging::{Document, Staged};
use source::{Handoff, Loaded, Opened, WriteFailure};
use std::fmt;
use std::path::PathBuf;

pub const RESET_CAPTION: &str = "Removes your bind so the game's default applies.";
pub const UNBIND_CAPTION: &str =
    "Not available yet. How the game stores \"no bind\" has not been confirmed.";
pub const ADD_CAPTION: &str = "For actions that have no bind in this file yet. Use the action's name as the game writes it, for example v_toggle_mining_mode.";
pub const CAPTURE_PROMPT: &str = "Press a key or a mouse button";
pub const UNKNOWN_KEY: &str = "This key has no known name. Type it below.";
pub const CHANGED_AGAIN: &str =
    "The file changed on disk. Your edits were kept. Check them and apply again.";
pub const EMPTY_TITLE: &str = "No keybinds yet";
pub const EMPTY_BODY: &str = "Change any bind in the game once, or export a profile from Options, Keybindings, Control Profiles. It shows up here after that.";
pub const CLOSE_TO_APPLY: &str =
    "Close Star Citizen before applying changes to your current binds.";
const ROLLBACK_FAILED: &str =
    "Some files may have changed. The snapshot on the Backups screen holds the originals.";
const KEYBOARD_ONLY: &str = "Press a key. Mouse buttons go in the Mouse column.";
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
    model: Option<Model>,
    lines: Vec<Line>,
    map: Option<String>,
    search: String,
    conflicts_only: bool,
    panel: Panel,
    list: VirtualListState,
    raw_list: VirtualListState,
    saving: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(PathBuf, Loaded),
    Listed(PathBuf, Vec<SourceChoice>),
    PickSource(SourceChoice),
    SwitchSource(Source),
    Opened(PathBuf, Source, Opened),
    PickMap(Option<String>),
    Search(String),
    ToggleConflicts,
    Scrolled(f32),
    RawScrolled(f32),
    Select(CellRef),
    ClosePanel,
    Input(iced::Event),
    CaptureHover(bool),
    Listen,
    Typed(String),
    QuickPick(String),
    Set,
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
    LayoutSaved(String, Result<SnapshotId, WriteFailure>),
    Applied(Handoff, Result<SnapshotId, WriteFailure>),
}

impl State {
    pub fn new() -> Self {
        Self {
            channel: None,
            sources: Vec::new(),
            source: None,
            doc: Doc::Loading,
            away_pending: 0,
            model: None,
            lines: Vec::new(),
            map: None,
            search: String::new(),
            conflicts_only: false,
            panel: Panel::Closed,
            list: VirtualListState::default(),
            raw_list: VirtualListState::default(),
            saving: false,
        }
    }

    pub fn reload(&mut self, ctx: &Context) -> Task<Message> {
        let Some(channel) = ctx.channel.clone() else {
            *self = Self::new();
            return Task::none();
        };
        if self.pending() > 0 {
            return Task::none();
        }
        if self.channel.as_ref() != Some(&channel.dir) {
            *self = Self::new();
            self.channel = Some(channel.dir.clone());
        }
        let wanted = self.source.clone();
        let dir = channel.dir.clone();
        work::background(
            move || source::load(&channel, wanted),
            move |loaded| Message::Loaded(dir, loaded),
        )
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
            Message::Scrolled(offset) => {
                self.list.offset = offset;
                Action::none()
            }
            Message::RawScrolled(offset) => {
                self.raw_list.offset = offset;
                Action::none()
            }
            Message::Select(cell) => {
                self.select(cell);
                Action::none()
            }
            Message::ClosePanel => {
                self.panel = Panel::Closed;
                Action::none()
            }
            Message::Input(event) => self.input(&event),
            Message::Listen => {
                if let Panel::Edit(edit) = &mut self.panel {
                    edit.listening = captures(edit.cell.slot.kind);
                    edit.note = None;
                    edit.capture = Capture::default();
                }
                Action::none()
            }
            Message::Set => self.set_typed(),
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
            Message::LayoutSaved(name, result) => self.layout_saved(name, result, ctx),
            Message::Applied(handoff, result) => self.applied(handoff, result),
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

    /// Rebuilds the table after the document changed.
    fn refresh(&mut self) {
        if let Doc::Ready { staged, .. } = &self.doc {
            self.model = Some(Model::build(staged.current(), staged.base()));
        } else if !matches!(self.doc, Doc::Away) {
            self.model = None;
        }
        if let (Some(model), Some(map)) = (&self.model, &self.map)
            && !model.maps.iter().any(|entry| &entry.name == map)
        {
            self.map = None;
        }
        let keep = match (&self.panel, &self.model) {
            (Panel::Edit(edit), Some(model)) => model.column(edit.cell.slot).is_some(),
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
            Some(model) => model.lines(self.map.as_deref(), &self.search, self.conflicts_only),
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

    fn select(&mut self, cell: CellRef) {
        if !self.writable() {
            return;
        }
        let Some(current) = self.model.as_ref().and_then(|model| model.cell(&cell)) else {
            return;
        };
        let typed = current.rest().unwrap_or_default().to_string();
        let listening = captures(cell.slot.kind);
        self.panel = Panel::Edit(Edit {
            cell,
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
        match edit.capture.event(event, edit.hovered) {
            Outcome::Waiting => Action::none(),
            Outcome::Cancelled => {
                edit.listening = false;
                edit.note = None;
                Action::none()
            }
            Outcome::Unknown => {
                edit.note = Some(UNKNOWN_KEY);
                Action::none()
            }
            Outcome::Done(rest) => match judge(edit.cell.slot, &rest) {
                Typed::Valid(input) => {
                    edit.listening = false;
                    edit.note = None;
                    edit.typed = rest;
                    let op = bind_op(&edit.cell, input);
                    self.stage(op)
                }
                Typed::Empty | Typed::Invalid(_) => {
                    edit.note = Some(if edit.cell.slot.kind == DeviceKind::Mouse {
                        MOUSE_ONLY
                    } else {
                        KEYBOARD_ONLY
                    });
                    Action::none()
                }
            },
        }
    }

    fn set_typed(&mut self) -> Action<Message> {
        let Panel::Edit(edit) = &mut self.panel else {
            return Action::none();
        };
        let Typed::Valid(input) = judge(edit.cell.slot, &edit.typed) else {
            return Action::none();
        };
        edit.listening = false;
        let op = bind_op(&edit.cell, input);
        self.stage(op)
    }

    fn reset(&mut self) -> Action<Message> {
        let Panel::Edit(edit) = &self.panel else {
            return Action::none();
        };
        let bound = self
            .model
            .as_ref()
            .and_then(|model| model.cell(&edit.cell))
            .is_some_and(|cell| cell.current.is_some());
        if !bound {
            return Action::none();
        }
        let op = Op::Reset {
            map: edit.cell.map.clone(),
            action: edit.cell.action.clone(),
            slot: edit.cell.slot,
        };
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

    fn slot_choices(&self) -> Vec<SlotChoice> {
        let mut slots: Vec<Slot> = self
            .model
            .as_ref()
            .map(|model| model.columns.clone())
            .unwrap_or_default();
        for kind in [DeviceKind::Keyboard, DeviceKind::Mouse] {
            if !slots.iter().any(|slot| slot.kind == kind) {
                slots.push(Slot { kind, instance: 1 });
            }
        }
        slots.sort();
        slots
            .into_iter()
            .map(|slot| self.slot_choice(slot))
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
        if !self.writable() {
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
            slot,
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
        let (Some(scope), Some(channel), Some(staged)) =
            (ctx.scope(), ctx.channel.clone(), self.staged())
        else {
            return Action::none();
        };
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
            move |result| Message::LayoutSaved(name, result),
        ))
    }

    fn can_save_layout(&self) -> bool {
        self.writable() && self.source.as_ref().is_some_and(Source::is_layout)
    }

    fn layout_saved(
        &mut self,
        name: String,
        result: Result<SnapshotId, WriteFailure>,
        ctx: &Context,
    ) -> Action<Message> {
        self.saving = false;
        match result {
            Ok(snapshot) => {
                self.panel = Panel::Closed;
                let toast = Toast::success(format!(
                    "Saved layout \"{name}\". Load it in game from Options, Keybindings, Control Profiles."
                ))
                .with_undo(snapshot);
                Action::task(self.relist(ctx)).with_toast(toast)
            }
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
        handoff: Handoff,
        result: Result<SnapshotId, WriteFailure>,
    ) -> Action<Message> {
        let applied = self.away_pending;
        self.away_pending = 0;
        if let Some(staged) = handoff.take() {
            self.doc = Doc::Ready {
                staged: Box::new(staged),
                writable: true,
            };
        }
        self.refresh();
        let toast = match result {
            Ok(snapshot) => Toast::success(applied_text(applied)).with_undo(snapshot),
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
        Action::task(work::background(
            move || source::apply(*staged, writer, scope, channel),
            |(handoff, result)| Message::Applied(handoff, result),
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
    match judge(slot, &form.input) {
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
        WriteFailure::ChangedAgain | WriteFailure::Exists => Toast::info(CHANGED_AGAIN),
        WriteFailure::NoLongerApply(count) => Toast::info(format!(
            "The file changed on disk and {count} of your edits no longer fit it. Check the rest and apply again."
        )),
        WriteFailure::Failed(reason) => Toast::error(format!("{what}: {reason}")),
    }
}
