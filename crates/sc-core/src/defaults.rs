//! Game default values, parsed from the game's own decoded XML and strings.

use crate::actionmaps::DeviceKind;
use crate::error::{Error, Result};
use crate::localization::Strings;
use crate::xml_edit::{self, Element};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

mod loader;

pub use loader::{
    ATTRIBUTES, KEY_NAMES, PROFILE, Unavailable, from_archive, load, seed_cache, strings_entry,
};

const ACTION_PREFIXES: [&str; 2] = ["v_", "pc_"];

#[derive(Debug, Clone, PartialEq)]
pub struct GameData {
    pub build: String,
    pub maps: Vec<MapDef>,
    pub settings: Vec<SettingDef>,
    pub input_names: HashMap<(DeviceKind, String), String>,
    pub option_trees: Vec<OptionTree>,
    pub strings: Strings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapDef {
    pub name: String,
    pub label: Option<String>,
    pub category: Option<String>,
    pub actions: Vec<ActionDef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDef {
    pub name: String,
    pub label: Option<String>,
    pub description: Option<String>,
    /// An empty string means the game explicitly leaves that device unbound.
    pub defaults: BTreeMap<DeviceKind, String>,
    pub activation_mode: Option<String>,
    pub option_group: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingDef {
    pub name: String,
    pub default: String,
    pub cvar: Option<String>,
    pub platform: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OptionTree {
    /// The keyboard tree covers the mouse too.
    pub kind: DeviceKind,
    pub instances: u32,
    pub sensitivity: Option<(f64, f64)>,
    /// Depth-first, so a parent always comes before its children.
    pub groups: Vec<OptionGroup>,
}

/// The show flags are kept as the game states them: 1, 0 or -1.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionGroup {
    pub name: String,
    pub parent: Option<usize>,
    pub label: Option<String>,
    pub show_invert: i8,
    pub show_curve: i8,
    pub show_sensitivity: i8,
    pub invert: Option<bool>,
    pub invert_cvar: Option<String>,
    pub exponent: Option<f64>,
    pub curve: Vec<(f64, f64)>,
}

/// How much a build's defaults hold. `labels_found` of `label_keys` action labels resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub maps: usize,
    pub actions: usize,
    pub settings: usize,
    pub option_trees: usize,
    pub groups: usize,
    pub label_keys: usize,
    pub labels_found: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Sources {
    pub profile: String,
    pub attributes: String,
    pub key_names: String,
    pub strings: String,
}

impl GameData {
    pub fn from_sources(build: String, sources: &Sources) -> Result<GameData> {
        let (maps, option_trees) = parse_profile(&sources.profile)?;
        Ok(GameData {
            build,
            maps,
            settings: parse_settings(&sources.attributes)?,
            input_names: parse_input_names(&sources.key_names)?,
            option_trees,
            strings: Strings::parse(&sources.strings),
        })
    }

    pub fn counts(&self) -> Counts {
        let actions = || self.maps.iter().flat_map(|map| &map.actions);
        let keys: Vec<&str> = actions()
            .filter_map(|action| action.label.as_deref())
            .collect();
        let found = |key: &&str| {
            self.strings
                .get(key)
                .is_some_and(|text| !text.trim().is_empty())
        };
        Counts {
            maps: self.maps.len(),
            actions: actions().count(),
            settings: self.settings.len(),
            option_trees: self.option_trees.len(),
            groups: self.option_trees.iter().map(|tree| tree.groups.len()).sum(),
            label_keys: keys.len(),
            labels_found: keys.into_iter().filter(found).count(),
        }
    }

    pub fn label(&self, key: Option<&str>, fallback: &str) -> String {
        key.and_then(|key| self.strings.get(key))
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map_or_else(|| humanise(fallback), str::to_string)
    }

    pub fn map(&self, name: &str) -> Option<&MapDef> {
        self.maps.iter().find(|map| map.name == name)
    }

    pub fn setting(&self, name: &str) -> Option<&SettingDef> {
        self.settings.iter().find(|setting| setting.name == name)
    }

    pub fn input_label(&self, input: &str) -> Option<String> {
        let (kind, keys) = split_device_prefix(input)?;
        let labels: Vec<String> = keys
            .split('+')
            .map(|key| self.key_label(kind, key.trim()))
            .collect();
        Some(labels.join(" + "))
    }

    fn key_label(&self, kind: DeviceKind, key: &str) -> String {
        let lower = key.to_lowercase();
        let named = self.input_names.get(&(kind, lower.clone())).or_else(|| {
            (kind == DeviceKind::Keyboard)
                .then(|| self.input_names.get(&(DeviceKind::Mouse, lower)))
                .flatten()
        });
        named
            .and_then(|label_key| self.strings.get(label_key))
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map_or_else(|| key.to_string(), str::to_string)
    }
}

/// `kb1_lalt+f` gives the keyboard and `lalt+f`. An unbound input (`kb1_ `) gives `None`.
fn split_device_prefix(input: &str) -> Option<(DeviceKind, &str)> {
    let (prefix, keys) = input.split_once('_')?;
    let digits = prefix.get(2..)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let kind = match prefix.get(..2)? {
        "kb" => DeviceKind::Keyboard,
        "mo" => DeviceKind::Mouse,
        "gp" => DeviceKind::Gamepad,
        "js" => DeviceKind::Joystick,
        _ => return None,
    };
    let keys = keys.trim();
    (!keys.is_empty()).then_some((kind, keys))
}

fn device_kind(device: &str) -> Option<DeviceKind> {
    match device {
        "keyboard" => Some(DeviceKind::Keyboard),
        "mouse" => Some(DeviceKind::Mouse),
        "control_pad" => Some(DeviceKind::Gamepad),
        _ => device
            .strip_prefix("joystick")
            .filter(|rest| rest.bytes().all(|b| b.is_ascii_digit()))
            .map(|_| DeviceKind::Joystick),
    }
}

fn tree_kind(kind: &str) -> Option<DeviceKind> {
    match kind {
        "keyboard" => Some(DeviceKind::Keyboard),
        "mouse" => Some(DeviceKind::Mouse),
        "gamepad" => Some(DeviceKind::Gamepad),
        "joystick" => Some(DeviceKind::Joystick),
        _ => None,
    }
}

fn attr<'a>(element: &'a Element, name: &str) -> Option<&'a str> {
    element
        .attrs
        .iter()
        .find(|attr| attr.name == name)
        .map(|attr| attr.value.as_str())
}

fn label_key(element: &Element, name: &str) -> Option<String> {
    let value = attr(element, name)?.trim();
    let value = value.strip_prefix('@').unwrap_or(value);
    (!value.is_empty()).then(|| value.to_string())
}

/// A value a later build writes in a form this one does not know is skipped, so one odd
/// number never costs the whole build's defaults.
fn number<T: std::str::FromStr>(element: &Element, name: &str) -> Option<T> {
    attr(element, name)?.trim().parse().ok()
}

fn flag(element: &Element, name: &str) -> i8 {
    number(element, name).unwrap_or(0)
}

fn scan_root(path: &str, text: &str, root: &str) -> Result<(std::path::PathBuf, Vec<Element>)> {
    let path = Path::new(path).to_path_buf();
    let elements = xml_edit::scan(&path, text)?.elements;
    match elements.first() {
        Some(first) if first.name == root => Ok((path, elements)),
        _ => Err(Error::Parse {
            path,
            line: 1,
            message: format!("expected <{root}> as the root element"),
        }),
    }
}

fn default_bind(value: &str) -> String {
    value.trim().to_string()
}

fn parse_settings(text: &str) -> Result<Vec<SettingDef>> {
    let (_, elements) = scan_root("attributes.xml", text, "Attributes")?;
    Ok(elements
        .iter()
        .filter(|element| element.name == "Attr" && element.parent == Some(0))
        .filter_map(|element| {
            Some(SettingDef {
                name: attr(element, "name")?.to_string(),
                default: attr(element, "value").unwrap_or_default().to_string(),
                cvar: attr(element, "cvar")
                    .filter(|cvar| !cvar.is_empty())
                    .map(str::to_string),
                platform: attr(element, "platform")
                    .filter(|platform| !platform.is_empty())
                    .map(str::to_string),
            })
        })
        .collect())
}

fn parse_input_names(text: &str) -> Result<HashMap<(DeviceKind, String), String>> {
    let (_, elements) = scan_root("keybinding_localization.xml", text, "LocalizedKeys")?;
    let mut names = HashMap::new();
    for element in elements.iter().filter(|element| element.name == "Key") {
        let Some(parent) = element.parent else {
            continue;
        };
        let device = &elements[parent];
        if device.name != "device" {
            continue;
        }
        let (Some(kind), Some(key), Some(label)) = (
            attr(device, "name").and_then(device_kind),
            attr(element, "name"),
            label_key(element, "localizationString"),
        ) else {
            continue;
        };
        names.entry((kind, key.to_lowercase())).or_insert(label);
    }
    Ok(names)
}

/// Where an element sits in the option trees: the tree, and the group when it is one.
type TreePlace = (usize, Option<usize>);

#[derive(Default)]
struct ProfileReader {
    maps: Vec<MapDef>,
    trees: Vec<OptionTree>,
    map_at: HashMap<usize, usize>,
    action_at: HashMap<usize, (usize, usize)>,
    device_at: HashMap<usize, (usize, usize, DeviceKind)>,
    tree_place: HashMap<usize, TreePlace>,
    curve_of: HashMap<usize, TreePlace>,
    child_filled: Option<usize>,
}

fn parse_profile(text: &str) -> Result<(Vec<MapDef>, Vec<OptionTree>)> {
    let (_, elements) = scan_root("defaultProfile.xml", text, "profile")?;
    let mut reader = ProfileReader::default();
    for (index, element) in elements.iter().enumerate() {
        reader.read(index, element);
    }
    Ok((reader.maps, reader.trees))
}

impl ProfileReader {
    fn read(&mut self, index: usize, element: &Element) {
        let Some(parent) = element.parent else {
            return;
        };
        match element.name.as_str() {
            "actionmap" if parent == 0 => self.actionmap(index, element),
            "action" => self.action(index, parent, element),
            "inputdata" => self.inputdata(parent, element),
            _ if self.action_at.contains_key(&parent) => {
                self.action_child(index, parent, element);
            }
            "optiontree" if parent == 0 => self.optiontree(index, element),
            "optiongroup" => self.optiongroup(index, parent, element),
            "nonlinearity_curve" => {
                if let Some(&(tree, Some(group))) = self.tree_place.get(&parent) {
                    self.curve_of.insert(index, (tree, Some(group)));
                }
            }
            "point" => self.point(parent, element),
            _ => {}
        }
    }

