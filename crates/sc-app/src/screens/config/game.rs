//! The Game settings tab when the build's own settings are known: the Options layout's tabs
//! and sections, then "Other", then attributes the build no longer has. Every line has the
//! same height so the whole list can scroll as one virtual list.

use super::{Message, NOT_IN_BUILD_GROUP, OTHER_GROUP, OVERRIDDEN_BY_CFG};
use crate::icons;
use crate::screens::attr_edit::Doc;
use crate::screens::typed::{self, Kind};
use crate::theme::{self, Tone, layout, size, space};
use crate::widgets::{Was, change_marker, divider, heading, hint, icon_toned, label, strong};
use iced::widget::{Space, button, column, container, row, text, tooltip};
use iced::{Alignment, Element, Length, Padding};
use sc_core::catalog::layout::{Entry, Kind as LayoutKind, Placed};
use sc_core::defaults::GameData;
use sc_core::usercfg::UserCfg;
use std::borrow::Cow;
use std::collections::HashSet;

const LINE_PADDING: Padding = Padding {
    top: 0.0,
    right: space::LG,
    bottom: 0.0,
    left: space::XS,
};
const HEADING_PADDING: Padding = Padding {
    top: 0.0,
    right: space::LG,
    bottom: space::SM,
    left: space::LG,
};
const RESET_HINT: &str = "Put the game's default back";
const REMOVE_HINT: &str = "Remove from the file";
const SEPARATOR: &str = " \u{b7} ";

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Toggle,
    Slider {
        min: f64,
        max: f64,
        step: f64,
    },
    Number,
    Text,
    /// Value and the label to show for it.
    Choice(Vec<(String, String)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct GameRow {
    pub name: String,
    pub label: String,
    pub note: Option<String>,
    pub shape: Shape,
    pub default: Option<String>,
    pub cvar: Option<String>,
    /// An attribute in the player's file that the build does not have.
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Tab(String),
    Section(String),
    /// A section outside the layout's tabs: "Other" and the stale attributes.
    Group(String),
    Setting(GameRow),
}

impl GameRow {
    fn kind(&self) -> Kind<'static> {
        match &self.shape {
            Shape::Toggle => Kind::Bool,
            Shape::Slider { min, max, step } => Kind::Range {
                min: *min,
                max: *max,
                step: *step,
            },
            Shape::Number => Kind::Number,
            Shape::Text => Kind::Text,
            Shape::Choice(choices) => Kind::Choice(Cow::Owned(choices.clone())),
        }
    }

    /// The default as the game's menu would show it.
    fn default_text(&self) -> Option<String> {
        let default = self.default.as_deref()?;
        Some(match &self.shape {
            Shape::Toggle if default.trim() == "1" => "on".to_string(),
            Shape::Toggle => "off".to_string(),
            Shape::Choice(choices) => choices
                .iter()
                .find(|(value, _)| value == default)
                .map_or_else(|| default.to_string(), |(_, label)| label.clone()),
            _ => default.to_string(),
        })
    }

    /// A toggle and a choice show their value in full, so there is nothing half typed to keep.
    pub fn is_typed(&self) -> bool {
        !matches!(self.shape, Shape::Toggle | Shape::Choice(_))
    }

    fn matches(&self, query: &str, value: Option<&str>) -> bool {
        self.label.to_lowercase().contains(query)
            || self.name.to_lowercase().contains(query)
            || value.is_some_and(|value| value.to_lowercase().contains(query))
    }
}

