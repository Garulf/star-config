//! Where each attributes.xml setting sits in the game's Options menu.

use crate::defaults::GameData;
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::OnceLock;

const DATA: &str = include_str!("../../data/options_layout.toml");

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub tabs: Vec<Tab>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    pub key: String,
    pub fallback: String,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub key: Option<String>,
    pub fallback: String,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub label_key: Option<String>,
    pub fallback: Option<String>,
    pub kind: Kind,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Toggle,
    Slider {
        min: f64,
        max: f64,
        step: f64,
    },
    Number,
    /// Value and the label key of its in-game name.
    Choice(Vec<(String, String)>),
    Text,
}

pub type PlacedSection = (&'static Section, Vec<&'static Entry>);

#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub tabs: Vec<(&'static Tab, Vec<PlacedSection>)>,
    /// Settings of the build that the layout does not place, in the build's order.
    pub other: Vec<String>,
    /// Layout entries the build does not have.
    pub absent: Vec<&'static Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    #[serde(default)]
    tab: Vec<RawTab>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTab {
    key: String,
    fallback: String,
    #[serde(default)]
    section: Vec<RawSection>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSection {
    key: Option<String>,
    fallback: String,
    #[serde(default)]
    entry: Vec<RawEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    name: String,
    label_key: Option<String>,
    fallback: Option<String>,
    kind: String,
    min: Option<f64>,
    max: Option<f64>,
    step: Option<f64>,
    choices: Option<Vec<(String, String)>>,
    note: Option<String>,
}

pub fn layout() -> &'static Layout {
    static LAYOUT: OnceLock<Layout> = OnceLock::new();
    LAYOUT.get_or_init(|| {
        parse(DATA).unwrap_or_else(|message| panic!("data/options_layout.toml: {message}"))
    })
}

pub fn entry(name: &str) -> Option<&'static Entry> {
    layout()
        .tabs
        .iter()
        .flat_map(|tab| &tab.sections)
        .flat_map(|section| &section.entries)
        .find(|entry| entry.name == name)
}

pub fn place(game: &GameData) -> Placed {
    let have: HashSet<&str> = game.settings.iter().map(|s| s.name.as_str()).collect();
    let mut tabs = Vec::new();
    let mut absent = Vec::new();
    for tab in &layout().tabs {
        let mut sections = Vec::new();
        for section in &tab.sections {
            let (present, missing): (Vec<_>, Vec<_>) = section
                .entries
                .iter()
                .partition(|entry| have.contains(entry.name.as_str()));
            absent.extend(missing);
            if !present.is_empty() {
                sections.push((section, present));
            }
        }
        if !sections.is_empty() {
            tabs.push((tab, sections));
        }
    }
    let mut seen = HashSet::new();
    let other = game
        .settings
        .iter()
        .filter(|setting| entry(&setting.name).is_none() && seen.insert(setting.name.as_str()))
        .map(|setting| setting.name.clone())
        .collect();
    Placed {
        tabs,
        other,
        absent,
    }
}

fn parse(text: &str) -> Result<Layout, String> {
    let raw: RawFile = toml::from_str(text).map_err(|error| error.to_string())?;
    let mut names = HashSet::new();
    let mut tabs = Vec::new();
    for tab in raw.tab {
        let mut sections = Vec::new();
        for section in tab.section {
            let mut entries = Vec::new();
            for raw_entry in section.entry {
                let entry = Entry::from_raw(raw_entry)?;
                if !names.insert(entry.name.clone()) {
                    return Err(format!("{} is listed twice", entry.name));
                }
                entries.push(entry);
            }
            sections.push(Section {
                key: section.key,
                fallback: section.fallback,
                entries,
            });
        }
        tabs.push(Tab {
            key: tab.key,
            fallback: tab.fallback,
            sections,
        });
    }
    Ok(Layout { tabs })
}

impl Entry {
    fn from_raw(raw: RawEntry) -> Result<Entry, String> {
        let RawEntry {
            name,
            label_key,
            fallback,
            kind,
            min,
            max,
            step,
            choices,
            note,
        } = raw;
        if label_key.is_none() && fallback.is_none() {
            return Err(format!(
                "{name} has neither a label_key nor a fallback label"
            ));
        }
        let kind = match (kind.as_str(), min, max, step, choices) {
            ("toggle", None, None, None, None) => Kind::Toggle,
            ("number", None, None, None, None) => Kind::Number,
            ("text", None, None, None, None) => Kind::Text,
            ("slider", Some(min), Some(max), Some(step), None) if min < max && step > 0.0 => {
                Kind::Slider { min, max, step }
            }
            ("choice", None, None, None, Some(choices)) if distinct_values(&choices) => {
                Kind::Choice(choices)
            }
            ("toggle" | "number" | "text" | "slider" | "choice", ..) => {
                return Err(format!(
                    "{name} is a {kind}, which needs exactly its own range or choices \
                     (slider: min < max and step > 0; choice: distinct values)"
                ));
            }
            _ => return Err(format!("{name} has an unknown kind {kind:?}")),
        };
        Ok(Entry {
            name,
            label_key,
            fallback,
            kind,
            note,
        })
    }
}