    fn actionmap(&mut self, index: usize, element: &Element) {
        self.map_at.insert(index, self.maps.len());
        self.maps.push(MapDef {
            name: attr(element, "name").unwrap_or_default().to_string(),
            label: label_key(element, "UILabel"),
            category: label_key(element, "UICategory"),
            actions: Vec::new(),
        });
    }

    fn action(&mut self, index: usize, parent: usize, element: &Element) {
        let Some(&map) = self.map_at.get(&parent) else {
            return;
        };
        let defaults = [
            ("keyboard", DeviceKind::Keyboard),
            ("mouse", DeviceKind::Mouse),
            ("gamepad", DeviceKind::Gamepad),
            ("joystick", DeviceKind::Joystick),
        ]
        .into_iter()
        .filter_map(|(name, kind)| Some((kind, default_bind(attr(element, name)?))))
        .collect();
        let actions = &mut self.maps[map].actions;
        self.action_at.insert(index, (map, actions.len()));
        actions.push(ActionDef {
            name: attr(element, "name").unwrap_or_default().to_string(),
            label: label_key(element, "UILabel"),
            description: label_key(element, "UIDescription"),
            defaults,
            activation_mode: attr(element, "activationMode").map(str::to_string),
            option_group: attr(element, "optionGroup").map(str::to_string),
        });
    }

