use super::attr_edit::{self, AttrFile, Doc};
use super::edits::{self, Carry, Edits, Pending, Target, Written};
use super::typed::{self, Kind};
use crate::context::Context;
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
use sc_core::catalog::attrs::{self, AttrKind, KnownAttr};
use sc_core::error::Result;
use sc_core::staging::Staged;
use std::path::PathBuf;

const SNAPSHOT_LABEL: &str = "Head tracking and comms changes";
const SUBTITLE: &str = "These are the game's own settings. star-config does not track your head. It sets what the game does with a tracker you already use.";
const SOURCE: &str = "HeadtrackingSource";
const SOURCE_HINT: &str = "Which tracker the game uses. What each number means has not been confirmed, so this is the number stored in the file.";
const OTHER_SETTINGS: &str = "Other settings in this file";
const ELSEWHERE: &str = "This name has no head tracking or comms word in it, so it will be listed on the Config screen under Game settings.";
const READ_ONLY: &str = "Close Star Citizen to apply these changes. Your edits were kept.";

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

pub struct State {
    seq: u64,
    channel: Option<PathBuf>,
    attrs: AttrFile,
    applying: bool,
    adding: Option<NewSetting>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(u64, Carry<Loaded>),
    Set(String, String),
    Remove(String),
    OpenAdd,
    AddName(String),
    AddValue(String),
    Add,
    CancelAdd,
    Applied(Carry<Applied>),
}

fn kind_of(attr: &KnownAttr) -> Kind<'static> {
    match &attr.kind {
        AttrKind::Bool => Kind::Bool,
        AttrKind::Float { min, max, step } => Kind::Range {
            min: *min,
            max: *max,
            step: *step,
        },
        AttrKind::RawInt => Kind::Number,
        AttrKind::Text => Kind::Text,
    }
}

fn hint_of(attr: &'static KnownAttr) -> Option<&'static str> {
    if attr.name == SOURCE {
        Some(SOURCE_HINT)
    } else {
        attr.note.as_deref()
    }
}

fn was(base: Option<&str>, current: Option<&str>) -> Option<Was> {
    (base != current).then(|| base.map_or(Was::NotSet, |value| Was::Value(value.to_string())))
}

fn is_other(name: &str) -> bool {
    attrs::find(name).is_none() && attrs::is_comms_related(name)
}

impl State {
    pub fn new() -> Self {
        Self {
            seq: 0,
            channel: None,
            attrs: AttrFile::Loading,
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
        let Some(channel) = channel else {
            self.channel = None;
            return Task::none();
        };
        let seq = self.seq;
        work::background(
            move || {
                Carry::new(Loaded {
                    channel: channel.dir.clone(),
                    attrs: attr_edit::load(&channel.attributes_xml()),
                })
            },
            move |loaded| Message::Loaded(seq, loaded),
        )
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
            Err(err) => Action::toast(Toast::error(format!("That change cannot be made: {err}."))),
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
            return Action::toast(Toast::info(READ_ONLY));
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
        for group in attrs::groups() {
            let items: Vec<Element<'a, Message>> = attrs::all()
                .iter()
                .filter(|attr| attr.group == group)
                .map(|attr| known_row(doc, attr))
                .collect();
            body = body.push(section(group, rows(items)));
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
        let note = (ready && !attrs::is_comms_related(name) && attrs::find(name).is_none())
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

fn known_row<'a>(doc: Doc<'a>, attr: &'static KnownAttr) -> Element<'a, Message> {
    let current = doc.get(&attr.name);
    let base = doc.was(&attr.name);
    let name = attr.name.clone();
    let on_set = doc
        .editable
        .then_some(move |value: String| Message::Set(name.clone(), value));
    let control = typed::control(kind_of(attr), current, attr.default.as_deref(), on_set);
    optional_row(
        OptionalRow {
            label: &attr.label,
            name: Some(&attr.name),
            hint: hint_of(attr),
            set: current.is_some(),
            was: was(base, current),
            remove: doc.editable.then(|| Message::Remove(attr.name.clone())),
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
    fn other_settings_are_comms_related_and_not_in_the_catalogue() {
        assert!(is_other("VoipPushToTalk"));
        assert!(!is_other("HeadtrackingToggle"));
        assert!(!is_other("FOV"));
    }

    #[test]
    fn source_gets_the_unconfirmed_note() {
        let source = attrs::find(SOURCE).unwrap();
        assert_eq!(hint_of(source), Some(SOURCE_HINT));
        assert!(matches!(kind_of(source), Kind::Number));
    }
}
