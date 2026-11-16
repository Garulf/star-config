mod labels;
mod text_check;

use super::attr_edit::{self, AttrFile, Doc};
use super::edits::{self, Carry, Edits, Pending, Target, changed_keys};
use super::typed::{self, Kind};
use crate::context::Context;
use crate::screen::{Action, Origin, Screen, Toast, ToastKind};
use crate::theme::{self, ButtonKind, layout, size, space};
use crate::widgets::{
    OptionalRow, VirtualListState, Was, banner, button, empty_state, hint, muted, optional_row,
    page, page_fixed, rows, search_field, section, segmented, setting_row, text_field, toggle,
    virtual_list,
};
use crate::work;
use iced::widget::{Column, column, container, text_editor};
use iced::{Alignment, Element, Length, Task};
use sc_core::attributes::{self, Attributes};
use sc_core::backup::SnapshotInfo;
use sc_core::catalog::cvars::{self, Cvar, CvarKind};
use sc_core::error::Result;
use sc_core::install::Channel;
use sc_core::staging::{Document, Staged};
use sc_core::usercfg::{Op, UserCfg};
use std::collections::BTreeMap;
use std::path::PathBuf;

const SNAPSHOT_LABEL: &str = "Config changes";
const SUBTITLE: &str = "The settings files the game reads for this channel.";
const MISSING_CFG: &str = "This channel has no user.cfg yet. Applying a change creates it.";
const GAME_CAPTION: &str = "Every setting the game stores for this channel. Head tracking and comms have their own screen.";
const NOT_UTF8: &str = "This file has characters that are not UTF-8, so it can only be edited as settings, not as text.";
const OTHER_LINES: &str = "Other lines in this file";
const READ_ONLY: &str = "Close Star Citizen to apply these changes. Your edits were kept.";
const TEXT_KEY: &str = "\u{0}text";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    UserCfg,
    Game,
}

enum CfgFile {
    Loading,
    Ready(Edits<UserCfg>),
    Unreadable(String),
}

impl CfgFile {
    fn edits(&self) -> Option<&Edits<UserCfg>> {
        match self {
            CfgFile::Ready(edits) => Some(edits),
            _ => None,
        }
    }

    fn edits_mut(&mut self) -> Option<&mut Edits<UserCfg>> {
        match self {
            CfgFile::Ready(edits) => Some(edits),
            _ => None,
        }
    }
}

struct TextView {
    content: text_editor::Content,
    editable: bool,
    opened_with: String,
    error: Option<String>,
}

impl TextView {
    fn of(doc: &UserCfg) -> Self {
        match doc.text() {
            Some(text) => Self {
                content: text_editor::Content::with_text(&text),
                editable: true,
                opened_with: text,
                error: None,
            },
            None => {
                let lossy = String::from_utf8_lossy(&doc.to_bytes()).into_owned();
                Self {
                    content: text_editor::Content::with_text(lossy.trim_start_matches('\u{feff}')),
                    editable: false,
                    opened_with: String::new(),
                    error: None,
                }
            }
        }
    }

    fn edited_text(&self) -> Option<String> {
        let text = self.content.text();
        (self.editable && text != self.opened_with).then_some(text)
    }
}

pub struct Loaded {
    channel: PathBuf,
    cfg: CfgFile,
    attrs: AttrFile,
}

pub struct Applied {
    seq: u64,
    origin: Origin,
    cfg: Option<Staged<UserCfg>>,
    attrs: Option<Staged<Attributes>>,
    result: Result<Option<SnapshotInfo>>,
    count: usize,
}

pub struct State {
    seq: u64,
    channel: Option<PathBuf>,
    tab: Tab,
    cfg: CfgFile,
    attrs: AttrFile,
    other_lines: Vec<String>,
    applying: bool,
    cfg_query: String,
    attr_query: String,
    text: Option<TextView>,
    new_name: String,
    new_value: String,
    list: VirtualListState,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(u64, Carry<Loaded>),
    Tab(Tab),
    Search(String),
    SetCvar(String, String),
    RemoveCvar(String),
    NewName(String),
    NewValue(String),
    AddLine,
    TextView(bool),
    TextEdited(text_editor::Action),
    SetAttr(String, String),
    Scrolled(f32),
    Applied(Carry<Applied>),
}