    /// The device child states its bind as its own `input`, or through `<inputdata>` children.
    fn action_child(&mut self, index: usize, parent: usize, element: &Element) {
        let Some(kind) = tree_kind(&element.name) else {
            return;
        };
        let (map, action) = self.action_at[&parent];
        let own = attr(element, "input");
        self.maps[map].actions[action]
            .defaults
            .insert(kind, default_bind(own.unwrap_or_default()));
        self.device_at.insert(index, (map, action, kind));
        if own.is_some() {
            self.child_filled = Some(index);
        }
    }

    /// Only the first `<inputdata>` of a device child is kept: the bind model has one bind per device.
    fn inputdata(&mut self, parent: usize, element: &Element) {
        let Some(&(map, action, kind)) = self.device_at.get(&parent) else {
            return;
        };
        if self.child_filled == Some(parent) {
            return;
        }
        let bind = default_bind(attr(element, "input").unwrap_or_default());
        self.maps[map].actions[action].defaults.insert(kind, bind);
        self.child_filled = Some(parent);
    }

    fn optiontree(&mut self, index: usize, element: &Element) {
        let Some(kind) = attr(element, "type").and_then(tree_kind) else {
            return;
        };
        let sensitivity = match (
            number(element, "UISensitivityMin"),
            number(element, "UISensitivityMax"),
        ) {
            (Some(min), Some(max)) => Some((min, max)),
            _ => None,
        };
        self.tree_place.insert(index, (self.trees.len(), None));
        self.trees.push(OptionTree {
            kind,
            instances: number(element, "instances").unwrap_or(1),
            sensitivity,
            groups: Vec::new(),
        });
    }

