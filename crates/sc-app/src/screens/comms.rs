use super::attr_edit::{self, AttrFile, Doc};
use super::edits::{self, Carry, Edits, Pending, Target, Written};
use super::typed::{self, Kind};
use super::words::{GAME_RUNNING, words};
use crate::context::Context;
use crate::game_data::Loaded as GameLoaded;
use crate::screen::{Action, Origin, Screen, Toast, ToastKind};
use crate::theme::{ButtonKind, layout, space};
use crate::widgets::{
    OptionalRow, Was, banner, button, empty_state, hint, icon_button, muted, optional_row, page,
    page_fixed, rows, section, setting_row, text_field,
};
use crate::{icons, work};
use iced::widget::{Column, row};
use iced::{Alignment, Element, Task};
use sc_core::attributes::{Attributes, Op};
use sc_core::catalog::layout as options;
use sc_core::catalog::layout::{Entry, Kind as LayoutKind};
use sc_core::defaults::GameData;
use sc_core::error::Result;
use sc_core::staging::Staged;
use std::borrow::Cow;
use std::path::PathBuf;

const SNAPSHOT_LABEL: &str = "Head tracking and comms changes";
const SUBTITLE: &str = "These are the game's own settings. star-config does not track your head. It sets what the game does with a tracker you already use.";
const SOURCE: &str = "HeadtrackingSource";
const SOURCE_HINT: &str = "Which tracker the game uses. What each number means has not been confirmed, so this is the number stored in the file.";
const COMMS_KEYWORDS: [&str; 8] = [
    "headtracking",
    "hmd",
    "voip",
    "foip",
    "faceware",
    "comms",
    "voice",
    "mic",
];
const OTHER_SETTINGS: &str = "Other settings in this file";
const ELSEWHERE: &str = "This name has no head tracking or comms word in it, so it will be listed on the Config screen under Game settings.";

pub struct Loaded {
    channel: PathBuf,
    attrs: AttrFile,
}

pub struct Applied {
    seq: u64,
    origin: Origin,
    attrs: Option<Staged<Attributes>>,
    result: Result<Written>,
    count: usize,
}

#[derive(Default)]
struct NewSetting {
    name: String,
    value: String,
}

struct Group {
    title: String,
    rows: Vec<Row>,
}

struct Row {
    entry: &'static Entry,
    label: String,
    default: Option<String>,
    choices: Vec<(String, String)>,
}

pub struct State {
    seq: u64,
    channel: Option<PathBuf>,
    attrs: AttrFile,
    groups: Vec<Group>,
    applying: bool,
    adding: Option<NewSetting>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(u64, Carry<Loaded>),
    GameLoaded(u64, GameLoaded),
    Set(String, String),
    Remove(String),
    OpenAdd,
    AddName(String),
    AddValue(String),
    Add,
    CancelAdd,
    Applied(Carry<Applied>),
}

fn kind_of(row: &Row) -> Kind<'_> {
    match &row.entry.kind {
        LayoutKind::Toggle => Kind::Bool,
        LayoutKind::Slider { min, max, step } => Kind::Range {
            min: *min,
            max: *max,
            step: *step,
        },
        LayoutKind::Number => Kind::Number,
        LayoutKind::Text => Kind::Text,
        LayoutKind::Choice(_) => Kind::Choice(Cow::Borrowed(&row.choices)),
    }
}

