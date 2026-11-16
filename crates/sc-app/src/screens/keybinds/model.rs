//! The table the screen shows, built from the staged document and the file as loaded.

use super::inputs::{caps, humanise, product_name, slot_label};
use sc_core::actionmaps::{ActionMaps, DeviceKind, Slot, parse_input};
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CellRef {
    pub map: String,
    pub action: String,
    pub slot: Slot,
}

#[derive(Debug, Clone)]
pub struct Cell {
    /// The whole input as the file has it now, staged edits included.
    pub current: Option<String>,
    pub base: Option<String>,
    pub caps: Vec<String>,
    pub was: String,
    pub conflict: bool,
}

impl Cell {
    pub fn changed(&self) -> bool {
        self.current != self.base
    }

    /// The input after its device prefix, such as `lalt+f`.
    pub fn rest(&self) -> Option<&str> {
        let current = self.current.as_deref()?;
        Some(parse_input(current).map_or(current, |(_, rest)| rest))
    }
}

#[derive(Debug, Clone)]
pub struct Row {
    pub map: String,
    pub action: String,
    pub label: String,
    pub cells: Vec<Cell>,
    haystack: String,
}

impl Row {
    pub fn conflict(&self) -> bool {
        self.cells.iter().any(|cell| cell.conflict)
    }
}

#[derive(Debug, Clone)]
pub struct MapEntry {
    pub name: String,
    pub label: String,
    pub count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Group(usize),
    Action(usize),
}

#[derive(Debug, Clone)]
pub struct Model {
    pub columns: Vec<Slot>,
    pub column_labels: Vec<String>,
    pub rows: Vec<Row>,
    pub maps: Vec<MapEntry>,
    pub total: usize,
    pub devices: String,
    pub conflicts: usize,
    pub products: HashMap<Slot, String>,
}

type Inputs = HashMap<(String, String, Slot), String>;

fn inputs_of(doc: &ActionMaps) -> Inputs {
    let mut inputs = Inputs::new();
    for binding in doc.bindings() {
        if let Some(slot) = binding.slot {
            inputs
                .entry((binding.map.clone(), binding.action.clone(), slot))
                .or_insert_with(|| binding.input.clone());
        }
    }
    inputs
}

fn map_order(current: &ActionMaps, base: &ActionMaps) -> Vec<String> {
    let mut order: Vec<String> = Vec::new();
    for name in current.maps().into_iter().chain(base.maps()) {
        if !order.iter().any(|known| known == name) {
            order.push(name.to_string());
        }
    }
    order
}

fn action_order(current: &ActionMaps, base: &ActionMaps, maps: &[String]) -> Vec<(String, String)> {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut order: Vec<(String, String)> = Vec::new();
    for binding in current.bindings().iter().chain(base.bindings()) {
        if binding.slot.is_none() {
            continue;
        }
        let key = (binding.map.clone(), binding.action.clone());
        if seen.insert(key.clone()) {
            order.push(key);
        }
    }
    let rank = |map: &str| maps.iter().position(|known| known == map);
    order.sort_by_key(|(map, _)| rank(map));
    order
}

fn columns_of(current: &ActionMaps, base: &ActionMaps) -> Vec<Slot> {
    let declared = current.devices().iter().map(|device| device.slot);
    let bound = current
        .bindings()
        .iter()
        .chain(base.bindings())
        .filter_map(|binding| binding.slot);
    declared
        .chain(bound)
        .collect::<BTreeSet<Slot>>()
        .into_iter()
        .collect()
}

fn conflicting(current: &ActionMaps) -> (HashSet<(String, String, String)>, usize) {
    let conflicts = current.conflicts();
    let set = conflicts
        .iter()
        .flat_map(|conflict| {
            let input = conflict.input.to_ascii_lowercase();
            conflict
                .bindings
                .iter()
                .map(move |(map, action)| (map.clone(), action.clone(), input.clone()))
        })
        .collect();
    (set, conflicts.len())
}

fn caps_of(input: Option<&str>) -> Vec<String> {
    match input.and_then(parse_input) {
        Some((slot, rest)) => caps(slot.kind, rest),
        None => input.map(|raw| vec![raw.to_string()]).unwrap_or_default(),
    }
}

fn was_text(base: Option<&str>) -> String {
    match base {
        Some(_) => format!("Was {}", caps_of(base).join(" + ")),
        None => "Was not set".to_string(),
    }
}

