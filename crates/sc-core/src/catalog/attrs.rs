//! Known game settings stored in attributes.xml.

use serde::Deserialize;
use std::sync::OnceLock;

const DATA: &str = include_str!("../../data/attrs.toml");
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

#[derive(Debug, Clone, PartialEq)]
pub enum AttrKind {
    Bool,
    Float { min: f64, max: f64, step: f64 },
    RawInt,
    Text,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KnownAttr {
    pub name: String,
    pub label: String,
    pub group: String,
    pub kind: AttrKind,
    pub default: Option<String>,
    pub cvar: Option<String>,
    pub note: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DataFile {
    attr: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    name: String,
    label: String,
    group: String,
    kind: String,
    min: Option<f64>,
    max: Option<f64>,
    step: Option<f64>,
    default: Option<String>,
    cvar: Option<String>,
    note: Option<String>,
}

impl Entry {
    fn into_known(self) -> KnownAttr {
        let kind = match (self.kind.as_str(), self.min, self.max, self.step) {
            ("bool", None, None, None) => AttrKind::Bool,
            ("raw_int", None, None, None) => AttrKind::RawInt,
            ("text", None, None, None) => AttrKind::Text,
            ("float", Some(min), Some(max), Some(step)) => AttrKind::Float { min, max, step },
            (kind, ..) => panic!(
                "data/attrs.toml: {} has a bad kind {kind:?} or range",
                self.name
            ),
        };
        KnownAttr {
            name: self.name,
            label: self.label,
            group: self.group,
            kind,
            default: self.default,
            cvar: self.cvar,
            note: self.note,
        }
    }
}

pub fn all() -> &'static [KnownAttr] {
    static ALL: OnceLock<Vec<KnownAttr>> = OnceLock::new();
    ALL.get_or_init(|| {
        let data: DataFile = toml::from_str(DATA).expect("data/attrs.toml is valid");
        data.attr.into_iter().map(Entry::into_known).collect()
    })
}

pub fn find(name: &str) -> Option<&'static KnownAttr> {
    all().iter().find(|attr| attr.name == name)
}

pub fn groups() -> Vec<&'static str> {
    let mut groups: Vec<&'static str> = Vec::new();
    for attr in all() {
        if !groups.contains(&attr.group.as_str()) {
            groups.push(&attr.group);
        }
    }
    groups
}

pub fn is_comms_related(name: &str) -> bool {
    let name = name.to_lowercase();
    COMMS_KEYWORDS.iter().any(|keyword| name.contains(keyword))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn data_file_parses_and_names_are_unique() {
        let all = all();
        assert_eq!(all.len(), 18);
        let names: HashSet<_> = all.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names.len(), all.len());
        for attr in all {
            assert!(!attr.label.is_empty(), "{}", attr.name);
            assert!(attr.cvar.is_some(), "{}", attr.name);
            if let AttrKind::Float { min, max, step } = attr.kind {
                assert!(min < max && step > 0.0, "{}", attr.name);
                let default: f64 = attr.default.as_deref().unwrap().parse().unwrap();
                assert!((min..=max).contains(&default), "{}", attr.name);
                assert!(attr.note.is_some(), "{}", attr.name);
            }
            if attr.kind == AttrKind::Bool {
                assert!(
                    matches!(attr.default.as_deref(), Some("0" | "1")),
                    "{}",
                    attr.name
                );
            }
        }

        let source = find("HeadtrackingSource").unwrap();
        assert_eq!(source.kind, AttrKind::RawInt);
        assert_eq!(source.label, "Source");
        assert_eq!(source.default.as_deref(), Some("0"));
        assert_eq!(source.cvar.as_deref(), Some("g_headtrackingSource"));
        assert_eq!(
            source.note.as_deref(),
            Some("Which number means which device is not confirmed. Check the in-game menu.")
        );

        let inactivity = find("HeadtrackingInactivityTime").unwrap();
        assert_eq!(
            inactivity.kind,
            AttrKind::Float {
                min: 0.0,
                max: 30.0,
                step: 0.5
            }
        );
        assert_eq!(inactivity.default.as_deref(), Some("2.0"));

        let ads = find("HeadtrackingDisableDuringVehicleAds").unwrap();
        assert_eq!(ads.group, "Head tracking: when it is off");
        assert_eq!(
            ads.cvar.as_deref(),
            Some("g_headtrackingToggleDisableVehicleAds")
        );
        assert!(find("headtrackingsource").is_none());
        assert!(find("Unknown").is_none());
    }

    #[test]
    fn groups_are_in_data_order() {
        assert_eq!(
            groups(),
            [
                "Head tracking",
                "Head tracking: when it is off",
                "Head tracking: third person"
            ]
        );
    }

    #[test]
    fn every_known_attr_is_comms_related() {
        for attr in all() {
            assert!(is_comms_related(&attr.name), "{}", attr.name);
        }
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
