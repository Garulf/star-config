//! The table the screen shows: the game's actions merged with the staged document. Built
//! once per load or edit, never while drawing.

use super::NOT_IN_BUILD;
use super::inputs::{input_caps, product_name, slot_label};
use sc_core::actionmaps::{ActionMaps, DeviceKind, Slot, parse_input};
use sc_core::binds::{self, KINDS, Origin};
use sc_core::defaults::{ActionDef, GameData, humanise};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CellRef {
    pub map: String,
    pub action: String,
    pub kind: DeviceKind,
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub kind: DeviceKind,
    /// The bind in effect, staged edits included, with its device prefix.
    pub input: Option<String>,
    pub origin: Origin,
    pub default: Option<String>,
    /// How many more entries of this kind the file has for the action.
    pub extra: usize,
    pub caps: Vec<String>,
    pub default_caps: Vec<String>,
    /// Differs from the game's default, or without game data from the file as loaded.
    pub changed: bool,
    /// What the cell showed before the staged edits, when they changed it.
    pub was: Option<String>,
    pub conflict: bool,
}

impl Cell {
    pub fn slot(&self) -> Option<Slot> {
        slot_of(self.input.as_deref())
    }

    pub fn default_slot(&self) -> Option<Slot> {
        slot_of(self.default.as_deref())
    }

    /// The input after its device prefix, such as `lalt+f`.
    pub fn rest(&self) -> Option<&str> {
        let input = self.input.as_deref()?;
        Some(parse_input(input).map_or(input, |(_, rest)| rest))
    }

    /// The file holds something of this kind for the action, which Reset removes.
    pub fn resettable(&self) -> bool {
        matches!(self.origin, Origin::Override | Origin::Unbound) || self.extra > 0
    }

    pub fn unbindable(&self) -> bool {
        self.input.is_some()
    }
}

fn slot_of(input: Option<&str>) -> Option<Slot> {
    input.and_then(parse_input).map(|(slot, _)| slot)
}

#[derive(Debug, Clone)]
pub struct Row {
    pub map: String,
    pub action: String,
    pub label: String,
    pub description: Option<String>,
    pub listed: bool,
    pub in_build: bool,
    /// In the order of `binds::KINDS`.
    pub cells: Vec<Cell>,
    haystack: String,
}

impl Row {
    pub fn conflict(&self) -> bool {
        self.cells.iter().any(|cell| cell.conflict)
    }

    fn shown(&self, show_unlisted: bool) -> bool {
        self.listed || show_unlisted
    }
}

#[derive(Debug, Clone)]
pub struct MapEntry {
    pub name: String,
    pub label: String,
    /// The game's category, `None` without game data.
    pub category: Option<String>,
    listed: usize,
    all: usize,
}

