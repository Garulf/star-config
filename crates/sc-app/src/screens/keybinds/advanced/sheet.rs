//! What the Advanced controls view shows for one device, built once per load or edit.

use super::super::inputs::{input_caps, product_name};
use sc_core::actionmaps::{ActionMaps, AxisField, DeviceKind, DeviceOptions, GroupSetting, Slot};
use sc_core::defaults::{GameData, OptionGroup, OptionTree, humanise};

pub const JOYSTICK_SLOTS: u32 = 8;
pub const AXIS_FIELDS: [AxisField; 3] = [
    AxisField::Deadzone,
    AxisField::Saturation,
    AxisField::Acceleration,
];
/// `deviceoptions` names the game gives the system keyboard and mouse, without a GUID.
const SYSTEM_DEVICES: [&str; 2] = ["Keyboard", "Mouse"];

pub type Points = Vec<(String, String)>;

/// An entry of the device list. `Unmapped` is a `deviceoptions` product no slot names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevicePick {
    KeyboardMouse,
    Gamepad,
    Joystick(u32),
    Unmapped(String),
}

impl DevicePick {
    /// The `options` slot holding this device's groups. The game keeps mouse groups in the
    /// keyboard slot.
    pub fn slot(&self) -> Option<Slot> {
        let (kind, instance) = match self {
            Self::KeyboardMouse => (DeviceKind::Keyboard, 1),
            Self::Gamepad => (DeviceKind::Gamepad, 1),
            Self::Joystick(instance) => (DeviceKind::Joystick, *instance),
            Self::Unmapped(_) => return None,
        };
        Some(Slot { kind, instance })
    }

    /// Every file slot whose product belongs to this device.
    fn slots(&self) -> Vec<Slot> {
        let mut slots: Vec<Slot> = self.slot().into_iter().collect();
        if *self == Self::KeyboardMouse {
            slots.push(Slot {
                kind: DeviceKind::Mouse,
                instance: 1,
            });
        }
        slots
    }

    pub fn title(&self) -> String {
        match self {
            Self::KeyboardMouse => "Keyboard and mouse".into(),
            Self::Gamepad => "Gamepad".into(),
            Self::Joystick(instance) => format!("Joystick {instance}"),
            Self::Unmapped(product) => unmapped_name(product),
        }
    }

    /// The device prefix the game writes before this device's inputs.
    fn input_prefix(&self) -> String {
        match self {
            Self::KeyboardMouse => "mo1".into(),
            Self::Gamepad => "gp1".into(),
            Self::Joystick(instance) => format!("js{instance}"),
            Self::Unmapped(_) => "js1".into(),
        }
    }
}

/// One value as the staged file has it, as the file on disk had it, and the game's default.
#[derive(Debug, Clone, PartialEq)]
pub struct Setting<T> {
    pub now: Option<T>,
    pub was: Option<T>,
    pub default: Option<T>,
}

impl<T: PartialEq> Setting<T> {
    pub fn staged(&self) -> bool {
        self.now != self.was
    }

    /// The file overrides the game here, or a staged edit changed it.
    pub fn changed(&self) -> bool {
        self.now.is_some() || self.staged()
    }

    pub fn effective(&self) -> Option<&T> {
        self.now.as_ref().or(self.default.as_ref())
    }

