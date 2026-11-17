//! What the Config and Comms screens share about attributes.xml: loading it behind the
//! format gate, counting changes, and the plain name and value row.

use super::edits::{Edits, changed_keys};
use super::units::thousands;
use super::words::words;
use crate::context::Context;
use crate::game_data::Loaded as GameLoaded;
use crate::screen::ToastKind;
use crate::theme::{self, layout, size, space};
use crate::widgets::{banner, change_marker, divider, hint, label, text_field};
use crate::work;
use iced::widget::{column, container, row, scrollable, text};
use iced::{Alignment, Element, Length, Padding, Task};
use sc_core::attributes::{AttrEntry, Attributes};
use sc_core::defaults::{GameData, Unavailable};
use sc_core::error::Error;
use sc_core::staging::{Document, Staged};
use sc_core::xml_edit;
use std::collections::BTreeMap;
use std::path::Path;

pub const NOT_RECOGNISED: &str =
    "This file's format is not recognised, so star-config will not change it.";
pub const MISSING_TITLE: &str = "No game settings yet";
pub const MISSING_BODY: &str =
    "Start the game once and change any setting. The file appears after that.";

pub enum AttrFile {
    Loading,
    Missing,
    Editable(Box<Edits<Attributes>>),
    /// The format gate failed: shown, never written.
    Locked(Vec<AttrEntry>),
    /// The file is not well-formed XML.
    Broken {
        summary: String,
        text: String,
    },
    Unreadable(String),
}

/// The entries a view shows, and what the file held before any staged edit.
#[derive(Clone, Copy)]
pub struct Doc<'a> {
    pub current: &'a [AttrEntry],
    pub base: &'a [AttrEntry],
    pub editable: bool,
}

impl<'a> Doc<'a> {
    pub fn get(&self, name: &str) -> Option<&'a str> {
        find(self.current, name)
    }

    pub fn was(&self, name: &str) -> Option<&'a str> {
        find(self.base, name)
    }
}

fn find<'a>(entries: &'a [AttrEntry], name: &str) -> Option<&'a str> {
    entries
        .iter()
        .find(|entry| entry.name == name)
        .map(|entry| entry.value.as_str())
}

impl AttrFile {
    pub fn doc(&self) -> Option<Doc<'_>> {
        match self {
            AttrFile::Editable(edits) => Some(Doc {
                current: edits.current().entries(),
                base: edits.base().entries(),
                editable: !edits.is_away(),
            }),
            AttrFile::Locked(entries) => Some(Doc {
                current: entries,
                base: entries,
                editable: false,
            }),
            _ => None,
        }
    }

    pub fn edits(&self) -> Option<&Edits<Attributes>> {
        match self {
            AttrFile::Editable(edits) => Some(edits.as_ref()),
            _ => None,
        }
    }

    pub fn edits_mut(&mut self) -> Option<&mut Edits<Attributes>> {
        match self {
            AttrFile::Editable(edits) => Some(edits.as_mut()),
            _ => None,
        }
    }

    pub fn pending(&self) -> usize {
        self.edits().map_or(0, Edits::pending)
    }

    pub fn is_away(&self) -> bool {
        self.edits().is_some_and(Edits::is_away)
    }
}

/// Loads attributes.xml. Blocking: run it through `work::background`.
pub fn load(path: &Path) -> AttrFile {
    match Staged::<Attributes>::open(path) {
        Ok(staged) if staged.loaded().hash.is_none() => AttrFile::Missing,
        Ok(staged) if !round_trips(&staged) => AttrFile::Locked(staged.base().entries().to_vec()),
        Ok(staged) => AttrFile::Editable(Box::new(Edits::new(staged, count))),
        Err(Error::Parse { line, message, .. }) => {
            let bytes = std::fs::read(path).unwrap_or_default();
            if message.starts_with("format not recognised") {
                AttrFile::Locked(scan_entries(path, &bytes))
            } else {
                AttrFile::Broken {
                    summary: format!(
                        "attributes.xml cannot be read. Line {line}: {message}. star-config will not change it."
                    ),
                    text: String::from_utf8_lossy(&bytes).into_owned(),
                }
            }
        }
        Err(err) => AttrFile::Unreadable(format!("attributes.xml cannot be read: {}", words(&err))),
    }
}