impl MapEntry {
    pub fn count(&self, show_unlisted: bool) -> usize {
        if show_unlisted { self.all } else { self.listed }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Group(usize),
    Action(usize),
}

#[derive(Debug, Clone)]
pub struct Model {
    pub rows: Vec<Row>,
    pub maps: Vec<MapEntry>,
    pub devices: String,
    pub conflicts: usize,
    pub products: HashMap<Slot, String>,
    stick_binds: BTreeMap<u32, usize>,
}

type Key = (String, String);
type Before = HashMap<Key, Vec<(Option<String>, Origin)>>;

impl Model {
    pub fn build(game: Option<&GameData>, current: &ActionMaps, base: &ActionMaps) -> Self {
        let merged = binds::rows(game, current);
        let conflicts = binds::conflicts(&merged);
        let conflicted: HashSet<(String, String, String)> = conflicts
            .iter()
            .flat_map(|conflict| {
                let input = conflict.input.to_ascii_lowercase();
                conflict
                    .bindings
                    .iter()
                    .map(move |(map, action)| (map.clone(), action.clone(), input.clone()))
            })
            .collect();
        let before = before(game, base);
        let defs = definitions(game);
        let maps = map_entries(game, &merged);
        let rank: HashMap<&str, usize> = maps
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.name.as_str(), index))
            .collect();
        let mut rows: Vec<Row> = merged
            .iter()
            .map(|row| {
                let key = (row.map.clone(), row.action.clone());
                let def = defs.get(&key).copied();
                let was = before.get(&key);
                build_row(game, row, def, was, &conflicted)
            })
            .collect();
        rows.sort_by_key(|row| rank.get(row.map.as_str()).copied());
        Self {
            maps,
            devices: devices_line(current),
            conflicts: conflicts.len(),
            products: products(current),
            stick_binds: stick_binds(current),
            rows,
        }
    }

    pub fn row(&self, map: &str, action: &str) -> Option<&Row> {
        self.rows
            .iter()
            .find(|row| row.map == map && row.action == action)
    }

    pub fn cell(&self, cell: &CellRef) -> Option<&Cell> {
        let column = KINDS.iter().position(|kind| *kind == cell.kind)?;
        self.row(&cell.map, &cell.action)
            .map(|row| &row.cells[column])
    }

    pub fn map_label(&self, map: &str) -> String {
        self.maps
            .iter()
            .find(|entry| entry.name == map)
            .map_or_else(|| humanise(map), |entry| entry.label.clone())
    }

    pub fn total(&self, show_unlisted: bool) -> usize {
        self.maps
            .iter()
            .map(|entry| entry.count(show_unlisted))
            .sum()
    }

    /// Sticks the file names or binds, in order.
    pub fn joysticks(&self) -> Vec<Slot> {
        let named = self
            .products
            .keys()
            .filter(|slot| slot.kind == DeviceKind::Joystick)
            .map(|slot| slot.instance);
        named
            .chain(self.stick_binds.keys().copied())
            .collect::<BTreeSet<u32>>()
            .into_iter()
            .map(|instance| Slot {
                kind: DeviceKind::Joystick,
                instance,
            })
            .collect()
    }

    /// Binds on either joystick, the number a swap of the two rewrites.
    pub fn joystick_binds(&self, a: u32, b: u32) -> usize {
        [a, b]
            .iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|instance| self.stick_binds.get(instance))
            .sum()
    }

    /// The rows to show, with a group line before each action map when all maps are shown.
    pub fn lines(
        &self,
        map: Option<&str>,
        query: &str,
        conflicts_only: bool,
        show_unlisted: bool,
    ) -> Vec<Line> {
        let query = query.trim().to_lowercase();
        let mut lines = Vec::new();
        let mut last_map: Option<&str> = None;
        for (index, row) in self.rows.iter().enumerate() {
            let shown = row.shown(show_unlisted)
                && map.is_none_or(|name| row.map == name)
                && (query.is_empty() || row.haystack.contains(&query))
                && (!conflicts_only || row.conflict());
            if !shown {
                continue;
            }
            if map.is_none() && last_map != Some(row.map.as_str()) {
                if let Some(group) = self.maps.iter().position(|entry| entry.name == row.map) {
                    lines.push(Line::Group(group));
                }
                last_map = Some(row.map.as_str());
            }
            lines.push(Line::Action(index));
        }
        lines
    }
}

fn definitions(game: Option<&GameData>) -> HashMap<Key, &ActionDef> {
    game.into_iter()
        .flat_map(|game| &game.maps)
        .flat_map(|map| {
            map.actions
                .iter()
                .map(move |def| ((map.name.clone(), def.name.clone()), def))
        })
        .collect()
}

fn before(game: Option<&GameData>, base: &ActionMaps) -> Before {
    binds::rows(game, base)
        .into_iter()
        .map(|row| {
            let cells = row
                .cells
                .into_iter()
                .map(|cell| (cell.input, cell.origin))
                .collect();
            ((row.map, row.action), cells)
        })
        .collect()
}

/// Maps grouped by category, each category where its first map stands in the game's order.
fn map_entries(game: Option<&GameData>, rows: &[binds::Row]) -> Vec<MapEntry> {
    let mut entries: Vec<MapEntry> = Vec::new();
    for row in rows {
        let at = match entries.iter().position(|entry| entry.name == row.map) {
            Some(at) => at,
            None => {
                entries.push(map_entry(game, &row.map));
                entries.len() - 1
            }
        };
        entries[at].all += 1;
        entries[at].listed += usize::from(row.listed);
    }
    let mut categories: Vec<Option<String>> = Vec::new();
    for entry in &entries {
        if !categories.contains(&entry.category) {
            categories.push(entry.category.clone());
        }
    }
    entries.sort_by_key(|entry| categories.iter().position(|known| *known == entry.category));
    entries
}