pub fn lines(game: &GameData, placed: &Placed, stale: &[&str]) -> Vec<Line> {
    let mut lines = Vec::new();
    for (tab, sections) in &placed.tabs {
        if placed.tabs.len() > 1 {
            lines.push(Line::Tab(game.label(Some(&tab.key), &tab.fallback)));
        }
        for (section, entries) in sections {
            lines.push(Line::Section(
                game.label(section.key.as_deref(), &section.fallback),
            ));
            let rows = entries.iter().filter_map(|entry| placed_row(game, entry));
            lines.extend(rows.map(Line::Setting));
        }
    }
    if !placed.other.is_empty() {
        lines.push(Line::Group(OTHER_GROUP.to_string()));
        let rows = placed.other.iter().map(|name| other_row(game, name));
        lines.extend(rows.map(Line::Setting));
    }
    let mut seen = HashSet::new();
    let stale: Vec<&str> = stale.iter().copied().filter(|n| seen.insert(*n)).collect();
    if !stale.is_empty() {
        lines.push(Line::Group(NOT_IN_BUILD_GROUP.to_string()));
        lines.extend(stale.into_iter().map(|name| Line::Setting(stale_row(name))));
    }
    lines
}

fn placed_row(game: &GameData, entry: &Entry) -> Option<GameRow> {
    let setting = game.setting(&entry.name)?;
    let fallback = entry.fallback.as_deref().unwrap_or(&entry.name);
    Some(GameRow {
        name: entry.name.clone(),
        label: game.label(entry.label_key.as_deref(), fallback),
        note: entry.note.clone(),
        shape: shape_of(game, &entry.kind),
        default: Some(setting.default.clone()),
        cvar: setting.cvar.clone(),
        stale: false,
    })
}

fn shape_of(game: &GameData, kind: &LayoutKind) -> Shape {
    match kind {
        LayoutKind::Toggle => Shape::Toggle,
        LayoutKind::Slider { min, max, step } => Shape::Slider {
            min: *min,
            max: *max,
            step: *step,
        },
        LayoutKind::Number => Shape::Number,
        LayoutKind::Text => Shape::Text,
        LayoutKind::Choice(choices) => Shape::Choice(
            choices
                .iter()
                .map(|(value, key)| (value.clone(), game.label(Some(key), value)))
                .collect(),
        ),
    }
}

fn other_row(game: &GameData, name: &str) -> GameRow {
    let setting = game.setting(name);
    let default = setting.map(|setting| setting.default.clone());
    let numeric = default
        .as_deref()
        .is_some_and(|default| default.trim().parse::<f64>().is_ok());
    GameRow {
        name: name.to_string(),
        label: conventional_label(game, name).unwrap_or_else(|| readable(name)),
        note: None,
        shape: if numeric { Shape::Number } else { Shape::Text },
        default,
        cvar: setting.and_then(|setting| setting.cvar.clone()),
        stale: false,
    }
}

fn stale_row(name: &str) -> GameRow {
    GameRow {
        name: name.to_string(),
        label: name.to_string(),
        note: None,
        shape: Shape::Text,
        default: None,
        cvar: None,
        stale: true,
    }
}