fn load(channel: &Channel) -> Loaded {
    let cfg = match Staged::<UserCfg>::open(&channel.user_cfg()) {
        Ok(staged) => CfgFile::Ready(Edits::new(staged, count_cfg)),
        Err(err) => CfgFile::Unreadable(format!("user.cfg cannot be read: {err}")),
    };
    Loaded {
        channel: channel.dir.clone(),
        cfg,
        attrs: attr_edit::load(&channel.attributes_xml()),
    }
}

fn count_cfg(base: &UserCfg, current: &UserCfg) -> usize {
    let map = |doc: &UserCfg| -> BTreeMap<String, String> {
        doc.entries()
            .into_iter()
            .map(|entry| (entry.key.to_lowercase(), entry.value))
            .collect()
    };
    match changed_keys(&map(base), &map(current)) {
        0 if base.to_bytes() != current.to_bytes() => 1,
        changed => changed,
    }
}

/// Names of lines that are not in the catalogue, in file order, then lines the staged edits
/// removed. One name per key, ignoring case.
fn other_lines(edits: &Edits<UserCfg>) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let current = edits.current().entries();
    let base = edits.base().entries();
    for entry in current.iter().chain(&base) {
        let known = cvars::find(&entry.key).is_some();
        let listed = names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&entry.key));
        if !known && !listed {
            names.push(entry.key.clone());
        }
    }
    names
}

fn cvar_groups() -> Vec<&'static str> {
    let mut groups: Vec<&'static str> = Vec::new();
    for cvar in cvars::all() {
        if !groups.contains(&cvar.category.as_str()) {
            groups.push(&cvar.category);
        }
    }
    groups
}

fn kind_of(cvar: &Cvar) -> Kind<'_> {
    match &cvar.kind {
        CvarKind::Bool => Kind::Bool,
        CvarKind::Int { min, max } => Kind::Range {
            min: *min as f64,
            max: *max as f64,
            step: 1.0,
        },
        CvarKind::Float { min, max } if *min > f64::MIN && *max < f64::MAX => Kind::Range {
            min: *min,
            max: *max,
            step: (max - min) / 100.0,
        },
        CvarKind::Float { .. } => Kind::Number,
        CvarKind::Choice(options) => Kind::Choice(options),
        CvarKind::Text if numeric_default(cvar) => Kind::Number,
        CvarKind::Text => Kind::Text,
    }
}

fn numeric_default(cvar: &Cvar) -> bool {
    cvar.default
        .as_deref()
        .is_some_and(|default| default.trim().parse::<f64>().is_ok())
}

fn cvar_matches(cvar: &'static Cvar, query: &str) -> bool {
    query.is_empty()
        || labels::label(cvar).to_lowercase().contains(query)
        || cvar.name.to_lowercase().contains(query)
        || cvar.description.to_lowercase().contains(query)
}

fn was(base: Option<&str>, current: Option<&str>) -> Option<Was> {
    (base != current).then(|| base.map_or(Was::NotSet, |value| Was::Value(value.to_string())))
}

fn stage_error(err: &sc_core::error::Error) -> Toast {
    Toast::error(format!("That change cannot be made: {err}."))
}

impl State {
    pub fn new() -> Self {
        Self {
            seq: 0,
            channel: None,
            tab: Tab::UserCfg,
            cfg: CfgFile::Loading,
            attrs: AttrFile::Loading,
            other_lines: Vec::new(),
            applying: false,
            cfg_query: String::new(),
            attr_query: String::new(),
            text: None,
            new_name: String::new(),
            new_value: String::new(),
            list: VirtualListState::default(),
        }
    }

    pub fn reload(&mut self, ctx: &Context) -> Task<Message> {
        self.seq += 1;
        let channel = ctx.channel.clone();
        let same_channel = channel.as_ref().map(|c| &c.dir) == self.channel.as_ref();
        if !same_channel || self.cfg_away() || self.attrs.is_away() {
            self.cfg = CfgFile::Loading;
            self.attrs = AttrFile::Loading;
            self.other_lines.clear();
            self.text = None;
            self.applying = false;
        }
        let Some(channel) = channel else {
            self.channel = None;
            return Task::none();
        };
        let seq = self.seq;
        work::background(
            move || Carry::new(load(&channel)),
            move |loaded| Message::Loaded(seq, loaded),
        )
    }

    fn cfg_away(&self) -> bool {
        self.cfg.edits().is_some_and(Edits::is_away)
    }

