//! The game's default binds merged with the user's `actionmaps.xml`, one row per action.

use crate::actionmaps::{
    ActionMaps, Binding, Conflict, DeviceKind, Slot, clashes, format_input, is_bound,
};
use crate::defaults::{ActionDef, GameData};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const KINDS: [DeviceKind; 4] = [
    DeviceKind::Keyboard,
    DeviceKind::Mouse,
    DeviceKind::Gamepad,
    DeviceKind::Joystick,
];

const DEFAULT_INSTANCE: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Default,
    Override,
    /// The user's file blanks this kind.
    Unbound,
    /// The game has no default for this kind and the file does not set one.
    DefaultUnbound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub kind: DeviceKind,
    /// The full input with its prefix. `None` when nothing is bound.
    pub input: Option<String>,
    pub origin: Origin,
    /// What the game binds here, shown on instance 1.
    pub default: Option<String>,
    /// The file's other rebinds of this kind, for example a layout's blanks on other sticks.
    pub extra: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub map: String,
    pub action: String,
    /// Whether the game shows the action in its menu (it has a `UILabel`).
    pub listed: bool,
    /// Whether the game's data knows the action.
    pub in_build: bool,
    /// In the order of `KINDS`.
    pub cells: [Cell; 4],
}

/// Game order first. A file action the game does not know follows its map's own actions,
/// and a map the game does not know comes last.
pub fn rows(game: Option<&GameData>, doc: &ActionMaps) -> Vec<Row> {
    let binds = Binds::of(doc);
    let mut file = file_actions(doc);
    let mut rows = Vec::new();
    for map in game.into_iter().flat_map(|game| &game.maps) {
        let in_file = file.remove(map.name.as_str()).unwrap_or_default();
        let known: HashSet<&str> = map.actions.iter().map(|def| def.name.as_str()).collect();
        rows.extend(
            map.actions
                .iter()
                .map(|def| row(&binds, &map.name, &def.name, Some(def), true)),
        );
        rows.extend(
            in_file
                .iter()
                .filter(|name| !known.contains(**name))
                .map(|name| row(&binds, &map.name, name, None, false)),
        );
    }
    let in_build = game.is_none();
    for (map, actions) in file.in_order() {
        rows.extend(
            actions
                .iter()
                .map(|name| row(&binds, map, name, None, in_build)),
        );
    }
    rows
}

/// Binds that share an input while maps are active together, where at least one is the user's.
pub fn conflicts(rows: &[Row]) -> Vec<Conflict> {
    let mut groups: BTreeMap<String, Vec<Holder>> = BTreeMap::new();
    for row in rows {
        for cell in &row.cells {
            if let Some(input) = &cell.input {
                groups
                    .entry(input.to_ascii_lowercase())
                    .or_default()
                    .push(Holder {
                        input,
                        pair: (row.map.clone(), row.action.clone()),
                        overridden: cell.origin == Origin::Override,
                    });
            }
        }
    }
    groups.into_values().filter_map(conflict_of).collect()
}

struct Holder<'a> {
    input: &'a str,
    pair: (String, String),
    overridden: bool,
}

fn conflict_of(holders: Vec<Holder>) -> Option<Conflict> {
    let involved: Vec<(String, String)> = holders
        .iter()
        .enumerate()
        .filter(|&(i, holder)| {
            holders.iter().enumerate().any(|(j, other)| {
                i != j
                    && (holder.overridden || other.overridden)
                    && clashes(&[holder.pair.clone(), other.pair.clone()])
            })
        })
        .map(|(_, holder)| holder.pair.clone())
        .collect();
    (!involved.is_empty()).then(|| Conflict {
        input: holders[0].input.to_string(),
        bindings: involved,
    })
}

/// The file's bindings by map and action, in file order.
struct Binds<'a> {
    by_action: HashMap<(&'a str, &'a str), Vec<&'a Binding>>,
}

impl<'a> Binds<'a> {
    fn of(doc: &'a ActionMaps) -> Self {
        let mut by_action: HashMap<(&str, &str), Vec<&Binding>> = HashMap::new();
        for binding in doc.bindings() {
            by_action
                .entry((&binding.map, &binding.action))
                .or_default()
                .push(binding);
        }
        Self { by_action }
    }

    fn of_kind(&self, map: &str, action: &str, kind: DeviceKind) -> Vec<&'a Binding> {
        self.by_action
            .get(&(map, action))
            .into_iter()
            .flatten()
            .filter(|binding| binding.slot.is_some_and(|slot| slot.kind == kind))
            .copied()
            .collect()
    }
}