/// The game names most of its settings `pause_Options<Name>`.
fn conventional_label(game: &GameData, name: &str) -> Option<String> {
    let text = game.strings.get(&format!("pause_Options{name}"))?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// `AudioMasterVolume` becomes `Audio master volume`; an all-capitals word such as `HMD` stays.
pub fn readable(name: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let chars: Vec<char> = name.chars().collect();
    for (at, &c) in chars.iter().enumerate() {
        let previous = at.checked_sub(1).map(|before| chars[before]);
        let next = chars.get(at + 1);
        let boundary = match previous {
            None => true,
            Some(before) => {
                c.is_uppercase()
                    && (before.is_lowercase()
                        || before.is_ascii_digit()
                        || (before.is_uppercase() && next.is_some_and(|n| n.is_lowercase())))
            }
        };
        if c == '_' || c == ' ' {
            words.push(String::new());
        } else if boundary || words.is_empty() {
            words.push(c.to_string());
        } else if let Some(word) = words.last_mut() {
            word.push(c);
        }
    }
    let mut words = words.into_iter().filter(|word| !word.is_empty());
    let first = words.next().map(capitalised).unwrap_or_default();
    let rest = words.map(|word| lowered(&word));
    std::iter::once(first)
        .chain(rest)
        .collect::<Vec<_>>()
        .join(" ")
}

fn lowered(word: &str) -> String {
    if word.chars().count() > 1 && word.chars().all(char::is_uppercase) {
        word.to_string()
    } else {
        word.to_lowercase()
    }
}

fn capitalised(word: String) -> String {
    let word = lowered(&word);
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Which lines show for a search: matching settings, each under the headings above it.
pub fn shown(lines: &[Line], doc: Doc<'_>, query: &str) -> Vec<usize> {
    let mut shown = Vec::new();
    let (mut tab, mut section) = (None, None);
    for (at, line) in lines.iter().enumerate() {
        match line {
            Line::Tab(_) => (tab, section) = (Some(at), None),
            Line::Section(_) => section = Some(at),
            Line::Group(_) => (tab, section) = (None, Some(at)),
            Line::Setting(row) => {
                if !query.is_empty() && !row.matches(query, doc.get(&row.name)) {
                    continue;
                }
                shown.extend(tab.take());
                shown.extend(section.take());
                shown.push(at);
            }
        }
    }
    shown
}

pub fn setting_count(lines: &[Line]) -> usize {
    lines
        .iter()
        .filter(|line| matches!(line, Line::Setting(_)))
        .count()
}

pub fn matched_count(lines: &[Line], shown: &[usize]) -> usize {
    shown
        .iter()
        .filter(|&&at| matches!(lines[at], Line::Setting(_)))
        .count()
}

/// The default of a setting the build has, by name.
pub fn default_of<'a>(lines: &'a [Line], name: &str) -> Option<&'a str> {
    lines.iter().find_map(|line| match line {
        Line::Setting(row) if row.name == name => row.default.as_deref(),
        _ => None,
    })
}

/// Whether a typed value is the default's own value, so the game would not store it.
pub fn same_as_default(value: &str, default: &str) -> bool {
    let (value, default) = (value.trim(), default.trim());
    if value == default {
        return true;
    }
    match (value.parse::<f64>(), default.parse::<f64>()) {
        (Ok(value), Ok(default)) => value == default,
        _ => false,
    }
}

/// What a line needs from the screen to draw itself.
#[derive(Clone, Copy)]
pub struct View<'a> {
    pub doc: Doc<'a>,
    pub cfg: Option<&'a UserCfg>,
    pub draft: Option<&'a (String, String)>,
}

impl<'a> View<'a> {
    fn overridden(&self, row: &GameRow) -> bool {
        match (self.cfg, row.cvar.as_deref()) {
            (Some(cfg), Some(cvar)) => cfg.get(cvar).is_some(),
            _ => false,
        }
    }

    fn draft_for(&self, row: &GameRow) -> Option<&'a str> {
        self.draft
            .filter(|(name, _)| *name == row.name)
            .map(|(_, value)| value.as_str())
    }
}

pub fn line<'a>(line: &'a Line, view: View<'a>) -> Element<'a, Message> {
    match line {
        Line::Tab(title) => heading_line(strong(title).into()),
        Line::Section(title) | Line::Group(title) => {
            heading_line(heading(title.to_uppercase()).into())
        }
        Line::Setting(row) => setting_line(row, view),
    }
}

fn heading_line<'a>(title: Element<'a, Message>) -> Element<'a, Message> {
    column![
        container(title)
            .padding(HEADING_PADDING)
            .height(layout::GAME_ROW_HEIGHT - theme::DIVIDER)
            .align_y(Alignment::End)
            .width(Length::Fill),
        divider(),
    ]
    .height(layout::GAME_ROW_HEIGHT)
    .width(Length::Fill)
    .into()
}