    pub fn update(&mut self, message: Message, _ctx: &Context) -> Action<Message> {
        match message {
            Message::Loaded(seq, loaded) => {
                if seq == self.seq
                    && self.pending() == 0
                    && let Some(loaded) = loaded.take()
                {
                    self.channel = Some(loaded.channel);
                    self.cfg = loaded.cfg;
                    self.attrs = loaded.attrs;
                    self.cfg_changed();
                    self.refresh_text();
                }
                Action::none()
            }
            Message::Tab(tab) => {
                self.tab = tab;
                Action::none()
            }
            Message::Search(query) => {
                match self.tab {
                    Tab::UserCfg => self.cfg_query = query,
                    Tab::Game => {
                        self.attr_query = query;
                        self.list.offset = 0.0;
                    }
                }
                Action::none()
            }
            Message::SetCvar(key, value) => {
                let merge = key.clone();
                self.stage_cfg(Op::Set { key, value }, Some(merge.as_str()))
                    .err()
                    .map_or_else(Action::none, Action::toast)
            }
            Message::RemoveCvar(key) => self
                .stage_cfg(Op::Remove { key }, None)
                .err()
                .map_or_else(Action::none, Action::toast),
            Message::NewName(name) => {
                self.new_name = name;
                Action::none()
            }
            Message::NewValue(value) => {
                self.new_value = value;
                Action::none()
            }
            Message::AddLine => self.add_line(),
            Message::TextView(on) => {
                self.toggle_text(on);
                Action::none()
            }
            Message::TextEdited(action) => self.edit_text(action),
            Message::SetAttr(name, value) => {
                let Some(edits) = self.attrs.edits_mut() else {
                    return Action::none();
                };
                let merge = name.clone();
                match edits.stage(attributes::Op::Set { name, value }, Some(merge.as_str())) {
                    Ok(()) => Action::none(),
                    Err(err) => Action::toast(stage_error(&err)),
                }
            }
            Message::Scrolled(offset) => {
                self.list.offset = offset;
                Action::none()
            }
            Message::Applied(applied) => self.finish_apply(applied),
        }
    }

    fn stage_cfg(&mut self, op: Op, merge: Option<&str>) -> std::result::Result<(), Toast> {
        let edits = self
            .cfg
            .edits_mut()
            .ok_or_else(|| Toast::info("user.cfg is still loading"))?;
        edits.stage(op, merge).map_err(|err| stage_error(&err))?;
        self.cfg_changed();
        Ok(())
    }

    fn add_line(&mut self) -> Action<Message> {
        let key = self.new_name.trim().to_string();
        if key.is_empty() {
            return Action::none();
        }
        let value = self.new_value.trim().to_string();
        match self.stage_cfg(Op::Set { key, value }, None) {
            Ok(()) => {
                self.new_name.clear();
                self.new_value.clear();
                Action::none()
            }
            Err(_) => Action::toast(Toast::error(
                "That name cannot be used. A name cannot be empty or contain spaces, quotes, = or ;.",
            )),
        }
    }

    fn cfg_changed(&mut self) {
        self.other_lines = self.cfg.edits().map(other_lines).unwrap_or_default();
    }

    fn refresh_text(&mut self) {
        if let (Some(view), Some(edits)) = (&mut self.text, self.cfg.edits()) {
            *view = TextView::of(edits.current());
        }
    }

    fn toggle_text(&mut self, on: bool) {
        if on {
            self.text = self.cfg.edits().map(|edits| TextView::of(edits.current()));
            return;
        }
        let Some(view) = &mut self.text else {
            return;
        };
        if let Some(text) = view.edited_text()
            && let Err(problem) = text_check::check(&text)
        {
            view.error = Some(problem);
            return;
        }
        self.text = None;
    }

    fn edit_text(&mut self, action: text_editor::Action) -> Action<Message> {
        let Some(view) = &mut self.text else {
            return Action::none();
        };
        let edit = action.is_edit();
        if edit && !view.editable {
            return Action::none();
        }
        view.content.perform(action);
        if !edit {
            return Action::none();
        }
        view.error = None;
        let text = view.content.text();
        match self.stage_cfg(Op::ReplaceText(text), Some(TEXT_KEY)) {
            Ok(()) => Action::none(),
            Err(toast) => Action::toast(toast),
        }
    }