fn devices_line(current: &ActionMaps, columns: &[Slot]) -> String {
    let mut devices: Vec<_> = current.devices().iter().collect();
    devices.sort_by_key(|device| device.slot);
    if devices.is_empty() {
        return columns
            .iter()
            .map(|slot| slot_label(*slot))
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

impl Model {
    pub fn build(current: &ActionMaps, base: &ActionMaps) -> Self {
        let columns = columns_of(current, base);
        let maps = map_order(current, base);
        let now = inputs_of(current);
        let then = inputs_of(base);
        let (conflicted, conflicts) = conflicting(current);

        let rows: Vec<Row> = action_order(current, base, &maps)
            .into_iter()
            .map(|(map, action)| {
                let cells: Vec<Cell> = columns
                    .iter()
                    .map(|slot| {
                        let key = (map.clone(), action.clone(), *slot);
                        let current = now.get(&key).cloned();
                        let base = then.get(&key).cloned();
                        let conflict = current.as_ref().is_some_and(|input| {
                            conflicted.contains(&(
                                map.clone(),
                                action.clone(),
                                input.to_ascii_lowercase(),
                            ))
                        });
                        Cell {
                            caps: caps_of(current.as_deref()),
                            was: was_text(base.as_deref()),
                            current,
                            base,
                            conflict,
                        }
                    })
                    .collect();
                let label = humanise(&action);
                let haystack = haystack(&map, &action, &label, &cells);
                Row {
                    map,
                    action,
                    label,
                    cells,
                    haystack,
                }
            })
            .collect();

        let maps: Vec<MapEntry> = maps
            .into_iter()
            .map(|name| MapEntry {
                count: rows
                    .iter()
                    .filter(|row| row.map == name)
                    .map(|row| row.cells.iter().filter(|c| c.current.is_some()).count())
                    .sum(),
                label: humanise(&name),
                name,
            })
            .filter(|entry| rows.iter().any(|row| row.map == entry.name))
            .collect();
        let products = current
            .devices()
            .iter()
            .filter_map(|device| {
                let product = product_name(device.product.as_deref()?);
                (!product.is_empty()).then_some((device.slot, product))
            })
            .collect();
        Self {
            column_labels: columns.iter().map(|slot| slot_label(*slot)).collect(),
            devices: devices_line(current, &columns),
            total: maps.iter().map(|entry| entry.count).sum(),
            columns,
            rows,
            maps,
            conflicts,
            products,
        }
    }

    pub fn column(&self, slot: Slot) -> Option<usize> {
        self.columns.iter().position(|known| *known == slot)
    }

    pub fn row(&self, map: &str, action: &str) -> Option<&Row> {
        self.rows
            .iter()
            .find(|row| row.map == map && row.action == action)
    }

    pub fn cell(&self, cell: &CellRef) -> Option<&Cell> {
        let column = self.column(cell.slot)?;
        self.row(&cell.map, &cell.action)
            .map(|row| &row.cells[column])
    }

    pub fn map_label(&self, map: &str) -> String {
        self.maps
            .iter()
            .find(|entry| entry.name == map)
            .map_or_else(|| humanise(map), |entry| entry.label.clone())
    }

    pub fn joysticks(&self) -> Vec<Slot> {
        self.columns
            .iter()
            .copied()
            .filter(|slot| slot.kind == DeviceKind::Joystick)
            .collect()
    }

    /// Binds on either joystick, the number a swap of the two rewrites.
    pub fn joystick_binds(&self, a: u32, b: u32) -> usize {
        self.columns
            .iter()
            .enumerate()
            .filter(|(_, slot)| {
                slot.kind == DeviceKind::Joystick && (slot.instance == a || slot.instance == b)
            })
            .map(|(column, _)| {
                self.rows
                    .iter()
                    .filter(|row| row.cells[column].current.is_some())
                    .count()
            })
            .sum()
    }

    /// The rows to show, with a group line before each action map when all maps are shown.
    pub fn lines(&self, map: Option<&str>, query: &str, conflicts_only: bool) -> Vec<Line> {
        let query = query.trim().to_lowercase();
        let mut lines = Vec::new();
        let mut last_map: Option<&str> = None;
        for (index, row) in self.rows.iter().enumerate() {
            let shown = map.is_none_or(|name| row.map == name)
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

fn haystack(map: &str, action: &str, label: &str, cells: &[Cell]) -> String {
    let mut parts = vec![
        map.to_string(),
        humanise(map),
        action.to_string(),
        label.to_string(),
    ];
    for cell in cells {
        if let Some(input) = &cell.current {
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

    fn doc() -> ActionMaps {
        ActionMaps::parse(Path::new("actionmaps.xml"), FILE.as_bytes()).unwrap()
    }

    #[test]
    fn columns_rows_conflicts_and_devices() {
        let model = Model::build(&doc(), &doc());
        assert_eq!(
            model.column_labels,
            ["Keyboard", "Mouse", "Joystick 1", "Joystick 2"]
        );
        assert_eq!(
            model.devices,
            "Keyboard, Joystick 1: VKB Gladiator NXT L, Joystick 2: VKB Gladiator NXT R"
        );
        assert_eq!(model.rows.len(), 2);
        assert_eq!(model.conflicts, 1);
        assert!(model.rows[0].cells[3].conflict);
        assert!(!model.rows[0].cells[0].conflict);
        assert_eq!(model.total, 4);
        assert_eq!(model.joystick_binds(1, 2), 2);
    }

    #[test]
    fn lines_group_by_map_and_filter() {
        let model = Model::build(&doc(), &doc());
        assert_eq!(
            model.lines(None, "", false),
            [
                Line::Group(0),
                Line::Action(0),
                Line::Group(1),
                Line::Action(1)
            ]
        );
        assert_eq!(
            model.lines(Some("spaceship_weapons"), "", false),
            [Line::Action(1)]
        );
        assert_eq!(
            model.lines(None, "kb1_space", false),
            [Line::Group(0), Line::Action(0)]
        );
        assert_eq!(model.lines(None, "left click", false).len(), 2);
    }
}