/// The file's maps and their actions, each in file order, with lookup by map name.
struct FileActions<'a> {
    maps: Vec<(&'a str, Vec<&'a str>)>,
    at: HashMap<&'a str, usize>,
    taken: Vec<bool>,
}

impl<'a> FileActions<'a> {
    fn remove(&mut self, map: &str) -> Option<Vec<&'a str>> {
        let at = *self.at.get(map)?;
        self.taken[at] = true;
        Some(std::mem::take(&mut self.maps[at].1))
    }

    fn in_order(&self) -> impl Iterator<Item = &(&'a str, Vec<&'a str>)> {
        self.maps
            .iter()
            .zip(&self.taken)
            .filter(|(_, taken)| !**taken)
            .map(|(entry, _)| entry)
    }
}

fn file_actions(doc: &ActionMaps) -> FileActions<'_> {
    let mut maps: Vec<(&str, Vec<&str>)> = Vec::new();
    let mut at: HashMap<&str, usize> = HashMap::new();
    let mut seen: HashSet<(&str, &str)> = HashSet::new();
    for binding in doc.bindings() {
        let index = *at.entry(&binding.map).or_insert_with(|| {
            maps.push((&binding.map, Vec::new()));
            maps.len() - 1
        });
        if seen.insert((&binding.map, &binding.action)) {
            maps[index].1.push(&binding.action);
        }
    }
    let taken = vec![false; maps.len()];
    FileActions { maps, at, taken }
}

fn row(binds: &Binds, map: &str, action: &str, def: Option<&ActionDef>, in_build: bool) -> Row {
    Row {
        map: map.into(),
        action: action.into(),
        listed: def.is_none_or(|def| def.label.is_some()),
        in_build,
        cells: KINDS.map(|kind| {
            let default = def.and_then(|def| def.defaults.get(&kind));
            cell(kind, default, &binds.of_kind(map, action, kind))
        }),
    }
}