/// The channel's game data when it is already read, else a task that reads it. The caller
/// shows `GAME_DATA_LOADING` until the task delivers.
pub fn game_data(ctx: &Context) -> (Option<GameLoaded>, Task<GameLoaded>) {
    let Some(channel) = ctx.channel.clone() else {
        return (None, Task::none());
    };
    let store = ctx.game_data();
    if let Some(cached) = store.cached(&channel) {
        return (Some(cached), Task::none());
    }
    let cache = ctx.dirs.game_data();
    let read = work::background(move || store.load(&channel, &cache), |loaded| loaded);
    (None, read)
}

pub fn game_of(loaded: Option<&GameLoaded>) -> Option<&GameData> {
    let result: &Result<GameData, Unavailable> = loaded?;
    result.as_ref().ok()
}

/// Why the game's own settings are not shown, without the Keybinds screen's wording.
pub fn unavailable_reason(unavailable: &Unavailable) -> String {
    match unavailable {
        Unavailable::NoArchive => "This channel has no Data.p4k.".to_string(),
        Unavailable::NoBuildId => "This channel has no build manifest.".to_string(),
        Unavailable::Unreadable(reason) => {
            format!("The game's Data.p4k could not be read ({reason}).")
        }
        Unavailable::Missing(entry) => format!("The game's Data.p4k has no {entry}."),
        Unavailable::Encrypted(entry) => format!("{entry} in the game's Data.p4k is encrypted."),
    }
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

/// The gate's second half: writing the parsed file back unchanged must give the same bytes.
fn round_trips(staged: &Staged<Attributes>) -> bool {
    staged.base().to_bytes() == staged.loaded().bytes
}

fn scan_entries(path: &Path, bytes: &[u8]) -> Vec<AttrEntry> {
    let text = String::from_utf8_lossy(bytes);
    let Ok(scan) = xml_edit::scan(path, &text) else {
        return Vec::new();
    };
    scan.elements
        .iter()
        .filter(|element| element.name == "Attr")
        .filter_map(|element| {
            let value_of = |key: &str| {
                element
                    .attrs
                    .iter()
                    .find(|attr| attr.name == key)
                    .map(|attr| attr.value.clone())
            };
            Some(AttrEntry {
                name: value_of("name")?,
                value: value_of("value").unwrap_or_default(),
            })
        })
        .collect()
}

pub fn count(base: &Attributes, current: &Attributes) -> usize {
    let map = |doc: &Attributes| -> BTreeMap<String, String> {
        doc.entries()
            .iter()
            .map(|entry| (entry.name.clone(), entry.value.clone()))
            .collect()
    };
    match changed_keys(&map(base), &map(current)) {
        0 if base.to_bytes() != current.to_bytes() => 1,
        changed => changed,
    }
}

/// Names of entries in `doc` for which `keep` is true, in file order, followed by names the
/// staged edits removed, so a removal still shows as a changed row.
pub fn names_with_removed<'a>(doc: &Doc<'a>, keep: impl Fn(&str) -> bool) -> Vec<&'a str> {
    let mut names: Vec<&'a str> = doc
        .current
        .iter()
        .map(|entry| entry.name.as_str())
        .filter(|name| keep(name))
        .collect();
    for entry in doc.base {
        if keep(&entry.name) && doc.get(&entry.name).is_none() {
            names.push(&entry.name);
        }
    }
    names
}

const LIST_PADDING: Padding = Padding {
    top: 0.0,
    right: space::LG,
    bottom: 0.0,
    left: space::XS,
};