fn setting_line<'a>(row: &'a GameRow, view: View<'a>) -> Element<'a, Message> {
    let doc = view.doc;
    let current = doc.get(&row.name);
    let was = doc.was(&row.name);
    let staged = current != was;
    let customised = match (current, row.default.as_deref()) {
        (Some(value), Some(default)) => !same_as_default(value, default),
        _ => false,
    };
    let shown = current.or_else(|| view.draft_for(row));
    let on_set = doc.editable.then(|| {
        let name = row.name.clone();
        move |value: String| Message::SetAttr(name.clone(), value)
    });
    let control = typed::control(row.kind(), shown, row.default.as_deref(), on_set);
    let reset: Element<'a, Message> = match (doc.editable, current) {
        (true, Some(_)) => reset_button(row),
        _ => Space::new().into(),
    };
    let line = container(
        row![
            change_marker(staged || customised),
            container(texts(row, view, was))
                .width(Length::Fill)
                .clip(true),
            control,
            container(reset)
                .width(layout::ROW_ACTION_WIDTH)
                .align_x(Alignment::End),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center),
    )
    .padding(LINE_PADDING)
    .height(layout::GAME_ROW_HEIGHT - theme::DIVIDER)
    .align_y(Alignment::Center)
    .width(Length::Fill);
    column![line, divider()]
        .height(layout::GAME_ROW_HEIGHT)
        .width(Length::Fill)
        .into()
}

fn texts<'a>(row: &'a GameRow, view: View<'a>, was: Option<&str>) -> Element<'a, Message> {
    let mut title = iced::widget::row![label(&row.label).wrapping(text::Wrapping::None)]
        .spacing(space::SM)
        .align_y(Alignment::Center);
    if row.name != row.label {
        title = title.push(
            text(&row.name)
                .size(size::CAPTION)
                .font(theme::FONT_MONO)
                .style(theme::caption)
                .wrapping(text::Wrapping::None),
        );
    }
    let mut column = iced::widget::column![title].spacing(space::XS / 2.0);
    for line in hint_lines(row, view, was) {
        column = column.push(hint(line).wrapping(text::Wrapping::None));
    }
    column.into()
}

/// First line: the default and what the row held before a staged edit. Second line: why
/// changes will not stick, else the layout's own note.
fn hint_lines(row: &GameRow, view: View<'_>, was: Option<&str>) -> Vec<String> {
    let mut first = Vec::new();
    if let Some(default) = row.default_text() {
        first.push(format!("Default: {default}"));
    }
    if view.doc.get(&row.name) != was {
        first.push(
            was.map_or(Was::NotSet, |was| Was::Value(was.to_string()))
                .text(),
        );
    }
    let mut lines = Vec::new();
    if !first.is_empty() {
        lines.push(first.join(SEPARATOR));
    }
    if view.overridden(row) {
        lines.push(OVERRIDDEN_BY_CFG.to_string());
    } else if let Some(note) = &row.note {
        lines.push(note.clone());
    }
    lines
}