fn cell(kind: DeviceKind, default: Option<&String>, binds: &[&Binding]) -> Cell {
    let default = default.filter(|rest| !rest.is_empty()).map(|rest| {
        format_input(
            Slot {
                kind,
                instance: DEFAULT_INSTANCE,
            },
            rest,
        )
    });
    let Some(chosen) = binds
        .iter()
        .position(|bind| is_bound(&bind.input))
        .or((!binds.is_empty()).then_some(0))
    else {
        let origin = if default.is_some() {
            Origin::Default
        } else {
            Origin::DefaultUnbound
        };
        return Cell {
            kind,
            input: default.clone(),
            origin,
            default,
            extra: Vec::new(),
        };
    };
    let bound = is_bound(&binds[chosen].input);
    Cell {
        kind,
        input: bound.then(|| binds[chosen].input.clone()),
        origin: if bound {
            Origin::Override
        } else {
            Origin::Unbound
        },
        default,
        extra: binds
            .iter()
            .enumerate()
            .filter(|&(at, _)| at != chosen)
            .map(|(_, bind)| bind.input.clone())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actionmaps::{Conflict, DeviceKind::*};
    use crate::defaults::MapDef;
    use crate::localization::Strings;
    use crate::staging::Document;
    use std::collections::{BTreeMap, HashMap};
    use std::path::Path;

    fn action(name: &str, label: Option<&str>, defaults: &[(DeviceKind, &str)]) -> ActionDef {
        ActionDef {
            name: name.into(),
            label: label.map(Into::into),
            description: None,
            defaults: defaults
                .iter()
                .map(|(kind, input)| (*kind, input.to_string()))
                .collect::<BTreeMap<_, _>>(),
            activation_mode: None,
            option_group: None,
        }
    }

    fn map(name: &str, actions: Vec<ActionDef>) -> MapDef {
        MapDef {
            name: name.into(),
            label: None,
            category: None,
            actions,
        }
    }

    fn game(maps: Vec<MapDef>) -> GameData {
        GameData {
            build: "test".into(),
            maps,
            settings: Vec::new(),
            input_names: HashMap::new(),
            option_trees: Vec::new(),
            strings: Strings::parse(""),
        }
    }

    type Actions<'a> = &'a [(&'a str, &'a [&'a str])];

    fn file(maps: &[(&str, Actions)]) -> ActionMaps {
        let mut xml = String::from("<ActionMaps>\n <ActionProfiles profileName=\"default\">\n");
        for (map, actions) in maps {
            xml.push_str(&format!("  <actionmap name=\"{map}\">\n"));
            for (action, inputs) in *actions {
                xml.push_str(&format!("   <action name=\"{action}\">\n"));
                for input in *inputs {
                    xml.push_str(&format!("    <rebind input=\"{input}\"/>\n"));
                }
                xml.push_str("   </action>\n");
            }
            xml.push_str("  </actionmap>\n");
        }
        xml.push_str(" </ActionProfiles>\n</ActionMaps>\n");
        ActionMaps::parse(Path::new("actionmaps.xml"), xml.as_bytes()).unwrap()
    }

    fn empty_file() -> ActionMaps {
        file(&[])
    }

    fn cell(row: &Row, kind: DeviceKind) -> &Cell {
        row.cells.iter().find(|cell| cell.kind == kind).unwrap()
    }

    fn row<'a>(rows: &'a [Row], map: &str, action: &str) -> &'a Row {
        rows.iter()
            .find(|row| row.map == map && row.action == action)
            .unwrap_or_else(|| panic!("no row for {map} {action}"))
    }

    fn names(rows: &[Row]) -> Vec<(&str, &str)> {
        rows.iter()
            .map(|row| (row.map.as_str(), row.action.as_str()))
            .collect()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(map, action)| (map.to_string(), action.to_string()))
            .collect()
    }

    fn flight() -> GameData {
        game(vec![map(
            "spaceship_movement",
            vec![action(
                "v_pitch",
                Some("ui_pitch"),
                &[(Keyboard, "w"), (Joystick, "button1"), (Gamepad, "")],
            )],
        )])
    }

    #[test]
    fn defaults_fill_cells_the_file_does_not_set() {
        let rows = rows(Some(&flight()), &empty_file());
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(
            (row.map.as_str(), row.action.as_str()),
            ("spaceship_movement", "v_pitch")
        );
        assert!(row.listed && row.in_build);
        assert_eq!(
            row.cells.iter().map(|cell| cell.kind).collect::<Vec<_>>(),
            KINDS
        );

        assert_eq!(
            *cell(row, Keyboard),
            Cell {
                kind: Keyboard,
                input: Some("kb1_w".into()),
                origin: Origin::Default,
                default: Some("kb1_w".into()),
                extra: Vec::new(),
            }
        );
        let stick = cell(row, Joystick);
        assert_eq!(stick.input.as_deref(), Some("js1_button1"));
        assert_eq!(stick.origin, Origin::Default);
        assert_eq!(stick.default.as_deref(), Some("js1_button1"));
    }

    #[test]
    fn a_kind_the_game_leaves_unbound_or_does_not_mention_is_default_unbound() {
        let rows = rows(Some(&flight()), &empty_file());
        for kind in [Gamepad, Mouse] {
            let cell = cell(&rows[0], kind);
            assert_eq!(cell.input, None, "{kind:?}");
            assert_eq!(cell.origin, Origin::DefaultUnbound, "{kind:?}");
            assert_eq!(cell.default, None, "{kind:?}");
        }
    }

    #[test]
    fn default_gamepad_and_mouse_binds_show_on_instance_one() {
        let data = game(vec![map(
            "player",
            vec![action(
                "jump",
                Some("ui_jump"),
                &[(Gamepad, "a"), (Mouse, "mouse1")],
            )],
        )]);
        let rows = rows(Some(&data), &empty_file());
        assert_eq!(cell(&rows[0], Gamepad).input.as_deref(), Some("gp1_a"));
        assert_eq!(cell(&rows[0], Mouse).input.as_deref(), Some("mo1_mouse1"));
    }

    #[test]
    fn override_of_a_kind_hides_its_default() {
        let doc = file(&[("spaceship_movement", &[("v_pitch", &["js4_button3"])])]);
        let rows = rows(Some(&flight()), &doc);
        let row = &rows[0];
        let stick = cell(row, Joystick);
        assert_eq!(stick.input.as_deref(), Some("js4_button3"));
        assert_eq!(stick.origin, Origin::Override);
        assert_eq!(stick.default.as_deref(), Some("js1_button1"));
        let key = cell(row, Keyboard);
        assert_eq!(key.input.as_deref(), Some("kb1_w"));
        assert_eq!(key.origin, Origin::Default);
    }

    #[test]
    fn a_mouse_button_in_a_keyboard_rebind_is_a_keyboard_override() {
        let doc = file(&[("spaceship_movement", &[("v_pitch", &["kb1_mouse3"])])]);
        let rows = rows(Some(&flight()), &doc);
        let key = cell(&rows[0], Keyboard);
        assert_eq!(key.input.as_deref(), Some("kb1_mouse3"));
        assert_eq!(key.origin, Origin::Override);
        assert_eq!(cell(&rows[0], Mouse).origin, Origin::DefaultUnbound);
    }

    #[test]
    fn user_unbind_shows_as_unbound() {
        let doc = file(&[(
            "spaceship_movement",
            &[("v_pitch", &["kb1_ ", "js1_  ", "gp1_ "])],
        )]);
        let rows = rows(Some(&flight()), &doc);
        for kind in [Keyboard, Joystick] {
            let cell = cell(&rows[0], kind);
            assert_eq!(cell.input, None, "{kind:?}");
            assert_eq!(cell.origin, Origin::Unbound, "{kind:?}");
        }
        assert_eq!(cell(&rows[0], Keyboard).default.as_deref(), Some("kb1_w"));
        assert_eq!(
            cell(&rows[0], Joystick).default.as_deref(),
            Some("js1_button1")
        );
        let pad = cell(&rows[0], Gamepad);
        assert_eq!(
            (pad.origin, pad.default.as_deref()),
            (Origin::Unbound, None)
        );
    }

    #[test]
    fn extra_holds_the_other_rebinds_of_a_kind() {
        let doc = file(&[(
            "spaceship_movement",
            &[("v_pitch", &["js1_ ", "js3_button7", "js2_ "])],
        )]);
        let merged = rows(Some(&flight()), &doc);
        let stick = cell(&merged[0], Joystick);
        assert_eq!(stick.input.as_deref(), Some("js3_button7"));
        assert_eq!(stick.origin, Origin::Override);
        assert_eq!(stick.extra, ["js1_ ", "js2_ "]);

        let blanks = file(&[("spaceship_movement", &[("v_pitch", &["js2_ ", "js1_ "])])]);
        let blank_rows = rows(Some(&flight()), &blanks);
        let stick = cell(&blank_rows[0], Joystick);
        assert_eq!(
            (stick.input.as_deref(), stick.origin),
            (None, Origin::Unbound)
        );
        assert_eq!(stick.extra, ["js1_ "]);
    }

    #[test]
    fn rows_follow_game_order_and_listed_follows_the_actions_label() {
        let data = game(vec![
            map(
                "player",
                vec![
                    action("walk", Some("ui_walk"), &[]),
                    action("hidden", None, &[]),
                ],
            ),
            map(
                "spaceship_view",
                vec![action("v_look", Some("ui_look"), &[])],
            ),
        ]);
        let rows = rows(Some(&data), &empty_file());
        assert_eq!(
            names(&rows),
            [
                ("player", "walk"),
                ("player", "hidden"),
                ("spaceship_view", "v_look")
            ]
        );
        assert_eq!(
            rows.iter().map(|row| row.listed).collect::<Vec<_>>(),
            [true, false, true]
        );
    }

    #[test]
    fn file_only_actions_are_kept_with_in_build_false() {
        let data = game(vec![
            map(
                "player",
                vec![action("walk", Some("ui_walk"), &[(Keyboard, "w")])],
            ),
            map(
                "spaceship_view",
                vec![action("v_look", Some("ui_look"), &[])],
            ),
        ]);
        let doc = file(&[
            ("old_map", &[("gone", &["kb1_g"])]),
            (
                "player",
                &[
                    ("removed_action", &["kb1_r", "js2_ "]),
                    ("walk", &["kb1_x"]),
                ],
            ),
        ]);
        let rows = rows(Some(&data), &doc);
        assert_eq!(
            names(&rows),
            [
                ("player", "walk"),
                ("player", "removed_action"),
                ("spaceship_view", "v_look"),
                ("old_map", "gone"),
            ]
        );
        assert_eq!(
            rows.iter().map(|row| row.in_build).collect::<Vec<_>>(),
            [true, false, true, false]
        );

        let removed = row(&rows, "player", "removed_action");
        assert!(removed.listed);
        let key = cell(removed, Keyboard);
        assert_eq!(key.input.as_deref(), Some("kb1_r"));
        assert_eq!(
            (key.origin, key.default.as_deref()),
            (Origin::Override, None)
        );
        assert_eq!(cell(removed, Joystick).origin, Origin::Unbound);
        assert_eq!(cell(removed, Gamepad).origin, Origin::DefaultUnbound);
    }

    #[test]
    fn no_game_data_shows_file_binds_only() {
        let doc = file(&[
            ("player", &[("walk", &["kb1_w", "gp1_ "])]),
            ("spaceship_view", &[("v_look", &["js2_button4"])]),
        ]);
        let rows = rows(None, &doc);
        assert_eq!(
            names(&rows),
            [("player", "walk"), ("spaceship_view", "v_look")]
        );
        assert!(rows.iter().all(|row| row.in_build && row.listed));
        let walk = &rows[0];
        assert_eq!(cell(walk, Keyboard).input.as_deref(), Some("kb1_w"));
        assert_eq!(cell(walk, Keyboard).default, None);
        assert_eq!(cell(walk, Gamepad).origin, Origin::Unbound);
        assert_eq!(cell(walk, Mouse).origin, Origin::DefaultUnbound);
        assert_eq!(
            cell(&rows[1], Joystick).input.as_deref(),
            Some("js2_button4")
        );
    }

    #[test]
    fn no_game_data_and_an_empty_file_has_no_rows() {
        assert_eq!(rows(None, &empty_file()), []);
    }

    fn shared_key() -> GameData {
        game(vec![map(
            "spaceship_movement",
            vec![
                action("v_a", Some("ui_a"), &[(Keyboard, "x")]),
                action("v_b", Some("ui_b"), &[(Keyboard, "x")]),
                action("v_c", Some("ui_c"), &[(Keyboard, "z")]),
            ],
        )])
    }

    #[test]
    fn conflicts_skip_default_against_default() {
        let rows = rows(Some(&shared_key()), &empty_file());
        assert_eq!(conflicts(&rows), []);
    }

    #[test]
    fn conflicts_flag_an_override_onto_a_default() {
        let doc = file(&[("spaceship_movement", &[("v_c", &["kb1_X"])])]);
        let rows = rows(Some(&shared_key()), &doc);
        assert_eq!(
            conflicts(&rows),
            [Conflict {
                input: "kb1_x".into(),
                bindings: pairs(&[
                    ("spaceship_movement", "v_a"),
                    ("spaceship_movement", "v_b"),
                    ("spaceship_movement", "v_c"),
                ]),
            }]
        );
    }

    #[test]
    fn conflicts_flag_two_overrides_and_name_only_the_clashing_actions() {
        let data = game(vec![map(
            "spaceship_movement",
            vec![
                action("v_a", Some("ui_a"), &[(Joystick, "button1")]),
                action("v_b", Some("ui_b"), &[(Joystick, "button2")]),
                action("v_c", Some("ui_c"), &[(Joystick, "button3")]),
            ],
        )]);
        let doc = file(&[(
            "spaceship_movement",
            &[("v_a", &["js2_button9"]), ("v_b", &["js2_button9"])],
        )]);
        let rows = rows(Some(&data), &doc);
        assert_eq!(
            conflicts(&rows),
            [Conflict {
                input: "js2_button9".into(),
                bindings: pairs(&[("spaceship_movement", "v_a"), ("spaceship_movement", "v_b"),]),
            }]
        );
    }

    #[test]
    fn conflicts_follow_the_active_together_rule() {
        let data = game(vec![
            map(
                "spaceship_movement",
                vec![action("v_a", Some("ui_a"), &[(Keyboard, "x")])],
            ),
            map(
                "spaceship_targeting",
                vec![action("v_b", Some("ui_b"), &[(Keyboard, "z")])],
            ),
            map(
                "player",
                vec![action("walk", Some("ui_walk"), &[(Keyboard, "y")])],
            ),
        ]);
        let doc = file(&[
            ("spaceship_targeting", &[("v_b", &["kb1_x"])]),
            ("player", &[("walk", &["kb1_x"])]),
        ]);
        let rows = rows(Some(&data), &doc);
        assert_eq!(
            conflicts(&rows),
            [Conflict {
                input: "kb1_x".into(),
                bindings: pairs(&[
                    ("spaceship_movement", "v_a"),
                    ("spaceship_targeting", "v_b"),
                ]),
            }]
        );
    }

    #[test]
    fn unbound_cells_never_conflict() {
        let doc = file(&[(
            "spaceship_movement",
            &[
                ("v_a", &["kb1_ "]),
                ("v_b", &["kb1_  "]),
                ("v_c", &["kb1_ "]),
            ],
        )]);
        let rows = rows(Some(&shared_key()), &doc);
        assert_eq!(conflicts(&rows), []);
    }
}