    fn optiongroup(&mut self, index: usize, parent: usize, element: &Element) {
        let Some(&(tree, parent_group)) = self.tree_place.get(&parent) else {
            return;
        };
        let group = OptionGroup {
            name: attr(element, "name").unwrap_or_default().to_string(),
            parent: parent_group,
            label: label_key(element, "UILabel"),
            show_invert: flag(element, "UIShowInvert"),
            show_curve: flag(element, "UIShowCurve"),
            show_sensitivity: flag(element, "UIShowSensitivity"),
            invert: attr(element, "invert").map(|value| matches!(value.trim(), "1" | "true")),
            invert_cvar: attr(element, "invert_cvar")
                .filter(|cvar| !cvar.is_empty())
                .map(str::to_string),
            exponent: number(element, "exponent"),
            curve: Vec::new(),
        };
        let groups = &mut self.trees[tree].groups;
        self.tree_place.insert(index, (tree, Some(groups.len())));
        groups.push(group);
    }

    fn point(&mut self, parent: usize, element: &Element) {
        let Some(&(tree, Some(group))) = self.curve_of.get(&parent) else {
            return;
        };
        if let (Some(input), Some(output)) = (number(element, "in"), number(element, "out")) {
            self.trees[tree].groups[group].curve.push((input, output));
        }
    }
}

/// `spaceship_movement` becomes `Spaceship movement`, `v_toggle_mining_mode` becomes
/// `Toggle mining mode`.
pub fn humanise(name: &str) -> String {
    let trimmed = ACTION_PREFIXES
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(name);
    capitalised(&trimmed.replace('_', " "))
}

fn capitalised(words: &str) -> String {
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(name: &str) -> String {
        let path = format!(
            "{}/tests/fixtures/defaults/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        String::from_utf8(std::fs::read(path).unwrap()).unwrap()
    }

    fn sources() -> Sources {
        Sources {
            profile: read("defaultProfile.xml"),
            attributes: read("attributes.xml"),
            key_names: read("keybinding_localization.xml"),
            strings: read("global.ini"),
        }
    }

    fn data() -> GameData {
        GameData::from_sources("4.1.0-test".into(), &sources()).unwrap()
    }

    fn action<'a>(data: &'a GameData, map: &str, name: &str) -> &'a ActionDef {
        data.map(map)
            .unwrap()
            .actions
            .iter()
            .find(|action| action.name == name)
            .unwrap()
    }