    fn shown(&self) -> bool {
        self.now.is_some() || self.was.is_some()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupLine {
    pub name: String,
    pub label: String,
    pub depth: usize,
    pub in_build: bool,
    pub invert: Option<Setting<bool>>,
    pub exponent: Option<Setting<String>>,
    pub sensitivity: Option<Setting<String>>,
    pub curve: Option<Setting<Points>>,
}

impl GroupLine {
    pub fn has_controls(&self) -> bool {
        self.invert.is_some()
            || self.exponent.is_some()
            || self.sensitivity.is_some()
            || self.curve.is_some()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AxisRow {
    pub input: String,
    pub label: String,
    /// One per [`AXIS_FIELDS`], in that order.
    pub values: Vec<Setting<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AxisBlock {
    pub product: String,
    pub title: String,
    pub rows: Vec<AxisRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceEntry {
    pub pick: DevicePick,
    pub label: String,
    pub product: Option<String>,
    /// How many values the file sets for this device.
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    pub devices: Vec<DeviceEntry>,
    pub unmapped: Vec<DeviceEntry>,
    pub groups: Vec<GroupLine>,
    pub blocks: Vec<AxisBlock>,
    pub sensitivity: Option<(f64, f64)>,
}

/// The two copies of the file a sheet compares: staged and on disk.
#[derive(Clone, Copy)]
pub struct Files<'a> {
    pub now: &'a ActionMaps,
    pub was: &'a ActionMaps,
}

impl Sheet {
    pub fn build(game: Option<&GameData>, files: Files<'_>, pick: &DevicePick) -> Self {
        let tree = game.and_then(|game| tree_for(game, pick));
        let groups = match (game, tree) {
            (Some(game), Some(tree)) => tree_lines(game, tree, files, pick),
            _ if pick.slot().is_some() => file_lines(files, pick, |_| true, 0),
            _ => Vec::new(),
        };
        Self {
            devices: device_entries(files.now),
            unmapped: unmapped_entries(files.now),
            groups,
            blocks: axis_blocks(game, files, pick),
            sensitivity: tree.and_then(|tree| tree.sensitivity),
        }
    }

    pub fn entry(&self, pick: &DevicePick) -> Option<&DeviceEntry> {
        self.devices
            .iter()
            .chain(&self.unmapped)
            .find(|entry| entry.pick == *pick)
    }

    pub fn group(&self, name: &str) -> Option<&GroupLine> {
        self.groups.iter().find(|line| line.name == name)
    }

    pub fn axis(&self, product: &str, input: &str, field: AxisField) -> Option<&Setting<String>> {
        let at = AXIS_FIELDS.iter().position(|f| *f == field)?;
        self.blocks
            .iter()
            .filter(|block| block.product == product)
            .flat_map(|block| &block.rows)
            .find(|row| row.input == input)
            .map(|row| &row.values[at])
    }
}

fn tree_for<'a>(game: &'a GameData, pick: &DevicePick) -> Option<&'a OptionTree> {
    let kind = pick.slot()?.kind;
    game.option_trees.iter().find(|tree| tree.kind == kind)
}

fn device_entries(file: &ActionMaps) -> Vec<DeviceEntry> {
    let mut picks = vec![DevicePick::KeyboardMouse, DevicePick::Gamepad];
    picks.extend((1..=JOYSTICK_SLOTS).map(DevicePick::Joystick));
    picks
        .into_iter()
        .map(|pick| {
            let product = match pick {
                DevicePick::Joystick(_) => slot_products(file, &pick)
                    .into_iter()
                    .map(product_name)
                    .find(|name| !name.is_empty()),
                _ => None,
            };
            DeviceEntry {
                label: pick.title(),
                product,
                count: overrides(file, &pick),
                pick,
            }
        })
        .collect()
}

fn unmapped_entries(file: &ActionMaps) -> Vec<DeviceEntry> {
    file.device_options()
        .iter()
        .filter(|block| !mapped(file, &block.product))
        .map(|block| {
            let pick = DevicePick::Unmapped(block.product.clone());
            DeviceEntry {
                label: pick.title(),
                product: None,
                count: overrides(file, &pick),
                pick,
            }
        })
        .collect()
}

fn unmapped_name(product: &str) -> String {
    let name = product_name(product);
    if name.is_empty() {
        product.trim().to_string()
    } else {
        name
    }
}

fn mapped(file: &ActionMaps, block: &str) -> bool {
    let mut picks = vec![DevicePick::KeyboardMouse, DevicePick::Gamepad];
    picks.extend((1..=JOYSTICK_SLOTS).map(DevicePick::Joystick));
    picks.iter().any(|pick| belongs(file, pick, block))
}

fn slot_products<'a>(file: &'a ActionMaps, pick: &DevicePick) -> Vec<&'a str> {
    let slots = pick.slots();
    file.devices()
        .iter()
        .filter(|device| slots.contains(&device.slot))
        .filter_map(|device| device.product.as_deref())
        .collect()
}

/// Whether a `deviceoptions` block belongs to the device. The block and the slot often name
/// the product differently, so the GUID decides when both carry one.
fn belongs(file: &ActionMaps, pick: &DevicePick, block: &str) -> bool {
    if let DevicePick::Unmapped(product) = pick {
        return product == block;
    }
    if *pick == DevicePick::KeyboardMouse
        && SYSTEM_DEVICES
            .iter()
            .any(|name| product_name(block).eq_ignore_ascii_case(name))
    {
        return true;
    }
    slot_products(file, pick)
        .into_iter()
        .any(|product| same_device(block, product))
}

pub fn same_device(a: &str, b: &str) -> bool {
    match (guid(a), guid(b)) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => {
            let (a, b) = (product_name(a), product_name(b));
            !a.is_empty() && a.eq_ignore_ascii_case(&b)
        }
    }
}