    fn text_problem(&self) -> Option<String> {
        let text = self.text.as_ref()?.edited_text()?;
        let problem = text_check::check(&text).err()?;
        Some(format!(
            "{problem} Fix it in the text view before applying."
        ))
    }

    pub fn pending(&self) -> usize {
        self.cfg.edits().map_or(0, Edits::pending) + self.attrs.pending()
    }

    pub fn apply(&mut self, ctx: &Context) -> Action<Message> {
        if self.applying || self.pending() == 0 {
            return Action::none();
        }
        if ctx.read_only() {
            return Action::toast(Toast::info(READ_ONLY));
        }
        if let Some(problem) = self.text_problem() {
            return Action::toast(Toast::info(problem));
        }
        let (Some(scope), Some(channel)) = (ctx.scope(), ctx.channel.clone()) else {
            return Action::none();
        };
        let count = self.pending();
        let cfg = self
            .cfg
            .edits_mut()
            .filter(|edits| edits.pending() > 0)
            .and_then(Edits::lend);
        let attrs = self
            .attrs
            .edits_mut()
            .filter(|edits| edits.pending() > 0)
            .and_then(Edits::lend);
        self.applying = true;
        let target = Target {
            writer: ctx.writer(),
            scope,
            channel,
            label: SNAPSHOT_LABEL,
        };
        let seq = self.seq;
        Action::task(work::background(
            move || {
                let (mut cfg, mut attrs) = (cfg, attrs);
                let result = {
                    let mut files: Vec<&mut dyn Pending> = Vec::new();
                    if let Some(staged) = cfg.as_mut() {
                        files.push(staged);
                    }
                    if let Some(staged) = attrs.as_mut() {
                        files.push(staged);
                    }
                    edits::write(&target, &mut files)
                };
                Carry::new(Applied {
                    seq,
                    origin: target.origin(),
                    cfg,
                    attrs,
                    result,
                    count,
                })
            },
            Message::Applied,
        ))
    }

    fn finish_apply(&mut self, applied: Carry<Applied>) -> Action<Message> {
        let Some(applied) = applied.take() else {
            return Action::none();
        };
        if applied.seq == self.seq {
            self.applying = false;
            if let (Some(staged), Some(edits)) = (applied.cfg, self.cfg.edits_mut()) {
                edits.put_back(staged);
            }
            if let (Some(staged), Some(edits)) = (applied.attrs, self.attrs.edits_mut()) {
                edits.put_back(staged);
            }
            self.cfg_changed();
            self.refresh_text();
        }
        edits::toast_for(&applied.result, applied.count, &applied.origin)
            .map_or_else(Action::none, Action::toast)
    }

    pub fn discard(&mut self) {
        if let Some(edits) = self.cfg.edits_mut() {
            edits.discard();
        }
        if let Some(edits) = self.attrs.edits_mut() {
            edits.discard();
        }
        self.new_name.clear();
        self.new_value.clear();
        self.cfg_changed();
        self.refresh_text();
    }

    pub fn undo(&mut self, _ctx: &Context) -> Action<Message> {
        match self.tab {
            Tab::UserCfg => {
                if self.cfg.edits_mut().is_some_and(Edits::undo) {
                    self.cfg_changed();
                    self.refresh_text();
                }
            }
            Tab::Game => {
                if let Some(edits) = self.attrs.edits_mut() {
                    edits.undo();
                }
            }
        }
        Action::none()
    }

    pub fn view<'a>(&'a self, _ctx: &'a Context) -> Element<'a, Message> {
        match (self.tab, &self.text) {
            (Tab::UserCfg, Some(view)) => self.text_page(view),
            (Tab::UserCfg, None) => self.cfg_page(),
            (Tab::Game, _) => self.game_page(),
        }
    }