    #[test]
    fn game_data_lists_maps_actions_and_defaults() {
        let data = data();
        assert_eq!(data.build, "4.1.0-test");
        assert_eq!(data.maps.len(), 3);
        let counts: Vec<usize> = data.maps.iter().map(|map| map.actions.len()).collect();
        assert_eq!(counts, [3, 3, 2]);

        let seat = data.map("seat_general").unwrap();
        assert_eq!(seat.label.as_deref(), Some("ui_CgSeatGeneral"));
        assert_eq!(seat.category.as_deref(), Some("ui_CcGeneral"));

        let pitch = action(&data, "spaceship_movement", "v_pitch_up");
        assert_eq!(pitch.defaults[&DeviceKind::Keyboard], "down");
        assert_eq!(pitch.defaults[&DeviceKind::Joystick], "");
        assert!(!pitch.defaults.contains_key(&DeviceKind::Gamepad));
        assert_eq!(pitch.activation_mode.as_deref(), Some("press"));
        assert_eq!(pitch.option_group.as_deref(), Some("flight_move_pitch"));
        assert_eq!(pitch.label.as_deref(), Some("ui_CiPitchUp"));
        assert_eq!(pitch.description.as_deref(), Some("ui_CiPitchUpDesc"));

        let mouse = action(&data, "spaceship_movement", "v_pitch_down");
        assert_eq!(mouse.defaults[&DeviceKind::Mouse], "maxis_y");
        assert_eq!(mouse.description, None);
    }

    #[test]
    fn a_map_without_a_label_has_none() {
        let data = data();
        let ui = data.map("ui_menu").unwrap();
        assert_eq!(ui.label, None);
        assert_eq!(ui.category, None);
        let confirm = action(&data, "ui_menu", "ui_confirm");
        assert_eq!(confirm.defaults[&DeviceKind::Keyboard], "");
        assert_eq!(confirm.defaults[&DeviceKind::Joystick], "button2");
    }

    #[test]
    fn child_element_defaults_are_read() {
        let data = data();
        let roll = action(&data, "spaceship_movement", "v_roll_left");
        assert_eq!(roll.defaults[&DeviceKind::Joystick], "button3");
        assert_eq!(roll.defaults[&DeviceKind::Gamepad], "");
    }

    #[test]
    fn a_device_element_can_state_its_own_input() {
        let data = data();
        let breath = action(&data, "seat_general", "v_hold_breath");
        assert_eq!(breath.defaults[&DeviceKind::Gamepad], "triggerl_btn");
        assert_eq!(
            breath.defaults[&DeviceKind::Keyboard],
            "",
            "a blank input still means explicitly unbound"
        );
        assert!(!breath.defaults.contains_key(&DeviceKind::Joystick));
    }

    #[test]
    fn game_data_lists_settings_with_cvar() {
        let data = data();
        assert_eq!(data.settings.len(), 8);
        let fov = data.setting("FieldOfView").unwrap();
        assert_eq!(fov.cvar.as_deref(), Some("cl_fov"));
        assert_eq!(fov.default, "80");
        assert_eq!(fov.platform, None);
        let resolution = data.setting("Resolution").unwrap();
        assert_eq!(resolution.cvar, None);
        assert_eq!(resolution.platform.as_deref(), Some("pc"));
        assert!(data.setting("Nope").is_none());
    }