fn map_entry(game: Option<&GameData>, name: &str) -> MapEntry {
    let (label, category) = match (game, game.and_then(|game| game.map(name))) {
        (Some(game), Some(def)) => (
            game.label(def.label.as_deref(), name),
            Some(game.label(def.category.as_deref(), "Other")),
        ),
        (Some(_), None) => (humanise(name), Some(NOT_IN_BUILD.to_string())),
        (None, _) => (humanise(name), None),
    };
    MapEntry {
        name: name.to_string(),
        label,
        category,
        listed: 0,
        all: 0,
    }
}

fn build_row(
    game: Option<&GameData>,
    row: &binds::Row,
    def: Option<&ActionDef>,
    before: Option<&Vec<(Option<String>, Origin)>>,
    conflicted: &HashSet<(String, String, String)>,
) -> Row {
    let label = match (game, def) {
        (Some(game), Some(def)) => game.label(def.label.as_deref(), &row.action),
        _ => humanise(&row.action),
    };
    let description = game
        .zip(def.and_then(|def| def.description.as_deref()))
        .map(|(game, key)| game.label(Some(key), ""))
        .filter(|text| !text.is_empty());
    let cells: Vec<Cell> = row
        .cells
        .iter()
        .enumerate()
        .map(|(column, cell)| {
            let (base_input, base_origin) = before
                .and_then(|cells| cells.get(column).cloned())
                .unwrap_or((None, Origin::DefaultUnbound));
            let conflict = cell.input.as_ref().is_some_and(|input| {
                conflicted.contains(&(
                    row.map.clone(),
                    row.action.clone(),
                    input.to_ascii_lowercase(),
                ))
            });
            let reference = if game.is_some() {
                cell.default.as_deref()
            } else {
                base_input.as_deref()
            };
            Cell {
                kind: cell.kind,
                caps: caps(game, cell.input.as_deref()),
                default_caps: caps(game, cell.default.as_deref()),
                changed: !same(cell.input.as_deref(), reference),
                was: (!same(cell.input.as_deref(), base_input.as_deref()))
                    .then(|| was_text(game, base_input.as_deref(), base_origin)),
                input: cell.input.clone(),
                origin: cell.origin,
                default: cell.default.clone(),
                extra: cell.extra.len(),
                conflict,
            }
        })
        .collect();
    let haystack = haystack(&row.map, &row.action, &label, &cells);
    Row {
        map: row.map.clone(),
        action: row.action.clone(),
        label,
        description,
        listed: row.listed,
        in_build: row.in_build,
        cells,
        haystack,
    }
}

fn same(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        (None, None) => true,
        _ => false,
    }
}

fn caps(game: Option<&GameData>, input: Option<&str>) -> Vec<String> {
    input
        .map(|input| input_caps(game, input))
        .unwrap_or_default()
}

fn was_text(game: Option<&GameData>, base: Option<&str>, origin: Origin) -> String {
    match (base, origin) {
        (Some(input), _) => format!("Was {}", input_caps(game, input).join(" + ")),
        (None, Origin::Unbound) => "Was unbound".to_string(),
        (None, _) => "Was not bound".to_string(),
    }
}

fn products(current: &ActionMaps) -> HashMap<Slot, String> {
    current
        .devices()
        .iter()
        .filter_map(|device| {
            let product = product_name(device.product.as_deref()?);
            (!product.is_empty()).then_some((device.slot, product))
        })
        .collect()
}

fn stick_binds(current: &ActionMaps) -> BTreeMap<u32, usize> {
    let mut counts = BTreeMap::new();
    for binding in current.bindings() {
        let Some((slot, rest)) = parse_input(&binding.input) else {
            continue;
        };
        if slot.kind == DeviceKind::Joystick && !rest.trim().is_empty() {
            *counts.entry(slot.instance).or_insert(0) += 1;
        }
    }
    counts
}