fn reset_button<'a>(row: &'a GameRow) -> Element<'a, Message> {
    let tip = if row.stale { REMOVE_HINT } else { RESET_HINT };
    tooltip(
        button(icon_toned(icons::UNDO, layout::ICON, Tone::Muted))
            .padding(space::SM)
            .style(theme::ghost)
            .on_press(Message::ResetAttr(row.name.clone())),
        container(hint(tip)).padding(space::SM).style(theme::toast),
        tooltip::Position::Left,
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::catalog::layout::place;
    use sc_core::defaults::Sources;

    fn game() -> GameData {
        GameData::from_sources(
            "4.1.0-test".into(),
            &Sources {
                profile: include_str!("../../../tests/fixtures/game_data/defaultProfile.xml")
                    .into(),
                attributes: include_str!("../../../tests/fixtures/config_game/attributes.xml")
                    .into(),
                key_names: include_str!(
                    "../../../tests/fixtures/game_data/keybinding_localization.xml"
                )
                .into(),
                strings: include_str!("../../../tests/fixtures/config_game/global.ini").into(),
            },
        )
        .unwrap()
    }

    fn setting_names(lines: &[Line]) -> Vec<&str> {
        lines
            .iter()
            .filter_map(|line| match line {
                Line::Setting(row) => Some(row.name.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_setting_of_the_build_is_listed_exactly_once() {
        let game = game();
        let lines = lines(&game, &place(&game), &["OldMouseCurve"]);
        let mut listed = setting_names(&lines);
        listed.sort_unstable();
        let mut expected: Vec<&str> = game.settings.iter().map(|s| s.name.as_str()).collect();
        expected.push("OldMouseCurve");
        expected.sort_unstable();
        assert_eq!(listed, expected);
    }

    #[test]
    fn groups_come_in_layout_order_then_other_then_not_in_this_version() {
        let game = game();
        let lines = lines(&game, &place(&game), &["OldMouseCurve", "OldMouseCurve"]);
        let sections: Vec<&str> = lines
            .iter()
            .filter_map(|line| match line {
                Line::Section(title) | Line::Group(title) => Some(title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            sections,
            [
                "Head tracking",
                "Head tracking: when it is off",
                "Head tracking: third person",
                OTHER_GROUP,
                NOT_IN_BUILD_GROUP
            ]
        );
        assert!(
            !lines.iter().any(|line| matches!(line, Line::Tab(_))),
            "one tab needs no heading of its own"
        );
        assert_eq!(
            setting_names(&lines)
                .iter()
                .filter(|n| **n == "OldMouseCurve")
                .count(),
            1
        );
    }

    #[test]
    fn placed_rows_take_game_labels_defaults_and_layout_kinds() {
        let game = game();
        let lines = lines(&game, &place(&game), &[]);
        let row = |name: &str| {
            lines
                .iter()
                .find_map(|line| match line {
                    Line::Setting(row) if row.name == name => Some(row.clone()),
                    _ => None,
                })
                .unwrap()
        };
        let source = row("HeadtrackingSource");
        assert_eq!(source.label, "Tracking device");
        assert_eq!(source.shape, Shape::Number);
        assert_eq!(source.default.as_deref(), Some("0"));
        let walking = row("HeadtrackingDisableDuringWalking");
        assert_eq!(walking.label, "Off on foot");
        assert_eq!(walking.default_text().as_deref(), Some("on"));
        assert_eq!(
            walking.cvar.as_deref(),
            Some("g_headtrackingToggleDisableDuringWalking")
        );
        assert!(matches!(
            row("HeadtrackingGlobalSmoothingPosition").shape,
            Shape::Slider { .. }
        ));
        assert_eq!(row("AudioMusicVolume").label, "Music volume");
        assert_eq!(row("AudioMasterVolume").label, "Audio master volume");
        assert_eq!(row("AudioMasterVolume").shape, Shape::Number);
        assert_eq!(row("Language").shape, Shape::Text);
    }

    #[test]
    fn a_choice_shows_game_labels_and_falls_back_to_the_value() {
        let game = game();
        let entry = Entry {
            name: "Language".into(),
            label_key: None,
            fallback: Some("Language".into()),
            kind: LayoutKind::Choice(vec![
                ("english".into(), "pause_OptionsFieldOfView".into()),
                ("french".into(), "missing_key".into()),
            ]),
            note: None,
            default: None,
        };
        let row = placed_row(&game, &entry).unwrap();
        assert_eq!(
            row.shape,
            Shape::Choice(vec![
                ("english".into(), "Field of view".into()),
                ("french".into(), "French".into()),
            ])
        );
        assert_eq!(row.default_text().as_deref(), Some("Field of view"));
        assert!(!row.is_typed());
    }

    #[test]
    fn readable_splits_camel_case_and_keeps_acronyms() {
        for (name, want) in [
            ("AudioMasterVolume", "Audio master volume"),
            ("HMDCursorSensitivity", "HMD cursor sensitivity"),
            ("FOV", "FOV"),
            ("show_hints", "Show hints"),
            ("Setting003", "Setting003"),
            ("VoipPushToTalk", "Voip push to talk"),
            ("", ""),
        ] {
            assert_eq!(readable(name), want, "{name}");
        }
    }

    #[test]
    fn same_as_default_compares_numbers_as_numbers() {
        assert!(same_as_default("0.8", "0.8"));
        assert!(same_as_default("0", "0.0"));
        assert!(same_as_default("0.", "0.0"));
        assert!(same_as_default(" 1 ", "1"));
        assert!(!same_as_default("0.85", "0.8"));
        assert!(!same_as_default("english", "french"));
        assert!(!same_as_default("", "0"));
    }
}