    #[test]
    fn option_trees_keep_hierarchy_flags_and_curve() {
        let data = data();
        let kinds: Vec<DeviceKind> = data.option_trees.iter().map(|tree| tree.kind).collect();
        assert_eq!(
            kinds,
            [
                DeviceKind::Keyboard,
                DeviceKind::Gamepad,
                DeviceKind::Joystick
            ]
        );
        let [keyboard, gamepad, joystick] = &data.option_trees[..] else {
            panic!("three trees");
        };
        assert_eq!(keyboard.instances, 1);
        assert_eq!(joystick.instances, 8);
        assert_eq!(keyboard.sensitivity, Some((0.01, 6.25)));
        assert_eq!(joystick.sensitivity, Some((0.01, 2.0)));

        let names: Vec<&str> = keyboard.groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(
            names,
            ["master", "flight", "flight_move_pitch", "fps_view_yaw"]
        );
        let parents: Vec<Option<usize>> = keyboard.groups.iter().map(|g| g.parent).collect();
        assert_eq!(parents, [None, Some(0), Some(1), Some(0)]);
        let flight = &keyboard.groups[1];
        assert_eq!(flight.label.as_deref(), Some("ui_CoFlight"));
        assert_eq!(
            (
                flight.show_invert,
                flight.show_curve,
                flight.show_sensitivity
            ),
            (-1, -1, 0)
        );
        let pitch = &keyboard.groups[2];
        assert_eq!(pitch.invert_cvar.as_deref(), Some("cl_invertMouse"));
        assert_eq!((pitch.show_invert, pitch.show_curve), (1, 1));
        assert_eq!(pitch.show_sensitivity, 0);

        let fps_view = &gamepad.groups[1];
        assert_eq!(fps_view.curve, [(0.1, 0.015), (0.5, 0.26), (0.9, 0.78)]);
        assert!(gamepad.groups[2].curve.is_empty());
        assert_eq!(gamepad.groups[2].parent, Some(1));

        let roll = &joystick.groups[2];
        assert_eq!(roll.invert, Some(true));
        assert_eq!(roll.exponent, Some(2.0));
        assert_eq!(joystick.groups[1].invert, None);
    }

    #[test]
    fn label_falls_back_to_humanised_name() {
        let data = data();
        assert_eq!(data.label(Some("@absent"), "v_pitch_up"), "Pitch up");
        assert_eq!(data.label(None, "spaceship_movement"), "Spaceship movement");
        assert_eq!(
            data.label(Some("ui_CgSpaceFlightMovement"), "spaceship_movement"),
            "Flight movement"
        );
        assert_eq!(
            data.label(Some("@ui_cgseatgeneral"), "seat_general"),
            "Seat controls"
        );
    }

    #[test]
    fn an_empty_string_in_global_ini_falls_back_too() {
        let mut sources = sources();
        sources.strings.push_str("ui_CiPitchDown=\r\n");
        let data = GameData::from_sources("b".into(), &sources).unwrap();
        assert_eq!(
            data.label(Some("@ui_CiPitchDown"), "v_pitch_down"),
            "Pitch down"
        );
    }

    #[test]
    fn labels_lose_surrounding_spaces() {
        let mut sources = sources();
        sources.strings.push_str(
            "\r\nui_CiLookBehind=Look behind \r\nui_CiRollLeft=  \r\ninput_key_keyboard_f=F \r\n",
        );
        let data = GameData::from_sources("b".into(), &sources).unwrap();
        assert_eq!(
            data.label(Some("ui_CiLookBehind"), "v_look_behind"),
            "Look behind"
        );
        assert_eq!(
            data.label(Some("ui_CiRollLeft"), "v_roll_left"),
            "Roll left"
        );
        assert_eq!(data.input_label("kb1_lalt+f").unwrap(), "Left Alt + F");
    }