fn devices_line(current: &ActionMaps) -> String {
    let mut devices: Vec<_> = current.devices().iter().collect();
    devices.sort_by_key(|device| device.slot);
    if devices.is_empty() {
        let bound: BTreeSet<Slot> = current
            .bindings()
            .iter()
            .filter_map(|binding| binding.slot)
            .collect();
        return bound
            .into_iter()
            .map(slot_label)
            .collect::<Vec<_>>()
            .join(", ");
    }
    devices
        .iter()
        .map(|device| {
            let label = slot_label(device.slot);
            let named = matches!(device.slot.kind, DeviceKind::Joystick | DeviceKind::Gamepad);
            match device.product.as_deref().map(product_name) {
                Some(product) if named && !product.is_empty() => format!("{label}: {product}"),
                _ => label,
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn haystack(map: &str, action: &str, label: &str, cells: &[Cell]) -> String {
    let mut parts = vec![
        map.to_string(),
        humanise(map),
        action.to_string(),
        label.to_string(),
    ];
    for cell in cells {
        if let Some(input) = &cell.input {
            parts.push(input.clone());
            parts.push(cell.rest().unwrap_or_default().to_string());
            parts.push(cell.caps.join(" + "));
        }
    }
    parts.join("\n").to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::defaults::Sources;
    use sc_core::staging::Document;
    use std::path::Path;

    const FILE: &str = r#"<ActionMaps>
 <ActionProfiles profileName="default">
  <options type="keyboard" instance="1" Product="Keyboard  {6F1D2B61-D5A0-11CF-BFC7-444553540000}"/>
  <options type="joystick" instance="2" Product="VKB Gladiator NXT R  {0201231D-0000-0000-0000-504944564944}"/>
  <options type="joystick" instance="1" Product="VKB Gladiator NXT L  {0200231D-0000-0000-0000-504944564944}"/>
  <actionmap name="spaceship_movement">
   <action name="v_strafe_up">
    <rebind input="kb1_space"/>
    <rebind input="js2_button5"/>
   </action>
  </actionmap>
  <actionmap name="spaceship_weapons">
   <action name="v_attack1_group1">
    <rebind input="js2_button5"/>
    <rebind input="mo1_mouse1"/>
   </action>
  </actionmap>
 </ActionProfiles>
</ActionMaps>
"#;

    const PROFILE: &str = r#"<profile>
 <actionmap name="spaceship_weapons" UILabel="@ui_weapons" UICategory="@ui_ship">
  <action name="v_attack1_group1" mouse="mouse1" joystick="button1" UILabel="@ui_fire"/>
  <action name="v_hidden" keyboard="f12"/>
 </actionmap>
 <actionmap name="spaceship_movement" UILabel="@ui_flight" UICategory="@ui_ship">
  <action name="v_strafe_up" keyboard="space" UILabel="@ui_up"/>
  <action name="v_boost" keyboard="lshift" UILabel="@ui_boost"/>
 </actionmap>
 <actionmap name="player" UILabel="@ui_player" UICategory="@ui_foot">
  <action name="jump" keyboard="space" UILabel="@ui_jump"/>
 </actionmap>
</profile>
"#;

    const STRINGS: &str = "ui_weapons=Weapons\nui_flight=Flight\nui_ship=Ship\nui_fire=Fire\nui_up=Up\nui_boost=Boost\nui_player=Character\nui_foot=On foot\nui_jump=Jump\n";

    fn doc(text: &str) -> ActionMaps {
        ActionMaps::parse(Path::new("actionmaps.xml"), text.as_bytes()).unwrap()
    }

    fn game() -> GameData {
        GameData::from_sources(
            "test".into(),
            &Sources {
                profile: PROFILE.into(),
                attributes: "<Attributes/>".into(),
                key_names: "<LocalizedKeys/>".into(),
                strings: STRINGS.into(),
            },
        )
        .unwrap()
    }

    fn cell<'a>(model: &'a Model, map: &str, action: &str, kind: DeviceKind) -> &'a Cell {
        model
            .cell(&CellRef {
                map: map.into(),
                action: action.into(),
                kind,
            })
            .unwrap()
    }

    #[test]
    fn without_game_data_rows_conflicts_and_devices_come_from_the_file() {
        let model = Model::build(None, &doc(FILE), &doc(FILE));
        assert_eq!(
            model.devices,
            "Keyboard, Joystick 1: VKB Gladiator NXT L, Joystick 2: VKB Gladiator NXT R"
        );
        assert_eq!(model.rows.len(), 2);
        assert_eq!(model.conflicts, 1);
        let stick = cell(
            &model,
            "spaceship_movement",
            "v_strafe_up",
            DeviceKind::Joystick,
        );
        assert!(stick.conflict);
        assert!(
            !stick.changed,
            "nothing staged and no defaults to differ from"
        );
        assert_eq!(model.total(false), 2);
        assert_eq!(model.joystick_binds(1, 2), 2);
        assert_eq!(
            model.joysticks(),
            [1, 2].map(|instance| Slot {
                kind: DeviceKind::Joystick,
                instance
            })
        );
    }

    #[test]
    fn with_game_data_maps_follow_their_category_and_the_changed_marker_follows_the_default() {
        let game = game();
        let model = Model::build(Some(&game), &doc(FILE), &doc(FILE));
        let maps: Vec<(&str, Option<&str>)> = model
            .maps
            .iter()
            .map(|entry| (entry.label.as_str(), entry.category.as_deref()))
            .collect();
        assert_eq!(
            maps,
            [
                ("Weapons", Some("Ship")),
                ("Flight", Some("Ship")),
                ("Character", Some("On foot"))
            ]
        );
        let up = cell(
            &model,
            "spaceship_movement",
            "v_strafe_up",
            DeviceKind::Keyboard,
        );
        assert_eq!(up.origin, Origin::Override);
        assert!(!up.changed, "an override equal to its default");
        let fire = cell(
            &model,
            "spaceship_weapons",
            "v_attack1_group1",
            DeviceKind::Joystick,
        );
        assert!(fire.changed);
        assert_eq!(fire.default.as_deref(), Some("js1_button1"));
        let boost = cell(
            &model,
            "spaceship_movement",
            "v_boost",
            DeviceKind::Keyboard,
        );
        assert_eq!(boost.origin, Origin::Default);
        assert!(!boost.changed);
        assert!(!boost.resettable());
        assert!(boost.unbindable());
        assert_eq!(model.total(false), 4);
        assert_eq!(model.total(true), 5);
    }

    #[test]
    fn rows_are_grouped_like_the_map_list_and_lines_filter() {
        let game = game();
        let model = Model::build(Some(&game), &doc(FILE), &doc(FILE));
        let names: Vec<&str> = model.rows.iter().map(|row| row.action.as_str()).collect();
        assert_eq!(
            names,
            [
                "v_attack1_group1",
                "v_hidden",
                "v_strafe_up",
                "v_boost",
                "jump"
            ]
        );
        assert_eq!(
            model.lines(None, "", false, false),
            [
                Line::Group(0),
                Line::Action(0),
                Line::Group(1),
                Line::Action(2),
                Line::Action(3),
                Line::Group(2),
                Line::Action(4)
            ]
        );
        assert_eq!(
            model.lines(Some("spaceship_weapons"), "", false, true),
            [Line::Action(0), Line::Action(1)]
        );
        assert_eq!(model.lines(None, "lshift", false, false).len(), 2);
        assert_eq!(model.lines(None, "left click", false, false).len(), 2);
    }

    #[test]
    fn staged_edits_say_what_the_cell_was() {
        let base = doc(FILE);
        let mut current = base.clone();
        current
            .apply(&sc_core::actionmaps::Op::Unbind {
                map: "spaceship_movement".into(),
                action: "v_boost".into(),
                slot: Slot {
                    kind: DeviceKind::Keyboard,
                    instance: 1,
                },
            })
            .unwrap();
        let game = game();
        let model = Model::build(Some(&game), &current, &base);
        let boost = cell(
            &model,
            "spaceship_movement",
            "v_boost",
            DeviceKind::Keyboard,
        );
        assert_eq!(boost.origin, Origin::Unbound);
        assert!(boost.changed);
        assert!(boost.resettable());
        assert!(!boost.unbindable());
        assert_eq!(boost.was.as_deref(), Some("Was L Shift"));
        let up = cell(
            &model,
            "spaceship_movement",
            "v_strafe_up",
            DeviceKind::Keyboard,
        );
        assert_eq!(up.was, None);
    }
}