fn guid(product: &str) -> Option<&str> {
    let start = product.find('{')? + 1;
    let end = start + product[start..].find('}')?;
    Some(product[start..end].trim()).filter(|guid| !guid.is_empty())
}

fn overrides(file: &ActionMaps, pick: &DevicePick) -> usize {
    let groups: usize = file_groups(file, pick)
        .iter()
        .map(|group| {
            usize::from(group.invert.is_some())
                + usize::from(group.exponent.is_some())
                + usize::from(group.sensitivity.is_some())
                + usize::from(group.curve.is_some())
        })
        .sum();
    let axes: usize = blocks_of(file, pick)
        .iter()
        .map(|block| block.axes.len())
        .sum();
    groups + axes
}

fn file_groups<'a>(file: &'a ActionMaps, pick: &DevicePick) -> Vec<&'a GroupSetting> {
    let slots = pick.slots();
    let mut found: Vec<&GroupSetting> = Vec::new();
    for options in file.group_options() {
        let slot = Slot {
            kind: options.kind,
            instance: options.instance,
        };
        if !slots.contains(&slot) {
            continue;
        }
        for group in &options.groups {
            if !found.iter().any(|seen| seen.group == group.group) {
                found.push(group);
            }
        }
    }
    found
}

fn file_group<'a>(file: &'a ActionMaps, pick: &DevicePick, name: &str) -> Option<&'a GroupSetting> {
    file_groups(file, pick)
        .into_iter()
        .find(|group| group.group == name)
}

fn setting<T: Clone>(
    files: Files<'_>,
    pick: &DevicePick,
    name: &str,
    read: impl Fn(&GroupSetting) -> Option<T>,
    default: Option<T>,
) -> Setting<T> {
    Setting {
        now: file_group(files.now, pick, name).and_then(&read),
        was: file_group(files.was, pick, name).and_then(&read),
        default,
    }
}

/// The flag shows the control; a value the file holds is shown even where the flag does not.
fn when<T: PartialEq>(flag: i8, value: Setting<T>) -> Option<Setting<T>> {
    (flag == 1 || value.shown()).then_some(value)
}

fn tree_lines(
    game: &GameData,
    tree: &OptionTree,
    files: Files<'_>,
    pick: &DevicePick,
) -> Vec<GroupLine> {
    let mut lines: Vec<GroupLine> = tree
        .groups
        .iter()
        .enumerate()
        .map(|(index, group)| GroupLine {
            name: group.name.clone(),
            label: game.label(group.label.as_deref(), &group.name),
            depth: depth(&tree.groups, index),
            in_build: true,
            ..controls(files, pick, group)
        })
        .collect();
    let known = |name: &str| tree.groups.iter().any(|group| group.name == name);
    lines.extend(file_lines(files, pick, |name| !known(name), 0));
    for line in lines.iter_mut().filter(|line| !known(&line.name)) {
        line.in_build = false;
    }
    lines
}

fn controls(files: Files<'_>, pick: &DevicePick, group: &OptionGroup) -> GroupLine {
    let name = group.name.as_str();
    let curve = (!group.curve.is_empty()).then(|| {
        group
            .curve
            .iter()
            .map(|&(x, y)| (number(x), number(y)))
            .collect()
    });
    GroupLine {
        name: name.to_string(),
        label: String::new(),
        depth: 0,
        in_build: true,
        invert: when(
            group.show_invert,
            setting(files, pick, name, |g| g.invert, group.invert),
        ),
        exponent: when(
            group.show_curve,
            setting(
                files,
                pick,
                name,
                |g| g.exponent.clone(),
                group.exponent.map(number),
            ),
        ),
        sensitivity: when(
            group.show_sensitivity,
            setting(files, pick, name, |g| g.sensitivity.clone(), None),
        ),
        curve: when(
            group.show_curve,
            setting(files, pick, name, |g| g.curve.clone(), curve),
        ),
    }
}