    #[test]
    fn humanise_matches_the_screen_rules() {
        assert_eq!(humanise("spaceship_movement"), "Spaceship movement");
        assert_eq!(humanise("v_toggle_mining_mode"), "Toggle mining mode");
        assert_eq!(humanise("pc_eject"), "Eject");
        assert_eq!(humanise("v_"), "V ");
        assert_eq!(humanise("jump"), "Jump");
        assert_eq!(humanise(""), "");
    }

    #[test]
    fn input_label_uses_key_names() {
        let data = data();
        assert_eq!(data.input_label("kb1_lalt+f").unwrap(), "Left Alt + F");
        assert_eq!(data.input_label("kb1_lalt+zzz").unwrap(), "Left Alt + zzz");
        assert_eq!(data.input_label("kb1_nolabel").unwrap(), "nolabel");
        assert_eq!(data.input_label("kb1_mouse3").unwrap(), "Middle button");
        assert_eq!(data.input_label("mo1_mouse3").unwrap(), "Middle button");
        assert_eq!(data.input_label("js2_button1").unwrap(), "Button 1");
        assert_eq!(data.input_label("kb1_LALT").unwrap(), "Left Alt");
    }

    #[test]
    fn input_label_is_none_for_an_unbound_or_unrecognised_input() {
        let data = data();
        assert_eq!(data.input_label("kb1_ "), None);
        assert_eq!(data.input_label(""), None);
        assert_eq!(data.input_label("lalt+f"), None);
        assert_eq!(data.input_label("xx1_a"), None);
    }

    #[test]
    fn malformed_profile_is_a_parse_error() {
        let mut sources = sources();
        let cut = sources.profile.find("<actionmap name=\"ui_menu\"").unwrap();
        sources.profile.truncate(cut);
        let err = GameData::from_sources("b".into(), &sources).unwrap_err();
        let Error::Parse { message, .. } = err else {
            panic!("expected a parse error, got {err:?}");
        };
        assert!(message.contains("never closed"), "{message}");
    }

    #[test]
    fn a_file_with_the_wrong_root_is_a_parse_error() {
        for swap in 0..3 {
            let mut sources = sources();
            let wrong = "<other/>".to_string();
            match swap {
                0 => sources.profile = wrong,
                1 => sources.attributes = wrong,
                _ => sources.key_names = wrong,
            }
            assert!(matches!(
                GameData::from_sources("b".into(), &sources),
                Err(Error::Parse { .. })
            ));
        }
    }

    #[test]
    fn a_number_that_does_not_parse_is_skipped_and_the_rest_still_loads() {
        let mut sources = sources();
        sources.profile = sources
            .profile
            .replace("exponent=\"2\"", "exponent=\"two\"")
            .replace(
                "name=\"flight_move_pitch\" UILabel=\"@ui_CoFlightPitch\" invert_cvar=\"cl_invertMouse\" UIShowCurve=\"1\"",
                "name=\"flight_move_pitch\" UILabel=\"@ui_CoFlightPitch\" invert_cvar=\"cl_invertMouse\" UIShowCurve=\"yes\"",
            )
            .replace("<point in=\"0.5\"", "<point in=\"half\"")
            .replace("UISensitivityMax=\"6.25\"", "UISensitivityMax=\"lots\"")
            .replace("instances=\"8\"", "instances=\"eight\"");
        let data = GameData::from_sources("b".into(), &sources).unwrap();
        assert_eq!(data.maps.len(), 3, "every action name is still there");
        let [keyboard, gamepad, joystick] = &data.option_trees[..] else {
            panic!("three trees");
        };
        assert_eq!(joystick.groups[2].exponent, None);
        assert_eq!(keyboard.groups[2].show_curve, 0);
        assert_eq!(keyboard.groups[2].show_invert, 1);
        assert_eq!(keyboard.sensitivity, None);
        assert_eq!(joystick.instances, 1);
        assert_eq!(gamepad.groups[1].curve, [(0.1, 0.015), (0.9, 0.78)]);
    }
}