    fn tabs(&self) -> Element<'_, Message> {
        segmented(vec![
            (
                "user.cfg",
                self.tab == Tab::UserCfg,
                Message::Tab(Tab::UserCfg),
            ),
            (
                "Game settings",
                self.tab == Tab::Game,
                Message::Tab(Tab::Game),
            ),
        ])
    }

    fn text_toggle(&self, on: bool) -> Option<Element<'_, Message>> {
        self.cfg.edits()?;
        Some(
            toggle(on, Message::TextView)
                .label("View as text")
                .text_size(size::BODY)
                .spacing(space::SM)
                .into(),
        )
    }

    fn cfg_page(&self) -> Element<'_, Message> {
        let mut trailing = Vec::new();
        if self.cfg.edits().is_some() {
            trailing.push(search_field(
                &self.cfg_query,
                "Search settings",
                Message::Search,
            ));
        }
        trailing.extend(self.text_toggle(false));
        let body: Element<'_, Message> = match &self.cfg {
            CfgFile::Loading => muted("Loading user.cfg").into(),
            CfgFile::Unreadable(message) => banner(ToastKind::Error, message, None),
            CfgFile::Ready(edits) => self.cfg_body(edits),
        };
        page(
            Screen::Config.label(),
            Some(SUBTITLE),
            trailing,
            column![self.tabs(), body].spacing(space::XL),
        )
    }

    fn cfg_body<'a>(&'a self, edits: &'a Edits<UserCfg>) -> Element<'a, Message> {
        let query = self.cfg_query.trim().to_lowercase();
        let editable = !edits.is_away();
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        if edits.missing() {
            parts.push(banner(ToastKind::Info, MISSING_CFG, None));
        }
        for group in cvar_groups() {
            let items: Vec<Element<'a, Message>> = cvars::all()
                .iter()
                .filter(|cvar| cvar.category == group && cvar_matches(cvar, &query))
                .map(|cvar| cvar_row(edits, cvar, editable))
                .collect();
            if !items.is_empty() {
                parts.push(section(group, rows(items)));
            }
        }
        let mut others: Vec<Element<'a, Message>> = self
            .other_lines
            .iter()
            .filter(|name| query.is_empty() || name.to_lowercase().contains(&query))
            .map(|name| other_row(edits, name, editable))
            .collect();
        if query.is_empty() {
            others.push(self.add_row(editable));
        }
        if !others.is_empty() {
            parts.push(section(OTHER_LINES, rows(others)));
        }
        let only_banner = usize::from(edits.missing());
        if parts.len() == only_banner {
            parts.push(empty_state(
                "Nothing matches",
                "No setting has that name or description. Try fewer letters.",
                None,
            ));
        }
        Column::with_children(parts).spacing(space::XL).into()
    }

    fn add_row(&self, editable: bool) -> Element<'_, Message> {
        let ready = editable && !self.new_name.trim().is_empty();
        let add = ready.then_some(Message::AddLine);
        let mut value =
            text_field("Value", &self.new_value, Message::NewValue).width(layout::CONTROL_WIDTH);
        if let Some(add) = add.clone() {
            value = value.on_submit(add);
        }
        setting_row(
            "Add a line",
            Some("A name that is already in the file changes that line."),
            false,
            iced::widget::row![
                text_field("Name", &self.new_name, Message::NewName).width(layout::CONTROL_WIDTH),
                value,
                button("Add", ButtonKind::Secondary, add),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        )
    }

    fn text_page<'a>(&'a self, view: &'a TextView) -> Element<'a, Message> {
        let mut editor = text_editor(&view.content)
            .font(theme::FONT_MONO)
            .size(size::BODY)
            .padding(space::MD)
            .height(Length::Fill)
            .style(theme::editor);
        if view.editable {
            editor = editor.on_action(Message::TextEdited);
        }
        let mut body = Column::new()
            .push(self.tabs())
            .spacing(space::LG)
            .height(Length::Fill);
        if let Some(error) = &view.error {
            body = body.push(banner(ToastKind::Error, error, None));
        }
        if !view.editable {
            body = body.push(hint(NOT_UTF8));
        }
        body = body.push(editor);
        page_fixed(
            Screen::Config.label(),
            Some(SUBTITLE),
            self.text_toggle(true).into_iter().collect(),
            body,
        )
    }

    fn game_page(&self) -> Element<'_, Message> {
        let doc = self.attrs.doc();
        let trailing = match doc {
            Some(_) => vec![search_field(
                &self.attr_query,
                "Search game settings",
                Message::Search,
            )],
            None => Vec::new(),
        };
        let body: Element<'_, Message> = match (&self.attrs, doc) {
            (_, Some(doc)) => self.game_list(doc),
            (AttrFile::Missing, None) => {
                empty_state(attr_edit::MISSING_TITLE, attr_edit::MISSING_BODY, None)
            }
            (AttrFile::Broken { summary, text }, None) => attr_edit::broken(summary, text),
            (AttrFile::Unreadable(message), None) => banner(ToastKind::Error, message, None),
            _ => muted("Loading attributes.xml").into(),
        };
        page_fixed(
            Screen::Config.label(),
            Some(SUBTITLE),
            trailing,
            column![self.tabs(), hint(GAME_CAPTION), body]
                .spacing(space::LG)
                .height(Length::Fill),
        )
    }

    fn game_list<'a>(&'a self, doc: Doc<'a>) -> Element<'a, Message> {
        let query = self.attr_query.trim().to_lowercase();
        let current = doc.current;
        let shown: Vec<usize> = current
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                query.is_empty()
                    || entry.name.to_lowercase().contains(&query)
                    || entry.value.to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect();
        let count = attr_edit::count_line(shown.len(), current.len());
        let mut body = Column::new().spacing(space::MD).height(Length::Fill);
        if !doc.editable && !self.attrs.is_away() {
            body = body.push(banner(ToastKind::Warning, attr_edit::NOT_RECOGNISED, None));
        }
        body = body.push(muted(count));
        if shown.is_empty() {
            return body
                .push(empty_state(
                    "Nothing matches",
                    "No game setting has that name or value.",
                    None,
                ))
                .into();
        }
        let editable = doc.editable;
        let list = virtual_list(
            &self.list,
            shown.len(),
            layout::EDIT_ROW_HEIGHT,
            Message::Scrolled,
            move |row| {
                let entry = &current[shown[row]];
                let name = entry.name.clone();
                attr_edit::value_row(
                    &entry.name,
                    Some(&entry.value),
                    doc.was(&entry.name),
                    editable.then_some(move |value: String| Message::SetAttr(name.clone(), value)),
                )
            },
        );
        body.push(
            container(list)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(theme::card)
                .clip(true),
        )
        .into()
    }
}