/// Lines for the groups the file holds, now or on disk, labelled from their names.
fn file_lines(
    files: Files<'_>,
    pick: &DevicePick,
    keep: impl Fn(&str) -> bool,
    depth: usize,
) -> Vec<GroupLine> {
    let mut names: Vec<&str> = Vec::new();
    for group in file_groups(files.now, pick)
        .into_iter()
        .chain(file_groups(files.was, pick))
    {
        if keep(&group.group) && !names.contains(&group.group.as_str()) {
            names.push(&group.group);
        }
    }
    names
        .into_iter()
        .map(|name| GroupLine {
            name: name.to_string(),
            label: humanise(name),
            depth,
            in_build: true,
            invert: when(0, setting(files, pick, name, |g| g.invert, None)),
            exponent: when(0, setting(files, pick, name, |g| g.exponent.clone(), None)),
            sensitivity: when(
                0,
                setting(files, pick, name, |g| g.sensitivity.clone(), None),
            ),
            curve: when(0, setting(files, pick, name, |g| g.curve.clone(), None)),
        })
        .collect()
}

fn depth(groups: &[OptionGroup], index: usize) -> usize {
    let mut depth = 0;
    let mut parent = groups[index].parent;
    while let Some(at) = parent.filter(|_| depth < groups.len()) {
        depth += 1;
        parent = groups[at].parent;
    }
    depth
}

fn blocks_of<'a>(file: &'a ActionMaps, pick: &DevicePick) -> Vec<&'a DeviceOptions> {
    file.device_options()
        .iter()
        .filter(|block| belongs(file, pick, &block.product))
        .collect()
}

fn axis_blocks(game: Option<&GameData>, files: Files<'_>, pick: &DevicePick) -> Vec<AxisBlock> {
    let mut products: Vec<&str> = Vec::new();
    for block in blocks_of(files.now, pick)
        .into_iter()
        .chain(blocks_of(files.was, pick))
    {
        if !products.contains(&block.product.as_str()) {
            products.push(&block.product);
        }
    }
    products
        .into_iter()
        .map(|product| AxisBlock {
            product: product.to_string(),
            title: unmapped_name(product),
            rows: axis_rows(game, files, pick, product),
        })
        .collect()
}

fn axis_rows(
    game: Option<&GameData>,
    files: Files<'_>,
    pick: &DevicePick,
    product: &str,
) -> Vec<AxisRow> {
    let lines = |file: &ActionMaps| -> Vec<(String, AxisField, String)> {
        file.device_options()
            .iter()
            .filter(|block| block.product == product)
            .flat_map(|block| &block.axes)
            .map(|axis| (axis.input.clone(), axis.field, axis.value.clone()))
            .collect()
    };
    let (now, was) = (lines(files.now), lines(files.was));
    let mut inputs: Vec<&str> = Vec::new();
    for (input, _, _) in now.iter().chain(&was) {
        if !inputs.contains(&input.as_str()) {
            inputs.push(input);
        }
    }
    let value = |lines: &[(String, AxisField, String)], input: &str, field: AxisField| {
        lines
            .iter()
            .find(|(i, f, _)| i == input && *f == field)
            .map(|(_, _, value)| value.clone())
    };
    inputs
        .into_iter()
        .map(|input| AxisRow {
            input: input.to_string(),
            label: axis_label(game, pick, input),
            values: AXIS_FIELDS
                .iter()
                .map(|&field| Setting {
                    now: value(&now, input, field),
                    was: value(&was, input, field),
                    default: None,
                })
                .collect(),
        })
        .collect()
}

fn axis_label(game: Option<&GameData>, pick: &DevicePick, input: &str) -> String {
    if let Some(key) = input.strip_prefix('@') {
        return match game {
            Some(game) => game.label(Some(input), key),
            None => humanise(key),
        };
    }
    input_caps(game, &format!("{}_{input}", pick.input_prefix())).join(" + ")
}

/// A value as the file writes it: at most three decimals, no trailing zeros.
pub fn number(value: f64) -> String {
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".into()
    } else {
        text.into()
    }
}

/// Points as numbers when every one is a number from 0 to 1.
pub fn parse_points(points: &[(String, String)]) -> Option<Vec<(f64, f64)>> {
    points
        .iter()
        .map(|(x, y)| Some((unit(x)?, unit(y)?)))
        .collect()
}

fn unit(text: &str) -> Option<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| (0.0..=1.0).contains(value))
}