fn hint_of(entry: &'static Entry) -> Option<&'static str> {
    if entry.name == SOURCE {
        Some(SOURCE_HINT)
    } else {
        entry.note.as_deref()
    }
}

/// Head tracking and comms entries of the layout, by section, with the game's own wording
/// when the build's strings are known and our fallback wording when not.
fn groups(game: Option<&GameData>) -> Vec<Group> {
    let sections = options::layout().tabs.iter().flat_map(|tab| &tab.sections);
    sections
        .filter_map(|section| {
            let rows: Vec<Row> = section
                .entries
                .iter()
                .filter(|entry| is_comms_related(&entry.name))
                .map(|entry| row_of(entry, game))
                .collect();
            let title = match game {
                Some(game) => game.label(section.key.as_deref(), &section.fallback),
                None => section.fallback.clone(),
            };
            (!rows.is_empty()).then_some(Group { title, rows })
        })
        .collect()
}

fn row_of(entry: &'static Entry, game: Option<&GameData>) -> Row {
    let fallback = entry.fallback.as_deref().unwrap_or(&entry.name);
    let Some(game) = game else {
        return Row {
            entry,
            label: fallback.to_string(),
            default: None,
            choices: Vec::new(),
        };
    };
    let choices = match &entry.kind {
        LayoutKind::Choice(choices) => choices
            .iter()
            .map(|(value, key)| (value.clone(), game.label(Some(key), value)))
            .collect(),
        _ => Vec::new(),
    };
    Row {
        entry,
        label: game.label(entry.label_key.as_deref(), fallback),
        default: game.setting(&entry.name).map(|s| s.default.clone()),
        choices,
    }
}

pub fn is_comms_related(name: &str) -> bool {
    let name = name.to_lowercase();
    COMMS_KEYWORDS.iter().any(|keyword| name.contains(keyword))
}

fn was(base: Option<&str>, current: Option<&str>) -> Option<Was> {
    (base != current).then(|| base.map_or(Was::NotSet, |value| Was::Value(value.to_string())))
}

fn is_other(name: &str) -> bool {
    options::entry(name).is_none() && is_comms_related(name)
}

impl State {
    pub fn new() -> Self {
        Self {
            seq: 0,
            channel: None,
            attrs: AttrFile::Loading,
            groups: groups(None),
            applying: false,
            adding: None,
        }
    }

    pub fn reload(&mut self, ctx: &Context) -> Task<Message> {
        self.seq += 1;
        let channel = ctx.channel.clone();
        let same_channel = channel.as_ref().map(|c| &c.dir) == self.channel.as_ref();
        if !same_channel || self.attrs.is_away() {
            self.attrs = AttrFile::Loading;
            self.applying = false;
        }
        if !same_channel {
            self.groups = groups(None);
        }
        let Some(channel) = channel else {
            self.channel = None;
            return Task::none();
        };
        let seq = self.seq;
        let (cached, read_game) = attr_edit::game_data(ctx);
        if cached.is_some() {
            self.groups = groups(attr_edit::game_of(cached.as_ref()));
        }
        Task::batch([
            work::background(
                move || {
                    Carry::new(Loaded {
                        channel: channel.dir.clone(),
                        attrs: attr_edit::load(&channel.attributes_xml()),
                    })
                },
                move |loaded| Message::Loaded(seq, loaded),
            ),
            read_game.map(move |loaded| Message::GameLoaded(seq, loaded)),
        ])
    }

    pub fn update(&mut self, message: Message, _ctx: &Context) -> Action<Message> {
        match message {
            Message::Loaded(seq, loaded) => {
                if seq == self.seq
                    && self.pending() == 0
                    && let Some(loaded) = loaded.take()
                {
                    self.channel = Some(loaded.channel);
                    self.attrs = loaded.attrs;
                }
                Action::none()
            }
            Message::GameLoaded(seq, loaded) => {
                if seq == self.seq {
                    self.groups = groups(attr_edit::game_of(Some(&loaded)));
                }
                Action::none()
            }
            Message::Set(name, value) => {
                let merge = name.clone();
                self.stage(Op::Set { name, value }, Some(merge.as_str()))
            }
            Message::Remove(name) => self.stage(Op::Remove { name }, None),
            Message::OpenAdd => {
                self.adding = Some(NewSetting::default());
                Action::none()
            }
            Message::AddName(name) => {
                if let Some(adding) = &mut self.adding {
                    adding.name = name;
                }
                Action::none()
            }
            Message::AddValue(value) => {
                if let Some(adding) = &mut self.adding {
                    adding.value = value;
                }
                Action::none()
            }
            Message::Add => self.add(),
            Message::CancelAdd => {
                self.adding = None;
                Action::none()
            }
            Message::Applied(applied) => self.finish_apply(applied),
        }
    }

    fn stage(&mut self, op: Op, merge: Option<&str>) -> Action<Message> {
        let Some(edits) = self.attrs.edits_mut() else {
            return Action::none();
        };
        match edits.stage(op, merge) {
            Ok(()) => Action::none(),
            Err(err) => Action::toast(Toast::error(format!(
                "That change cannot be made: {}.",
                words(&err)
            ))),
        }
    }

    fn add(&mut self) -> Action<Message> {
        let Some(adding) = &self.adding else {
            return Action::none();
        };
        let name = adding.name.trim().to_string();
        if name.is_empty() {
            return Action::none();
        }
        let value = adding.value.trim().to_string();
        let Some(edits) = self.attrs.edits_mut() else {
            return Action::none();
        };
        match edits.stage(Op::Set { name, value }, None) {
            Ok(()) => {
                self.adding = None;
                Action::none()
            }
            Err(_) => Action::toast(Toast::error(
                "That name cannot be used. A name cannot be empty or contain spaces, quotes, < > or &.",
            )),
        }
    }

    pub fn pending(&self) -> usize {
        self.attrs.pending()
    }

    pub fn apply(&mut self, ctx: &Context) -> Action<Message> {
        if self.applying || self.pending() == 0 {
            return Action::none();
        }
        if ctx.read_only() {
            return Action::toast(Toast::error(GAME_RUNNING));
        }
        let (Some(scope), Some(channel)) = (ctx.scope(), ctx.channel.clone()) else {
            return Action::none();
        };
        let count = self.pending();
        let attrs = self.attrs.edits_mut().and_then(Edits::lend);
        self.applying = true;
        let target = Target {
            writer: ctx.writer(),
            scope,
            channel,
            label: SNAPSHOT_LABEL,
            replay: true,
        };
        let seq = self.seq;
        Action::task(work::background(
            move || {
                let mut attrs = attrs;
                let result = match attrs.as_mut() {
                    Some(staged) => edits::write(&target, &mut [staged as &mut dyn Pending]),
                    None => Ok(Written {
                        info: None,
                        dropped: 0,
                    }),
                };
                Carry::new(Applied {
                    seq,
                    origin: target.origin(),
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
            if let (Some(staged), Some(edits)) = (applied.attrs, self.attrs.edits_mut()) {
                edits.put_back(staged);
            }
        }
        let toast = if applied.seq == self.seq {
            edits::toast_for(&applied.result, applied.count, &applied.origin)
        } else {
            edits::reloaded_toast(&applied.result, applied.count, &applied.origin)
        };
        toast.map_or_else(Action::none, Action::toast)
    }

    pub fn discard(&mut self) {
        if let Some(edits) = self.attrs.edits_mut() {
            edits.discard();
        }
        self.adding = None;
    }

    pub fn undo(&mut self, _ctx: &Context) -> Action<Message> {
        if let Some(edits) = self.attrs.edits_mut() {
            edits.undo();
        }
        Action::none()
    }

    pub fn view<'a>(&'a self, _ctx: &'a Context) -> Element<'a, Message> {
        let title = Screen::Comms.label();
        match (&self.attrs, self.attrs.doc()) {
            (_, Some(doc)) => page(title, Some(SUBTITLE), Vec::new(), self.body(doc)),
            (AttrFile::Missing, None) => page_fixed(
                title,
                Some(SUBTITLE),
                Vec::new(),
                empty_state(attr_edit::MISSING_TITLE, attr_edit::MISSING_BODY, None),
            ),
            (AttrFile::Broken { summary, text }, None) => page_fixed(
                title,
                Some(SUBTITLE),
                Vec::new(),
                attr_edit::broken(summary, text),
            ),
            (AttrFile::Unreadable(message), None) => page(
                title,
                Some(SUBTITLE),
                Vec::new(),
                banner(ToastKind::Error, message, None),
            ),
            _ => page(
                title,
                Some(SUBTITLE),
                Vec::new(),
                muted("Loading attributes.xml"),
            ),
        }
    }

    fn body<'a>(&'a self, doc: Doc<'a>) -> Element<'a, Message> {
        let mut body = Column::new().spacing(space::XL);
        if !doc.editable && !self.attrs.is_away() {
            body = body.push(banner(ToastKind::Warning, attr_edit::NOT_RECOGNISED, None));
        }
        for group in &self.groups {
            let items: Vec<Element<'a, Message>> =
                group.rows.iter().map(|row| known_row(doc, row)).collect();
            body = body.push(section(&group.title, rows(items)));
        }
        body.push(section(OTHER_SETTINGS, rows(self.other_rows(doc))))
            .into()
    }

    fn other_rows<'a>(&'a self, doc: Doc<'a>) -> Vec<Element<'a, Message>> {
        let mut items: Vec<Element<'a, Message>> = attr_edit::names_with_removed(&doc, is_other)
            .into_iter()
            .map(|name| other_row(doc, name))
            .collect();
        if items.is_empty() {
            items.push(setting_row(
                "No other head tracking or comms settings",
                Some("Settings whose names mention head tracking, voice or comms show up here."),
                false,
                iced::widget::Space::new(),
            ));
        }
        if doc.editable {
            items.push(self.add_row());
        }
        items
    }

    fn add_row(&self) -> Element<'_, Message> {
        let Some(adding) = &self.adding else {
            return iced::widget::container(icon_button(
                icons::PLUS,
                "Add a setting",
                ButtonKind::Ghost,
                Some(Message::OpenAdd),
            ))
            .padding(iced::Padding {
                top: space::SM,
                right: space::LG,
                bottom: space::SM,
                left: space::SM,
            })
            .into();
        };
        let name = adding.name.trim();
        let ready = !name.is_empty();
        let note = (ready && !is_comms_related(name) && options::entry(name).is_none())
            .then_some(ELSEWHERE);
        let mut value =
            text_field("Value", &adding.value, Message::AddValue).width(layout::CONTROL_WIDTH);
        if ready {
            value = value.on_submit(Message::Add);
        }
        let controls = row![
            text_field("Name", &adding.name, Message::AddName).width(layout::CONTROL_WIDTH),
            value,
            button("Add", ButtonKind::Secondary, ready.then_some(Message::Add)),
            button("Cancel", ButtonKind::Ghost, Some(Message::CancelAdd)),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);
        let mut column = Column::new().push(setting_row(
            "Add a setting",
            Some("A name that is already in the file changes that setting."),
            false,
            controls,
        ));
        if let Some(note) = note {
            column = column.push(iced::widget::container(hint(note)).padding(iced::Padding {
                top: 0.0,
                right: space::LG,
                bottom: space::MD,
                left: space::LG,
            }));
        }
        column.into()
    }
}

fn known_row<'a>(doc: Doc<'a>, row: &'a Row) -> Element<'a, Message> {
    let name = &row.entry.name;
    let current = doc.get(name);
    let base = doc.was(name);
    let key = name.clone();
    let on_set = doc
        .editable
        .then_some(move |value: String| Message::Set(key.clone(), value));
    let control = typed::control(kind_of(row), current, row.default.as_deref(), on_set);
    optional_row(
        OptionalRow {
            label: &row.label,
            name: Some(name),
            hint: hint_of(row.entry),
            set: current.is_some(),
            was: was(base, current),
            remove: doc.editable.then(|| Message::Remove(name.clone())),
        },
        control,
    )
}

fn other_row<'a>(doc: Doc<'a>, name: &'a str) -> Element<'a, Message> {
    let current = doc.get(name);
    let base = doc.was(name);
    let key = name.to_string();
    let on_set = doc
        .editable
        .then_some(move |value: String| Message::Set(key.clone(), value));
    optional_row(
        OptionalRow {
            label: name,
            name: None,
            hint: None,
            set: current.is_some(),
            was: was(base, current),
            remove: doc.editable.then(|| Message::Remove(name.to_string())),
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

    #[test]
    fn other_settings_are_comms_related_and_not_in_the_layout() {
        assert!(is_other("VoipPushToTalk"));
        assert!(!is_other("HeadtrackingToggle"));
        assert!(!is_other("FOV"));
    }

    #[test]
    fn source_gets_the_unconfirmed_note() {
        let source = options::entry(SOURCE).unwrap();
        assert_eq!(hint_of(source), Some(SOURCE_HINT));
        let row = row_of(source, None);
        assert!(matches!(kind_of(&row), Kind::Number));
        assert_eq!(row.label, "Source");
    }

    #[test]
    fn groups_follow_the_layout_sections_with_fallback_wording() {
        let groups = groups(None);
        let titles: Vec<&str> = groups.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Head tracking",
                "Head tracking: when it is off",
                "Head tracking: third person"
            ]
        );
        let count: usize = groups.iter().map(|g| g.rows.len()).sum();
        assert_eq!(count, 18);
        assert!(
            groups
                .iter()
                .flat_map(|g| &g.rows)
                .all(|r| r.default.is_none())
        );
    }

    #[test]
    fn is_comms_related_matches_keywords_case_insensitively() {
        for name in [
            "VoipVolume",
            "MicThreshold",
            "FOIPEnabled",
            "HMDMode",
            "FacewareCamera",
            "CommsChannel",
            "VoiceActivation",
            "HEADTRACKINGSOURCE",
        ] {
            assert!(is_comms_related(name), "{name}");
        }
        for name in ["Brightness", "FOV", ""] {
            assert!(!is_comms_related(name), "{name}");
        }
    }
}