fn cvar_row<'a>(
    edits: &'a Edits<UserCfg>,
    cvar: &'static Cvar,
    editable: bool,
) -> Element<'a, Message> {
    let current = edits.current().get(&cvar.name);
    let base = edits.base().get(&cvar.name);
    let name = cvar.name.clone();
    let on_set = editable.then_some(move |value: String| Message::SetCvar(name.clone(), value));
    let control = typed::control(kind_of(cvar), current, cvar.default.as_deref(), on_set);
    optional_row(
        OptionalRow {
            label: labels::label(cvar),
            name: Some(&cvar.name),
            hint: labels::description(cvar),
            set: current.is_some(),
            was: was(base, current),
            remove: editable.then(|| Message::RemoveCvar(cvar.name.clone())),
        },
        control,
    )
}

fn other_row<'a>(edits: &'a Edits<UserCfg>, name: &'a str, editable: bool) -> Element<'a, Message> {
    let current = edits.current().get(name);
    let base = edits.base().get(name);
    let key = name.to_string();
    let on_set = editable.then_some(move |value: String| Message::SetCvar(key.clone(), value));
    optional_row(
        OptionalRow {
            label: name,
            name: None,
            hint: None,
            set: current.is_some(),
            was: was(base, current),
            remove: editable.then(|| Message::RemoveCvar(name.to_string())),
        },
        typed::control(Kind::Text, current, base, on_set),
    )
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> UserCfg {
        UserCfg::parse(std::path::Path::new("user.cfg"), text.as_bytes()).unwrap()
    }

    #[test]
    fn count_cfg_counts_settings_and_falls_back_to_one_for_text_only_changes() {
        let base = parse("; mine\nr_DisplayInfo = 1\ncl_fov = 90\n");
        assert_eq!(count_cfg(&base, &base), 0);
        assert_eq!(count_cfg(&base, &parse("; mine\nR_DISPLAYINFO = 2\n")), 2);
        assert_eq!(
            count_cfg(&base, &parse("; changed\nr_DisplayInfo = 1\ncl_fov = 90\n")),
            1
        );
    }

    #[test]
    fn cvar_groups_keep_catalogue_order() {
        let groups = cvar_groups();
        assert_eq!(groups.first(), Some(&"Graphics"));
        assert!(groups.contains(&"Head tracking"));
        let unique: std::collections::HashSet<_> = groups.iter().collect();
        assert_eq!(unique.len(), groups.len());
    }
}