/// One attribute in a dense list: name, the earlier value when changed, and a value field.
/// `on_input: None` shows the value read-only.
pub fn value_row<'a, M: Clone + 'a>(
    name: &'a str,
    value: Option<&'a str>,
    was: Option<&'a str>,
    on_input: Option<impl Fn(String) -> M + 'a>,
) -> Element<'a, M> {
    let changed = value != was;
    let mut names = column![label(name).wrapping(text::Wrapping::None)];
    if changed {
        names = names.push(hint(match was {
            Some(was) => format!("Was {was}"),
            None => "Was not set".to_string(),
        }));
    }
    let shown = value.unwrap_or_default();
    let control: Element<'a, M> = match on_input {
        Some(on_input) => text_field(was.unwrap_or_default(), shown, on_input)
            .width(layout::CONTROL_WIDTH)
            .into(),
        None => container(
            text(shown)
                .size(size::BODY)
                .font(theme::FONT_MONO)
                .style(theme::muted)
                .wrapping(text::Wrapping::None),
        )
        .width(layout::CONTROL_WIDTH)
        .clip(true)
        .into(),
    };
    let line = container(
        row![
            change_marker(changed),
            container(names).width(Length::Fill).clip(true),
            control,
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center),
    )
    .padding(LIST_PADDING)
    .height(layout::EDIT_ROW_HEIGHT - theme::DIVIDER)
    .align_y(Alignment::Center)
    .width(Length::Fill);
    column![line, divider()]
        .height(layout::EDIT_ROW_HEIGHT)
        .width(Length::Fill)
        .into()
}

/// The error for a file that is not well-formed, over its raw text, read-only.
pub fn broken<'a, M: Clone + 'a>(summary: &'a str, raw: &'a str) -> Element<'a, M> {
    column![
        banner(ToastKind::Error, summary, None),
        container(
            scrollable(
                text(raw)
                    .font(theme::FONT_MONO)
                    .size(size::CAPTION)
                    .width(Length::Fill)
            )
            .style(theme::scroll)
            .height(Length::Fill)
        )
        .padding(space::MD)
        .height(Length::Fill)
        .width(Length::Fill)
        .style(theme::sunken),
    ]
    .spacing(space::LG)
    .height(Length::Fill)
    .into()
}

/// `1,204 settings`, or `38 of 1,204` while a search narrows the list.
pub fn count_line(shown: usize, total: usize) -> String {
    match (shown == total, total) {
        (true, 1) => "1 setting".to_string(),
        (true, _) => format!("{} settings", thousands(total)),
        (false, _) => format!("{} of {}", thousands(shown), thousands(total)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const PLAIN: &str =
        "<Attributes Version=\"1\">\n <Attr name=\"FOV\" value=\"90\"/>\n</Attributes>\n";

    #[test]
    fn thousands_and_count_line() {
        assert_eq!(count_line(1204, 1204), "1,204 settings");
        assert_eq!(count_line(38, 1204), "38 of 1,204");
        assert_eq!(count_line(1, 1), "1 setting");
    }

    #[test]
    fn gate_sorts_files_into_editable_locked_broken_and_missing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("attributes.xml");
        assert!(matches!(load(&path), AttrFile::Missing));

        fs::write(&path, PLAIN).unwrap();
        assert!(matches!(load(&path), AttrFile::Editable(_)));

        fs::write(&path, format!("<!DOCTYPE Attributes>\n{PLAIN}")).unwrap();
        match load(&path) {
            AttrFile::Locked(entries) => assert_eq!(entries[0].name, "FOV"),
            _ => panic!("a DOCTYPE file must be locked"),
        }

        fs::write(
            &path,
            "<Attributes>\n <Attr name=\"A\" value=\"1/>\n</Attributes>\n",
        )
        .unwrap();
        match load(&path) {
            AttrFile::Broken { summary, .. } => assert!(summary.contains("Line 2"), "{summary}"),
            _ => panic!("malformed XML must be broken"),
        }
    }

    #[test]
    fn count_reports_changed_names() {
        let path = Path::new("attributes.xml");
        let base = Attributes::parse(path, PLAIN.as_bytes()).unwrap();
        let mut current = base.clone();
        current
            .apply(&sc_core::attributes::Op::Set {
                name: "FOV".into(),
                value: "100".into(),
            })
            .unwrap();
        assert_eq!(count(&base, &base), 0);
        assert_eq!(count(&base, &current), 1);
    }
}