fn distinct_values(choices: &[(String, String)]) -> bool {
    let values: HashSet<_> = choices.iter().map(|(value, _)| value).collect();
    !choices.is_empty() && values.len() == choices.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::attrs::{self, AttrKind};
    use crate::defaults::Sources;
    use crate::localization::Strings;

    fn fixture(name: &str) -> String {
        let path = format!(
            "{}/tests/fixtures/defaults/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        String::from_utf8(std::fs::read(path).unwrap()).unwrap()
    }

    fn game_with(attributes: &str) -> GameData {
        GameData::from_sources(
            "4.1.0-test".into(),
            &Sources {
                profile: fixture("defaultProfile.xml"),
                attributes: attributes.into(),
                key_names: fixture("keybinding_localization.xml"),
                strings: fixture("global.ini"),
            },
        )
        .unwrap()
    }

    fn all_entries() -> Vec<&'static Entry> {
        layout()
            .tabs
            .iter()
            .flat_map(|tab| &tab.sections)
            .flat_map(|section| &section.entries)
            .collect()
    }

    fn placed_names(placed: &Placed) -> Vec<&str> {
        placed
            .tabs
            .iter()
            .flat_map(|(_, sections)| sections)
            .flat_map(|(_, entries)| entries)
            .map(|entry| entry.name.as_str())
            .collect()
    }

    const ONE_ENTRY: &str = r#"
[[tab]]
key = "t"
fallback = "T"
[[tab.section]]
fallback = "S"
[[tab.section.entry]]
name = "A"
fallback = "A"
"#;

    #[test]
    fn layout_file_parses_and_names_are_unique() {
        let entries = all_entries();
        assert!(entries.len() >= 18);
        let names: HashSet<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names.len(), entries.len());
        for entry in &entries {
            assert!(
                entry.label_key.is_some() || entry.fallback.is_some(),
                "{}",
                entry.name
            );
            let wording = entry.fallback.as_deref().unwrap_or_default();
            assert!(!wording.starts_with("TODO"), "{}", entry.name);
        }
        for tab in &layout().tabs {
            assert!(!tab.key.is_empty() && !tab.fallback.starts_with("TODO"));
            for section in &tab.sections {
                assert!(
                    !section.fallback.starts_with("TODO"),
                    "{}",
                    section.fallback
                );
            }
        }

        let source = entry("HeadtrackingSource").unwrap();
        assert_eq!(
            source.label_key.as_deref(),
            Some("pause_OptionsHeadtrackingSource")
        );
        assert_eq!(source.kind, Kind::Number);
        assert!(entry("headtrackingsource").is_none());
        assert!(entry("Unknown").is_none());
    }

    #[test]
    fn place_puts_every_setting_exactly_once() {
        let game = game_with(&fixture("attributes.xml"));
        let placed = place(&game);
        let mut seen: Vec<&str> = placed_names(&placed);
        seen.extend(placed.other.iter().map(String::as_str));
        let mut seen_sorted = seen.clone();
        seen_sorted.sort_unstable();
        let mut expected: Vec<&str> = game.settings.iter().map(|s| s.name.as_str()).collect();
        expected.sort_unstable();
        assert_eq!(seen_sorted, expected);
        assert_eq!(placed.absent.len(), all_entries().len());
    }

    #[test]
    fn place_keeps_layout_order_and_reports_what_the_build_lacks() {
        let game = game_with(
            r#"<Attributes Version="1">
 <Attr name="Zeta" value="1"/>
 <Attr name="HeadtrackingInactivityTime" value="2.0"/>
 <Attr name="HeadtrackingSource" value="0"/>
 <Attr name="Alpha" value="1"/>
 <Attr name="Zeta" value="2"/>
 <Attr name="HeadtrackingToggle" value="0"/>
</Attributes>"#,
        );
        let placed = place(&game);
        assert_eq!(
            placed_names(&placed),
            [
                "HeadtrackingSource",
                "HeadtrackingToggle",
                "HeadtrackingInactivityTime"
            ]
        );
        assert_eq!(placed.other, ["Zeta", "Alpha"]);
        assert_eq!(placed.absent.len(), all_entries().len() - 3);
        assert!(
            placed
                .absent
                .iter()
                .all(|entry| entry.name != "HeadtrackingSource")
        );
        assert_eq!(placed.tabs.len(), 1);
        assert_eq!(placed.tabs[0].1[0].0.fallback, "Head tracking");
    }

    #[test]
    fn a_build_with_no_layout_settings_places_nothing() {
        let placed = place(&game_with(r#"<Attributes Version="1"/>"#));
        assert!(placed.tabs.is_empty() && placed.other.is_empty());
        assert_eq!(placed.absent.len(), all_entries().len());
    }

    #[test]
    fn former_attrs_catalogue_is_covered() {
        assert_eq!(attrs::all().len(), 18);
        for attr in attrs::all() {
            let entry = entry(&attr.name).unwrap_or_else(|| panic!("{} is missing", attr.name));
            let expected = match attr.kind {
                AttrKind::Bool => Kind::Toggle,
                AttrKind::Float { min, max, step } => Kind::Slider { min, max, step },
                AttrKind::RawInt => Kind::Number,
                AttrKind::Text => Kind::Text,
            };
            assert_eq!(entry.kind, expected, "{}", attr.name);
            assert_eq!(entry.fallback.as_deref(), Some(attr.label.as_str()));
            assert_eq!(entry.note, attr.note, "{}", attr.name);
            let section = layout()
                .tabs
                .iter()
                .flat_map(|tab| &tab.sections)
                .find(|section| section.entries.iter().any(|e| e.name == attr.name))
                .unwrap();
            assert_eq!(section.fallback, attr.group, "{}", attr.name);
        }
    }

    #[test]
    fn choices_and_sliders_are_well_formed() {
        for entry in all_entries() {
            match &entry.kind {
                Kind::Slider { min, max, step } => {
                    assert!(min < max && *step > 0.0, "{}", entry.name)
                }
                Kind::Choice(choices) => {
                    assert!(!choices.is_empty(), "{}", entry.name);
                    let values: HashSet<_> = choices.iter().map(|(value, _)| value).collect();
                    assert_eq!(values.len(), choices.len(), "{}", entry.name);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn every_kind_parses() {
        let layout = parse(
            r#"
[[tab]]
key = "t"
fallback = "T"
[[tab.section]]
key = "s"
fallback = "S"
[[tab.section.entry]]
name = "A"
label_key = "ka"
kind = "toggle"
[[tab.section.entry]]
name = "B"
fallback = "B"
kind = "slider"
min = 0.0
max = 2.0
step = 0.5
note = "n"
[[tab.section.entry]]
name = "C"
fallback = "C"
kind = "choice"
choices = [["0", "k0"], ["1", "k1"]]
[[tab.section.entry]]
name = "D"
fallback = "D"
kind = "number"
[[tab.section.entry]]
name = "E"
fallback = "E"
kind = "text"
"#,
        )
        .unwrap();
        let section = &layout.tabs[0].sections[0];
        assert_eq!(section.key.as_deref(), Some("s"));
        let kinds: Vec<_> = section.entries.iter().map(|e| e.kind.clone()).collect();
        assert_eq!(
            kinds,
            [
                Kind::Toggle,
                Kind::Slider {
                    min: 0.0,
                    max: 2.0,
                    step: 0.5
                },
                Kind::Choice(vec![("0".into(), "k0".into()), ("1".into(), "k1".into())]),
                Kind::Number,
                Kind::Text,
            ]
        );
        assert_eq!(section.entries[1].note.as_deref(), Some("n"));
    }

    #[test]
    fn bad_files_are_rejected_with_the_entry_named() {
        let with_entry = |body: &str| format!("{ONE_ENTRY}{body}");
        let cases = [
            (
                with_entry("kind = \"slider\"\nmin = 1.0\nmax = 1.0\nstep = 0.1\n"),
                "slider",
            ),
            (
                with_entry("kind = \"slider\"\nmin = 0.0\nmax = 1.0\nstep = 0.0\n"),
                "slider",
            ),
            (with_entry("kind = \"slider\"\n"), "slider"),
            (with_entry("kind = \"choice\"\nchoices = []\n"), "choice"),
            (
                with_entry("kind = \"choice\"\nchoices = [[\"0\", \"a\"], [\"0\", \"b\"]]\n"),
                "choice",
            ),
            (with_entry("kind = \"toggle\"\nmin = 0.0\n"), "range"),
            (with_entry("kind = \"dial\"\n"), "dial"),
            (
                format!(
                    "{ONE_ENTRY}kind = \"toggle\"\n[[tab.section.entry]]\nname = \"A\"\nfallback = \"A\"\nkind = \"toggle\"\n"
                ),
                "twice",
            ),
            (
                with_entry("kind = \"toggle\"\ncolour = \"red\"\n"),
                "colour",
            ),
        ];
        for (text, expected) in cases {
            let message = parse(&text).expect_err(&text);
            assert!(message.contains(expected), "{message} for {text}");
        }
        let unlabelled = ONE_ENTRY.replace("fallback = \"A\"\n", "") + "kind = \"toggle\"\n";
        assert!(parse(&unlabelled).unwrap_err().contains("label"));
    }

    #[test]
    #[ignore = "needs SC_GLOBAL_INI, the game's global.ini"]
    fn layout_keys_resolve_in_real_strings() {
        let path = std::env::var("SC_GLOBAL_INI").expect("SC_GLOBAL_INI");
        let strings = Strings::parse(&String::from_utf8_lossy(&std::fs::read(path).unwrap()));
        let mut missing = Vec::new();
        let mut check = |key: &str| {
            if strings.get(key).is_none() {
                missing.push(key.to_string());
            }
        };
        for tab in &layout().tabs {
            check(&tab.key);
            for section in &tab.sections {
                section.key.iter().for_each(|key| check(key));
                for entry in &section.entries {
                    entry.label_key.iter().for_each(|key| check(key));
                    if let Kind::Choice(choices) = &entry.kind {
                        choices.iter().for_each(|(_, key)| check(key));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "keys not in global.ini: {missing:?}");
    }
}