/// The curve from (0, 0) through the points to (1, 1), in input order.
fn polyline(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut line = vec![(0.0, 0.0)];
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    line.extend(sorted);
    line.push((1.0, 1.0));
    line
}

fn output_at(line: &[(f64, f64)], x: f64) -> f64 {
    let after = line
        .iter()
        .position(|&(px, _)| px >= x)
        .unwrap_or(line.len() - 1);
    if after == 0 {
        return line[0].1;
    }
    let ((x0, y0), (x1, y1)) = (line[after - 1], line[after]);
    if x1 <= x0 {
        return y1;
    }
    y0 + (y1 - y0) * (x - x0) / (x1 - x0)
}

/// A point in the middle of the widest gap, on the curve as it stands.
pub fn new_point(points: &[(f64, f64)]) -> (f64, f64) {
    let line = polyline(points);
    let (mut from, mut gap) = (0.0, -1.0);
    for pair in line.windows(2) {
        let width = pair[1].0 - pair[0].0;
        if width > gap {
            (from, gap) = (pair[0].0, width);
        }
    }
    let x = from + gap / 2.0;
    (x, output_at(&line, x))
}

/// The curve's output at `count` evenly spaced inputs, for a bar plot.
pub fn samples(points: &[(f64, f64)], count: usize) -> Vec<f64> {
    let line = polyline(points);
    (1..=count)
        .map(|step| output_at(&line, step as f64 / count as f64).clamp(0.0, 1.0))
        .collect()
}

/// The points sorted by input, as typed.
pub fn in_order(points: &[(String, String)]) -> Option<Points> {
    let numbers = parse_points(points)?;
    let mut paired: Vec<(f64, (String, String))> = numbers
        .into_iter()
        .zip(
            points
                .iter()
                .map(|(x, y)| (x.trim().to_string(), y.trim().to_string())),
        )
        .map(|((x, _), text)| (x, text))
        .collect();
    paired.sort_by(|a, b| a.0.total_cmp(&b.0));
    Some(paired.into_iter().map(|(_, text)| text).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devices_pair_by_guid_and_fall_back_to_the_name() {
        assert!(same_device(
            "T-Pedals  {B678044F-0000-0000-0000-504944564944}",
            "Thrustmaster T-Pedals  {b678044f-0000-0000-0000-504944564944}"
        ));
        assert!(!same_device(
            "VKB Gladiator NXT L  {0200231D-0000-0000-0000-504944564944}",
            "VKB Gladiator NXT L  {0201231D-0000-0000-0000-504944564944}"
        ));
        assert!(same_device(
            "Controller (Gamepad)",
            " Controller (Gamepad) "
        ));
        assert!(!same_device("", ""));
    }

    #[test]
    fn numbers_read_like_the_file() {
        assert_eq!(number(0.05), "0.05");
        assert_eq!(number(2.0), "2");
        assert_eq!(number(0.125), "0.125");
        assert_eq!(number(0.950_000_000_1), "0.95");
        assert_eq!(number(-0.0), "0");
    }

    #[test]
    fn a_new_point_splits_the_widest_gap_on_the_curve() {
        assert_eq!(new_point(&[]), (0.5, 0.5));
        let (x, y) = new_point(&[(0.25, 0.1), (0.5, 0.3), (0.75, 0.6)]);
        assert_eq!(number(x), "0.125");
        assert_eq!(number(y), "0.05");
        let (x, y) = new_point(&[(0.2, 0.1)]);
        assert_eq!(number(x), "0.6");
        assert_eq!(number(y), "0.55");
    }

    #[test]
    fn samples_follow_the_points() {
        assert_eq!(samples(&[], 4), vec![0.25, 0.5, 0.75, 1.0]);
        let bars = samples(&[(0.5, 0.1)], 2);
        assert!((bars[0] - 0.1).abs() < 1e-9);
        assert!((bars[1] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn points_are_checked_and_sorted_by_input() {
        let typed = vec![
            ("0.9".to_string(), "0.05".to_string()),
            (" 0.25".to_string(), "0.1".to_string()),
        ];
        assert_eq!(
            in_order(&typed),
            Some(vec![
                ("0.25".to_string(), "0.1".to_string()),
                ("0.9".to_string(), "0.05".to_string()),
            ])
        );
        assert_eq!(in_order(&[("1.2".into(), "0".into())]), None);
        assert_eq!(in_order(&[("x".into(), "0".into())]), None);
    }
}
