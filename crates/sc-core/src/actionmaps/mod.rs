//! actionmaps.xml parsing and editing.

mod options;

pub use options::{AxisField, AxisOption, DeviceOptions, GroupField, GroupOptions, GroupSetting};

use crate::error::{Error, Result};
use crate::staging::Document;
use crate::xml_edit::{self, Attr, Element, Scan, line_at};
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const BOM: &[u8] = b"\xef\xbb\xbf";
const ROOT: &str = "ActionMaps";
const PROFILES: &str = "ActionProfiles";
const DEFAULT_PROFILE: &str = "default";
const MAP: &str = "actionmap";
const ACTION: &str = "action";
const REBIND: &str = "rebind";
const INPUT: &str = "input";
const OPTIONS: &str = "options";
const SHARED_MAP_PREFIX: &str = "spaceship_";
const PREFIXES: [(&str, DeviceKind); 4] = [
    ("kb", DeviceKind::Keyboard),
    ("mo", DeviceKind::Mouse),
    ("js", DeviceKind::Joystick),
    ("gp", DeviceKind::Gamepad),
];
const OPTION_TYPES: [(&str, DeviceKind); 4] = [
    ("keyboard", DeviceKind::Keyboard),
    ("mouse", DeviceKind::Mouse),
    ("joystick", DeviceKind::Joystick),
    ("gamepad", DeviceKind::Gamepad),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Profile,
    Layout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeviceKind {
    Keyboard,
    Mouse,
    Joystick,
    Gamepad,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slot {
    pub kind: DeviceKind,
    pub instance: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub slot: Slot,
    pub product: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub map: String,
    pub action: String,
    pub input: String,
    pub slot: Option<Slot>,
    pub extra: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub input: String,
    /// Each entry is `(map, action)`.
    pub bindings: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Replaces every rebind of the input's device kind on the action with this one.
    Bind {
        map: String,
        action: String,
        input: String,
    },
    /// Like `Bind`, with the game's explicit unbind (`<prefix><n>_ `) as the input.
    Unbind {
        map: String,
        action: String,
        slot: Slot,
    },
    /// Removes every rebind of the kind, and the action and map when that leaves them empty.
    Reset {
        map: String,
        action: String,
        kind: DeviceKind,
    },
    SwapJoysticks {
        a: u32,
        b: u32,
    },
    /// Sets one field of an option group, creating the `options` slot and the group element as needed.
    /// The game keeps mouse groups in its keyboard slot, so `kind: Mouse` edits `type="keyboard"`,
    /// as do the other group and curve ops.
    SetGroup {
        kind: DeviceKind,
        instance: u32,
        group: String,
        field: GroupField,
        value: String,
    },
    /// Removes the field, and the group element when that leaves it empty.
    ResetGroup {
        kind: DeviceKind,
        instance: u32,
        group: String,
        field: GroupField,
    },
    /// Replaces the group's response curve with these `(in, out)` points, in the order given.
    SetCurve {
        kind: DeviceKind,
        instance: u32,
        group: String,
        points: Vec<(String, String)>,
    },
    /// Removes the group's response curve, and the group element when that leaves it empty.
    ResetCurve {
        kind: DeviceKind,
        instance: u32,
        group: String,
    },
    /// Sets the value on every line of the device that carries it, or adds the line.
    SetAxis {
        product: String,
        input: String,
        field: AxisField,
        value: String,
    },
    /// Removes the value from every line that carries it, and each line and device left empty.
    ResetAxis {
        product: String,
        input: String,
        field: AxisField,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionMaps {
    path: PathBuf,
    bom: bool,
    text: String,
    kind: FileKind,
    profile_name: Option<String>,
    devices: Vec<Device>,
    maps: Vec<String>,
    bindings: Vec<Binding>,
    device_options: Vec<DeviceOptions>,
    group_options: Vec<GroupOptions>,
}

impl ActionMaps {
    pub fn kind(&self) -> FileKind {
        self.kind
    }

    pub fn profile_name(&self) -> Option<&str> {
        self.profile_name.as_deref()
    }

    pub fn devices(&self) -> &[Device] {
        &self.devices
    }

    pub fn maps(&self) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        for name in &self.maps {
            if !names.contains(&name.as_str()) {
                names.push(name);
            }
        }
        names
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    pub fn binds_of(&self, map: &str, action: &str, kind: DeviceKind) -> Vec<&Binding> {
        self.bindings
            .iter()
            .filter(|b| b.map == map && b.action == action)
            .filter(|b| b.slot.is_some_and(|slot| slot.kind == kind))
            .collect()
    }

    /// Per-axis deadzone, saturation and acceleration, one entry per `deviceoptions` product.
    pub fn device_options(&self) -> &[DeviceOptions] {
        &self.device_options
    }

    /// Per-slot option group overrides, one entry per `options` element.
    pub fn group_options(&self) -> &[GroupOptions] {
        &self.group_options
    }

    pub fn conflicts(&self) -> Vec<Conflict> {
        let mut groups: BTreeMap<String, Conflict> = BTreeMap::new();
        for binding in self.bindings.iter().filter(|b| is_bound(&b.input)) {
            let group = groups
                .entry(binding.input.to_ascii_lowercase())
                .or_insert_with(|| Conflict {
                    input: binding.input.clone(),
                    bindings: Vec::new(),
                });
            let pair = (binding.map.clone(), binding.action.clone());
            if !group.bindings.contains(&pair) {
                group.bindings.push(pair);
            }
        }
        groups
            .into_values()
            .filter(|group| clashes(&group.bindings))
            .collect()
    }

    fn from_text(path: PathBuf, bom: bool, text: String) -> Result<Self> {
        let tree = Tree::check(&path, &text)?;
        let profile_name = attr_value(tree.element(tree.container), "profileName").map(Into::into);
        let devices = tree.children[tree.container]
            .iter()
            .filter_map(|&index| device(tree.element(index)))
            .collect();
        let mut maps = Vec::new();
        let mut bindings = Vec::new();
        for map in tree.maps() {
            let map_name = name_of(tree.element(map));
            maps.push(map_name.to_string());
            for &action in &tree.children[map] {
                let action_name = name_of(tree.element(action));
                for &rebind in &tree.children[action] {
                    bindings.push(binding(map_name, action_name, tree.element(rebind)));
                }
            }
        }
        Ok(Self {
            kind: tree.kind,
            device_options: options::read_device_options(&tree),
            group_options: options::read_group_options(&tree),
            path,
            bom,
            text,
            profile_name,
            devices,
            maps,
            bindings,
        })
    }

    fn edited(&self, op: &Op) -> Result<String> {
        let mut text = self.text.clone();
        let path = &self.path;
        match op {
            Op::Bind { map, action, input } => bind(path, &mut text, map, action, input)?,
            Op::Unbind { map, action, slot } => unbind(path, &mut text, map, action, *slot)?,
            Op::Reset { map, action, kind } => reset(path, &mut text, map, action, *kind)?,
            Op::SwapJoysticks { a, b } => swap_joysticks(path, &mut text, *a, *b)?,
            Op::SetGroup {
                kind,
                instance,
                group,
                field,
                value,
            } => options::set_group(
                path,
                &mut text,
                option_slot(*kind, *instance),
                group,
                *field,
                value,
            )?,
            Op::ResetGroup {
                kind,
                instance,
                group,
                field,
            } => options::reset_group(
                path,
                &mut text,
                option_slot(*kind, *instance),
                group,
                *field,
            )?,
            Op::SetCurve {
                kind,
                instance,
                group,
                points,
            } => options::set_curve(
                path,
                &mut text,
                option_slot(*kind, *instance),
                group,
                points,
            )?,
            Op::ResetCurve {
                kind,
                instance,
                group,
            } => options::reset_curve(path, &mut text, option_slot(*kind, *instance), group)?,
            Op::SetAxis {
                product,
                input,
                field,
                value,
            } => options::set_axis(path, &mut text, product, input, *field, value)?,
            Op::ResetAxis {
                product,
                input,
                field,
            } => options::reset_axis(path, &mut text, product, input, *field)?,
        }
        Ok(text)
    }
}

impl Document for ActionMaps {
    type Op = Op;

    fn parse(path: &Path, bytes: &[u8]) -> Result<Self> {
        let (bom, body) = match bytes.strip_prefix(BOM) {
            Some(body) => (true, body),
            None => (false, bytes),
        };
        let text = String::from_utf8(body.to_vec()).map_err(|err| {
            let valid = &body[..err.utf8_error().valid_up_to()];
            Error::Parse {
                path: path.to_path_buf(),
                line: valid.iter().filter(|&&b| b == b'\n').count() + 1,
                message: "the file is not valid UTF-8".into(),
            }
        })?;
        Self::from_text(path.to_path_buf(), bom, text)
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(BOM.len() + self.text.len());
        if self.bom {
            bytes.extend_from_slice(BOM);
        }
        bytes.extend_from_slice(self.text.as_bytes());
        bytes
    }

    fn apply(&mut self, op: &Op) -> Result<()> {
        let text = self.edited(op)?;
        *self = Self::from_text(self.path.clone(), self.bom, text)?;
        Ok(())
    }
}

fn option_slot(kind: DeviceKind, instance: u32) -> Slot {
    let kind = match kind {
        DeviceKind::Mouse => DeviceKind::Keyboard,
        other => other,
    };
    Slot { kind, instance }
}

pub fn parse_input(input: &str) -> Option<(Slot, &str)> {
    let (head, rest) = input.split_once('_')?;
    let (kind, digits) = PREFIXES
        .iter()
        .find_map(|&(prefix, kind)| head.strip_prefix(prefix).map(|digits| (kind, digits)))?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let instance = digits.parse().ok()?;
    Some((Slot { kind, instance }, rest))
}

pub fn format_input(slot: Slot, rest: &str) -> String {
    let prefix = PREFIXES
        .iter()
        .find(|(_, kind)| *kind == slot.kind)
        .map_or("", |(prefix, _)| prefix);
    format!("{prefix}{}_{rest}", slot.instance)
}

pub fn is_unbind(input: &str) -> bool {
    parse_input(input).is_some_and(|(_, rest)| rest == " ")
}

pub fn layout_file_name(name: &str) -> Result<String> {
    let name = name.trim();
    let allowed = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_');
    if name.is_empty() || !name.bytes().all(allowed) {
        return Err(Error::Invalid(format!(
            "{name:?} is not a usable layout name: use only letters, digits, - and _"
        )));
    }
    Ok(format!("layout_{name}_exported.xml"))
}

fn bind(path: &Path, text: &mut String, map: &str, action: &str, input: &str) -> Result<()> {
    let Some((slot, rest)) = parse_input(input) else {
        return Err(Error::Invalid(format!(
            "{input:?} does not start with a device prefix such as kb1_ or js1_"
        )));
    };
    if rest.trim().is_empty() {
        return Err(Error::Invalid("use Unbind to clear a binding".into()));
    }
    replace_kind(
        path,
        text,
        map,
        action,
        slot.kind,
        input,
        Keep::OtherAttributes,
    )
}

fn unbind(path: &Path, text: &mut String, map: &str, action: &str, slot: Slot) -> Result<()> {
    let input = format_input(slot, " ");
    if !is_unbind(&input) {
        return Err(Error::Invalid(format!(
            "{input:?} is not a usable device prefix"
        )));
    }
    replace_kind(path, text, map, action, slot.kind, &input, Keep::Nothing)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Keep {
    OtherAttributes,
    Nothing,
}

fn replace_kind(
    path: &Path,
    text: &mut String,
    map: &str,
    action: &str,
    kind: DeviceKind,
    input: &str,
    keep: Keep,
) -> Result<()> {
    check_name(map)?;
    check_name(action)?;
    let tree = Tree::check(path, text)?;
    if tree.find_map(map).is_none() {
        let element = tree.element(tree.container);
        xml_edit::append_child(text, element, &named(MAP, map));
    }
    let tree = Tree::check(path, text)?;
    let map_index = tree.find_map(map).ok_or_else(|| not_added(MAP, map))?;
    if tree.find_action(map_index, action).is_none() {
        xml_edit::append_child(text, tree.element(map_index), &named(ACTION, action));
    }
    match keep {
        Keep::OtherAttributes => remove_rebinds_after_first(path, text, map, action, kind)?,
        Keep::Nothing => remove_rebinds(path, text, map, action, kind).map(drop)?,
    }

    let tree = Tree::check(path, text)?;
    let action_index = tree
        .find_map(map)
        .and_then(|map_index| tree.find_action(map_index, action))
        .ok_or_else(|| not_added(ACTION, action))?;
    match tree
        .find_rebind(action_index, kind)
        .and_then(|rebind| attr(tree.element(rebind), INPUT))
    {
        Some(existing) => xml_edit::set_attr(text, existing, input),
        None => {
            let rebind = format!("<{REBIND} {INPUT}=\"{}\"/>", xml_edit::escape_attr(input));
            xml_edit::append_child(text, tree.element(action_index), &rebind);
        }
    }
    Ok(())
}

fn remove_rebinds_after_first(
    path: &Path,
    text: &mut String,
    map: &str,
    action: &str,
    kind: DeviceKind,
) -> Result<()> {
    loop {
        let tree = Tree::check(path, text)?;
        let Some(action_index) = tree
            .find_map(map)
            .and_then(|map_index| tree.find_action(map_index, action))
        else {
            return Ok(());
        };
        let Some(second) = tree.rebinds_of(action_index, kind).nth(1) else {
            return Ok(());
        };
        xml_edit::remove_element(text, tree.element(second));
    }
}

fn remove_rebinds(
    path: &Path,
    text: &mut String,
    map: &str,
    action: &str,
    kind: DeviceKind,
) -> Result<usize> {
    let mut removed = 0;
    loop {
        let tree = Tree::check(path, text)?;
        let Some(rebind) = tree
            .find_map(map)
            .and_then(|map_index| tree.find_action(map_index, action))
            .and_then(|action_index| tree.find_rebind(action_index, kind))
        else {
            return Ok(removed);
        };
        xml_edit::remove_element(text, tree.element(rebind));
        removed += 1;
    }
}

fn reset(path: &Path, text: &mut String, map: &str, action: &str, kind: DeviceKind) -> Result<()> {
    check_name(map)?;
    check_name(action)?;
    if remove_rebinds(path, text, map, action, kind)? == 0 {
        return Err(Error::Invalid(format!(
            "{map}/{action} has no {} binding",
            kind_name(kind)
        )));
    }

    let tree = Tree::check(path, text)?;
    let Some(map_index) = tree.find_map(map) else {
        return Ok(());
    };
    let Some(action_index) = tree.find_action(map_index, action) else {
        return Ok(());
    };
    if !tree.children[action_index].is_empty() {
        return Ok(());
    }
    xml_edit::remove_element(text, tree.element(action_index));

    let tree = Tree::check(path, text)?;
    if let Some(map_index) = tree.find_map(map)
        && tree.children[map_index].is_empty()
    {
        xml_edit::remove_element(text, tree.element(map_index));
    }
    Ok(())
}

fn swap_joysticks(path: &Path, text: &mut String, a: u32, b: u32) -> Result<()> {
    if a == b || a == 0 || b == 0 {
        return Err(Error::Invalid(format!(
            "cannot swap joystick {a} with joystick {b}"
        )));
    }
    let other = |instance: u32| match instance {
        i if i == a => Some(b),
        i if i == b => Some(a),
        _ => None,
    };
    let tree = Tree::check(path, text)?;
    let mut edits: Vec<(&Attr, String)> = Vec::new();
    for rebind in tree.rebinds() {
        if let Some(attr) = attr(tree.element(rebind), INPUT)
            && let Some((slot, rest)) = parse_input(&attr.value)
            && slot.kind == DeviceKind::Joystick
            && let Some(instance) = other(slot.instance)
        {
            let swapped = Slot {
                kind: DeviceKind::Joystick,
                instance,
            };
            edits.push((attr, format_input(swapped, rest)));
        }
    }
    for &index in &tree.children[tree.container] {
        let element = tree.element(index);
        if let Some(device) = device(element)
            && device.slot.kind == DeviceKind::Joystick
            && let Some(instance) = other(device.slot.instance)
            && let Some(attr) = attr(element, "instance")
        {
            edits.push((attr, instance.to_string()));
        }
    }
    edits.sort_by_key(|(attr, _)| Reverse(attr.value_span.start));
    for (attr, value) in edits {
        xml_edit::set_attr(text, attr, &value);
    }
    Ok(())
}

/// A scan that passed the structure check, with each element's children and the bindings container located.
struct Tree {
    scan: Scan,
    children: Vec<Vec<usize>>,
    kind: FileKind,
    container: usize,
}

impl Tree {
    fn check(path: &Path, text: &str) -> Result<Self> {
        let fail = |at: usize, reason: String| Error::Parse {
            path: path.to_path_buf(),
            line: line_at(text, at),
            message: format!("format not recognised: {reason}"),
        };
        if text.is_empty() {
            return Err(fail(0, "empty file".into()));
        }
        let scan = xml_edit::scan(path, text)?;
        if scan.has_doctype {
            let at = text.find("<!DOCTYPE").unwrap_or(0);
            return Err(fail(at, "the file has a DOCTYPE declaration".into()));
        }
        let mut roots = scan
            .elements
            .iter()
            .enumerate()
            .filter(|(_, e)| e.depth == 0);
        let Some((root, root_element)) = roots.next() else {
            return Err(fail(0, "there is no root element".into()));
        };
        if let Some((_, second)) = roots.next() {
            return Err(fail(
                second.open_tag.start,
                "there is more than one root element".into(),
            ));
        }
        if root_element.name != ROOT {
            return Err(fail(
                root_element.open_tag.start,
                format!(
                    "the root element is <{}>, expected <{ROOT}>",
                    root_element.name
                ),
            ));
        }

        let mut children = vec![Vec::new(); scan.elements.len()];
        for (index, element) in scan.elements.iter().enumerate() {
            if let Some(parent) = element.parent {
                children[parent].push(index);
            }
        }
        let profiles: Vec<usize> = children[root]
            .iter()
            .copied()
            .filter(|&index| scan.elements[index].name == PROFILES)
            .collect();
        let (kind, container) = match profiles.first() {
            None => (FileKind::Layout, root),
            Some(&first) => {
                let default = profiles.iter().copied().find(|&index| {
                    attr_value(&scan.elements[index], "profileName") == Some(DEFAULT_PROFILE)
                });
                (FileKind::Profile, default.unwrap_or(first))
            }
        };
        let tree = Self {
            scan,
            children,
            kind,
            container,
        };

        for map in tree.maps() {
            let map_element = tree.element(map);
            let Some(map_name) = attr_value(map_element, "name") else {
                return Err(fail(
                    map_element.open_tag.start,
                    format!("<{MAP}> has no name attribute"),
                ));
            };
            for &action in &tree.children[map] {
                let action_element = tree.element(action);
                let at = action_element.open_tag.start;
                if action_element.name != ACTION {
                    return Err(fail(
                        at,
                        format!(
                            "<{}> inside <{MAP} name=\"{map_name}\">, expected <{ACTION}>",
                            action_element.name
                        ),
                    ));
                }
                let Some(action_name) = attr_value(action_element, "name") else {
                    return Err(fail(
                        at,
                        format!("<{ACTION}> in <{MAP} name=\"{map_name}\"> has no name attribute"),
                    ));
                };
                for &rebind in &tree.children[action] {
                    let rebind_element = tree.element(rebind);
                    let at = rebind_element.open_tag.start;
                    if rebind_element.name != REBIND {
                        return Err(fail(
                            at,
                            format!(
                                "<{}> inside <{ACTION} name=\"{action_name}\">, expected <{REBIND}>",
                                rebind_element.name
                            ),
                        ));
                    }
                    if attr(rebind_element, INPUT).is_none() {
                        return Err(fail(
                            at,
                            format!(
                                "<{REBIND}> in <{ACTION} name=\"{action_name}\"> has no {INPUT} attribute"
                            ),
                        ));
                    }
                }
            }
        }
        Ok(tree)
    }

    fn element(&self, index: usize) -> &Element {
        &self.scan.elements[index]
    }

    fn maps(&self) -> impl Iterator<Item = usize> + '_ {
        self.children[self.container]
            .iter()
            .copied()
            .filter(|&index| self.element(index).name == MAP)
    }

    fn rebinds(&self) -> impl Iterator<Item = usize> + '_ {
        self.maps()
            .flat_map(|map| &self.children[map])
            .flat_map(|&action| &self.children[action])
            .copied()
    }

    fn find_map(&self, name: &str) -> Option<usize> {
        self.maps()
            .find(|&index| attr_value(self.element(index), "name") == Some(name))
    }

    fn find_action(&self, map: usize, name: &str) -> Option<usize> {
        self.children[map]
            .iter()
            .copied()
            .find(|&index| attr_value(self.element(index), "name") == Some(name))
    }

    fn find_rebind(&self, action: usize, kind: DeviceKind) -> Option<usize> {
        self.rebinds_of(action, kind).next()
    }

    fn rebinds_of(&self, action: usize, kind: DeviceKind) -> impl Iterator<Item = usize> + '_ {
        self.children[action].iter().copied().filter(move |&index| {
            attr_value(self.element(index), INPUT)
                .and_then(parse_input)
                .is_some_and(|(slot, _)| slot.kind == kind)
        })
    }
}

fn device(element: &Element) -> Option<Device> {
    if element.name != OPTIONS {
        return None;
    }
    let device_type = attr_value(element, "type")?;
    let kind = OPTION_TYPES
        .iter()
        .find(|(name, _)| *name == device_type)
        .map(|&(_, kind)| kind)?;
    let instance = attr_value(element, "instance")?.parse().ok()?;
    Some(Device {
        slot: Slot { kind, instance },
        product: attr_value(element, "Product").map(Into::into),
    })
}

fn binding(map: &str, action: &str, rebind: &Element) -> Binding {
    let input = attr_value(rebind, INPUT).unwrap_or_default().to_string();
    Binding {
        map: map.into(),
        action: action.into(),
        slot: parse_input(&input).map(|(slot, _)| slot),
        input,
        extra: rebind
            .attrs
            .iter()
            .filter(|attr| attr.name != INPUT)
            .map(|attr| (attr.name.clone(), attr.value.clone()))
            .collect(),
    }
}

fn kind_name(kind: DeviceKind) -> &'static str {
    OPTION_TYPES
        .iter()
        .find(|(_, found)| *found == kind)
        .map_or("device", |(name, _)| name)
}

pub(crate) fn is_bound(input: &str) -> bool {
    !input.trim().is_empty() && parse_input(input).is_none_or(|(_, rest)| !rest.trim().is_empty())
}

pub(crate) fn clashes(pairs: &[(String, String)]) -> bool {
    pairs.iter().enumerate().any(|(i, (first, _))| {
        pairs[i + 1..].iter().any(|(second, _)| {
            first == second
                || (first.starts_with(SHARED_MAP_PREFIX) && second.starts_with(SHARED_MAP_PREFIX))
        })
    })
}

fn attr<'a>(element: &'a Element, name: &str) -> Option<&'a Attr> {
    element.attrs.iter().find(|attr| attr.name == name)
}

fn attr_value<'a>(element: &'a Element, name: &str) -> Option<&'a str> {
    attr(element, name).map(|attr| attr.value.as_str())
}

fn name_of(element: &Element) -> &str {
    attr_value(element, "name").unwrap_or_default()
}

fn named(tag: &str, name: &str) -> String {
    format!("<{tag} name=\"{}\"/>", xml_edit::escape_attr(name))
}

fn not_added(tag: &str, name: &str) -> Error {
    Error::Invalid(format!("could not add <{tag} name={name:?}>"))
}

fn check_name(name: &str) -> Result<()> {
    let bad =
        |c: char| matches!(c, '"' | '\'' | '<' | '>' | '&') || c.is_whitespace() || c.is_control();
    if name.is_empty() || name.contains(bad) {
        return Err(Error::Invalid(format!(
            "{name:?} is not a usable map or action name"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::staging::Document;
    use std::fs;
    use std::path::Path;

    use DeviceKind::{Gamepad, Joystick, Keyboard, Mouse};

    const RECOGNISED: [&str; 6] = [
        "profile.xml",
        "layout.xml",
        "two_profiles.xml",
        "unknown_bits.xml",
        "real_profile.xml",
        "real_layout.xml",
    ];

    fn fixture(name: &str) -> Vec<u8> {
        fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/actionmaps")
                .join(name),
        )
        .unwrap()
    }

    fn fixture_text(name: &str) -> String {
        text(&fixture(name))
    }

    fn parse(bytes: &[u8]) -> crate::error::Result<ActionMaps> {
        ActionMaps::parse(Path::new("actionmaps.xml"), bytes)
    }

    fn doc(name: &str) -> ActionMaps {
        parse(&fixture(name)).unwrap()
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn slot(kind: DeviceKind, instance: u32) -> Slot {
        Slot { kind, instance }
    }

    fn bind(map: &str, action: &str, input: &str) -> Op {
        Op::Bind {
            map: map.into(),
            action: action.into(),
            input: input.into(),
        }
    }

    fn unbind(map: &str, action: &str, slot: Slot) -> Op {
        Op::Unbind {
            map: map.into(),
            action: action.into(),
            slot,
        }
    }

    fn reset(map: &str, action: &str, kind: DeviceKind) -> Op {
        Op::Reset {
            map: map.into(),
            action: action.into(),
            kind,
        }
    }

    fn swap(a: u32, b: u32) -> Op {
        Op::SwapJoysticks { a, b }
    }

    fn applied(name: &str, op: Op) -> String {
        let mut doc = doc(name);
        doc.apply(&op).unwrap();
        text(&doc.to_bytes())
    }

    fn replaced_once(haystack: &str, from: &str, to: &str) -> String {
        assert_eq!(haystack.matches(from).count(), 1, "{from:?}");
        haystack.replacen(from, to, 1)
    }

    fn exchanged(haystack: &str, a: &str, b: &str) -> String {
        const HOLD: &str = "\u{1}";
        haystack.replace(a, HOLD).replace(b, a).replace(HOLD, b)
    }

    fn invalid(doc: &mut ActionMaps, op: Op) -> String {
        let before = doc.clone();
        let message = match doc.apply(&op) {
            Err(Error::Invalid(message)) => message,
            other => panic!("expected Invalid for {op:?}, got {other:?}"),
        };
        assert_eq!(*doc, before, "{op:?}");
        message
    }

    fn not_recognised(bytes: &[u8]) -> (usize, String) {
        match parse(bytes).unwrap_err() {
            Error::Parse { line, message, .. } => (line, message),
            other => panic!("expected a parse error, got {other:?}"),
        }
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(map, action)| (map.to_string(), action.to_string()))
            .collect()
    }

    fn profile_with_maps(maps: &str) -> Vec<u8> {
        format!(
            "<ActionMaps>\n <ActionProfiles profileName=\"default\">\n{maps} </ActionProfiles>\n</ActionMaps>\n"
        )
        .into_bytes()
    }

    #[test]
    fn parse_input_and_format_input_round_trip() {
        for (input, kind, instance, rest) in [
            ("kb1_lalt+f", Keyboard, 1, "lalt+f"),
            ("mo1_mwheel_up", Mouse, 1, "mwheel_up"),
            ("js2_button5", Joystick, 2, "button5"),
            ("gp1_shoulderl", Gamepad, 1, "shoulderl"),
            ("js12_hat1_up", Joystick, 12, "hat1_up"),
            ("js1_", Joystick, 1, ""),
            ("kb1_ ", Keyboard, 1, " "),
        ] {
            let slot = slot(kind, instance);
            assert_eq!(parse_input(input), Some((slot, rest)), "{input}");
            assert_eq!(format_input(slot, rest), input);
        }
        for no_prefix in [
            "button5",
            "",
            "js_x",
            "js0_x",
            "jsx_y",
            "js1button",
            "xx1_y",
            "js-1_x",
        ] {
            assert_eq!(parse_input(no_prefix), None, "{no_prefix:?}");
        }
    }

    #[test]
    fn every_recognised_fixture_round_trips_byte_for_byte() {
        for name in RECOGNISED {
            let bytes = fixture(name);
            assert_eq!(parse(&bytes).unwrap().to_bytes(), bytes, "{name}");
        }
        for inline in [
            &b"\xef\xbb\xbf<ActionMaps profileName=\"x\">\r\n <actionmap name=\"m\"/>\r\n</ActionMaps>"[..],
            b"<ActionMaps/>",
            b"<ActionMaps>\n <actionmap name=\"m\">\n  <action name=\"a\">text<rebind input='js1_x'/></action>\n </actionmap>\n</ActionMaps>",
        ] {
            assert_eq!(parse(inline).unwrap().to_bytes(), inline);
        }
        let mut bom = b"\xef\xbb\xbf".to_vec();
        bom.extend_from_slice(&fixture("profile.xml"));
        let mut doc = parse(&bom).unwrap();
        assert_eq!(doc.bindings().len(), 12);
        doc.apply(&swap(1, 2)).unwrap();
        assert!(doc.to_bytes().starts_with(b"\xef\xbb\xbf<ActionMaps>"));
    }

    #[test]
    fn profile_and_layout_are_told_apart() {
        let profile = doc("profile.xml");
        assert_eq!(profile.kind(), FileKind::Profile);
        assert_eq!(profile.profile_name(), Some("default"));

        let layout = doc("layout.xml");
        assert_eq!(layout.kind(), FileKind::Layout);
        assert_eq!(layout.profile_name(), Some("mylayout"));
        assert_eq!(layout.bindings().len(), 2);

        let bare = parse(b"<ActionMaps/>\n").unwrap();
        assert_eq!(bare.kind(), FileKind::Layout);
        assert_eq!(bare.profile_name(), None);
        assert!(bare.bindings().is_empty());
    }

    #[test]
    fn default_profile_is_used_when_several_exist() {
        let doc = doc("two_profiles.xml");
        assert_eq!(doc.kind(), FileKind::Profile);
        assert_eq!(doc.profile_name(), Some("default"));
        let inputs: Vec<_> = doc.bindings().iter().map(|b| b.input.as_str()).collect();
        assert_eq!(inputs, ["js1_button3"]);
        assert_eq!(
            doc.devices()[0].product.as_deref(),
            Some("Some Stick {GUID}")
        );

        let no_default = b"<ActionMaps>\n <ActionProfiles profileName=\"first\">\n  <actionmap name=\"m\">\n   <action name=\"a\">\n    <rebind input=\"kb1_a\"/>\n   </action>\n  </actionmap>\n </ActionProfiles>\n <ActionProfiles profileName=\"second\">\n  <actionmap name=\"m\">\n   <action name=\"a\">\n    <rebind input=\"kb1_b\"/>\n   </action>\n  </actionmap>\n </ActionProfiles>\n</ActionMaps>\n";
        let doc = parse(no_default).unwrap();
        assert_eq!(doc.profile_name(), Some("first"));
        let inputs: Vec<_> = doc.bindings().iter().map(|b| b.input.as_str()).collect();
        assert_eq!(inputs, ["kb1_a"]);
    }

    #[test]
    fn devices_list_slots_and_products() {
        let device = |kind, instance, product: &str| Device {
            slot: slot(kind, instance),
            product: Some(product.into()),
        };
        assert_eq!(
            doc("profile.xml").devices(),
            [
                device(
                    Keyboard,
                    1,
                    "Keyboard  {6F1D2B61-D5A0-11CF-BFC7-444553540000}"
                ),
                device(Mouse, 1, "Mouse  {6F1D2B60-D5A0-11CF-BFC7-444553540000}"),
                device(
                    Joystick,
                    1,
                    "T16000M Stick {B10A044F-0000-0000-0000-504944564944}"
                ),
                device(
                    Joystick,
                    2,
                    "T16000M Throttle {B10B044F-0000-0000-0000-504944564944}"
                ),
            ]
        );
        let layout = doc("layout.xml");
        let slots: Vec<_> = layout.devices().iter().map(|d| d.slot).collect();
        assert_eq!(slots, [slot(Keyboard, 1), slot(Joystick, 1)]);

        let odd = b"<ActionMaps>\n <options type=\"gamepad\" instance=\"1\"/>\n <options type=\"pedals\" instance=\"1\"/>\n <options type=\"joystick\" instance=\"one\"/>\n <options type=\"joystick\"/>\n <options instance=\"3\"/>\n <other><options type=\"joystick\" instance=\"4\"/></other>\n</ActionMaps>\n";
        assert_eq!(
            parse(odd).unwrap().devices(),
            [Device {
                slot: slot(Gamepad, 1),
                product: None
            }]
        );
    }

    #[test]
    fn bindings_carry_map_action_input_slot_and_extra() {
        let doc = doc("profile.xml");
        let bindings = doc.bindings();
        assert_eq!(bindings.len(), 12);
        assert_eq!(
            bindings[0],
            Binding {
                map: "spaceship_movement".into(),
                action: "v_afterburner".into(),
                input: "js1_button3".into(),
                slot: Some(slot(Joystick, 1)),
                extra: vec![],
            }
        );
        let multi_tap = bindings
            .iter()
            .find(|b| b.action == "v_toggle_decoupled")
            .unwrap();
        assert_eq!(multi_tap.input, "kb1_lalt+c");
        assert_eq!(multi_tap.slot, Some(slot(Keyboard, 1)));
        assert_eq!(multi_tap.extra, [("multiTap".into(), "2".into())]);
        let unlisted_stick = bindings.iter().find(|b| b.input == "js3_button1").unwrap();
        assert_eq!(unlisted_stick.slot, Some(slot(Joystick, 3)));
        assert_eq!(bindings[11].map, "player");
        assert_eq!(bindings[11].slot, Some(slot(Gamepad, 1)));

        let unknown = parse(&profile_with_maps(
            "  <actionmap name=\"m\">\n   <action name=\"a\">\n    <rebind a=\"1\" input=\"button5\" b=\"x &amp; y\"/>\n   </action>\n  </actionmap>\n",
        ))
        .unwrap();
        assert_eq!(unknown.bindings()[0].slot, None);
        assert_eq!(
            unknown.bindings()[0].extra,
            [("a".into(), "1".into()), ("b".into(), "x & y".into())]
        );
    }

    #[test]
    fn maps_are_in_file_order() {
        assert_eq!(
            doc("profile.xml").maps(),
            ["spaceship_movement", "spaceship_targeting", "player"]
        );
        assert_eq!(
            doc("layout.xml").maps(),
            ["spaceship_movement", "spaceship_weapons"]
        );
        let repeats = parse(&profile_with_maps(
            "  <actionmap name=\"b\"/>\n  <actionmap name=\"a\"/>\n  <actionmap name=\"b\"/>\n",
        ))
        .unwrap();
        assert_eq!(repeats.maps(), ["b", "a"]);
    }

    #[test]
    fn conflicts_found_within_a_map_and_across_spaceship_maps() {
        let mut doc = doc("profile.xml");
        assert_eq!(
            doc.conflicts(),
            [Conflict {
                input: "js1_button3".into(),
                bindings: pairs(&[
                    ("spaceship_movement", "v_afterburner"),
                    ("spaceship_movement", "v_boost"),
                ]),
            }]
        );

        doc.apply(&bind("spaceship_targeting", "v_target_pin", "js1_BUTTON3"))
            .unwrap();
        doc.apply(&bind("spaceship_targeting", "v_target_pin", "kb1_lctrl"))
            .unwrap();
        assert_eq!(
            doc.conflicts(),
            [
                Conflict {
                    input: "js1_button3".into(),
                    bindings: pairs(&[
                        ("spaceship_movement", "v_afterburner"),
                        ("spaceship_movement", "v_boost"),
                        ("spaceship_targeting", "v_target_pin"),
                    ]),
                },
                Conflict {
                    input: "kb1_lctrl".into(),
                    bindings: pairs(&[
                        ("spaceship_movement", "v_strafe_down"),
                        ("spaceship_targeting", "v_target_pin"),
                    ]),
                },
            ]
        );
    }

    #[test]
    fn no_conflict_across_unrelated_maps() {
        let doc = doc("profile.xml");
        assert!(doc.conflicts().iter().all(|c| c.input != "kb1_space"));

        let unrelated = parse(&profile_with_maps(
            "  <actionmap name=\"player\">\n   <action name=\"jump\">\n    <rebind input=\"kb1_space\"/>\n    <rebind input=\"kb1_space\"/>\n   </action>\n  </actionmap>\n  <actionmap name=\"vehicle_general\">\n   <action name=\"v_brake\">\n    <rebind input=\"kb1_space\"/>\n   </action>\n  </actionmap>\n  <actionmap name=\"spaceship_view\">\n   <action name=\"v_look\">\n    <rebind input=\"kb1_space\"/>\n    <rebind input=\"js1_ \"/>\n   </action>\n   <action name=\"v_zoom\">\n    <rebind input=\"js1_ \"/>\n   </action>\n  </actionmap>\n",
        ))
        .unwrap();
        assert_eq!(unrelated.conflicts(), []);
    }

    #[test]
    fn bind_keeps_the_other_attributes_of_the_rebind_it_replaces() {
        let original = fixture_text("profile.xml");
        let mut doc = doc("profile.xml");
        doc.apply(&bind(
            "spaceship_movement",
            "v_toggle_decoupled",
            "kb1_lalt+d",
        ))
        .unwrap();
        assert_eq!(
            text(&doc.to_bytes()),
            replaced_once(
                &original,
                "\"kb1_lalt+c\" multiTap=\"2\"/>",
                "\"kb1_lalt+d\" multiTap=\"2\"/>"
            )
        );
        let edited = doc
            .bindings()
            .iter()
            .find(|b| b.action == "v_toggle_decoupled")
            .unwrap();
        assert_eq!(edited.extra, [("multiTap".into(), "2".into())]);

        let mut again = doc.clone();
        again
            .apply(&bind(
                "spaceship_movement",
                "v_toggle_decoupled",
                "kb1_lalt+d",
            ))
            .unwrap();
        assert_eq!(again.to_bytes(), doc.to_bytes());

        let edited = applied(
            "profile.xml",
            bind("spaceship_movement", "v_strafe_up", "js2_x&y"),
        );
        assert_eq!(
            edited,
            replaced_once(&original, "\"js2_button5\"", "\"js2_x&amp;y\"")
        );
    }

    #[test]
    fn bind_adds_rebind_for_a_new_kind() {
        let original = fixture_text("profile.xml");
        let row = "    <rebind input=\"kb1_lctrl\"/>\r\n";
        assert_eq!(
            applied(
                "profile.xml",
                bind("spaceship_movement", "v_strafe_down", "js1_button7")
            ),
            replaced_once(
                &original,
                row,
                &format!("{row}    <rebind input=\"js1_button7\"/>\r\n")
            )
        );

        let original = fixture_text("layout.xml");
        let row = "\t\t\t<rebind input=\"js1_button1\"/>\n";
        assert_eq!(
            applied(
                "layout.xml",
                bind("spaceship_weapons", "v_attack1_group1", "kb1_space")
            ),
            replaced_once(
                &original,
                row,
                &format!("{row}\t\t\t<rebind input=\"kb1_space\"/>\n")
            )
        );
    }

    #[test]
    fn bind_creates_missing_action_then_missing_actionmap() {
        let original = fixture_text("profile.xml");
        let last_action = "    <rebind input=\"gp1_shoulderl\"/>\r\n   </action>\r\n";
        let mut doc = doc("profile.xml");
        doc.apply(&bind("player", "use", "kb1_f")).unwrap();
        let with_action = replaced_once(
            &original,
            last_action,
            &format!(
                "{last_action}   <action name=\"use\">\r\n    <rebind input=\"kb1_f\"/>\r\n   </action>\r\n"
            ),
        );
        assert_eq!(text(&doc.to_bytes()), with_action);

        doc.apply(&bind("vehicle_general", "v_horn", "kb1_h"))
            .unwrap();
        let with_map = replaced_once(
            &with_action,
            " </ActionProfiles>",
            "  <actionmap name=\"vehicle_general\">\r\n   <action name=\"v_horn\">\r\n    <rebind input=\"kb1_h\"/>\r\n   </action>\r\n  </actionmap>\r\n </ActionProfiles>",
        );
        assert_eq!(text(&doc.to_bytes()), with_map);
        assert_eq!(doc.maps().last(), Some(&"vehicle_general"));
        assert_eq!(doc.bindings().last().unwrap().input, "kb1_h");

        let original = fixture_text("layout.xml");
        assert_eq!(
            applied("layout.xml", bind("vehicle_general", "v_horn", "kb1_h")),
            replaced_once(
                &original,
                "</ActionMaps>",
                "\t<actionmap name=\"vehicle_general\">\n\t\t<action name=\"v_horn\">\n\t\t\t<rebind input=\"kb1_h\"/>\n\t\t</action>\n\t</actionmap>\n</ActionMaps>",
            )
        );
    }

    #[test]
    fn bind_rejects_blank_rest_and_unparseable_input() {
        let mut doc = doc("profile.xml");
        for blank in ["js1_", "js1_  ", "kb1_\t"] {
            let message = invalid(&mut doc, bind("spaceship_movement", "v_afterburner", blank));
            assert_eq!(message, "use Unbind to clear a binding");
        }
        for bad in ["button5", "", "js0_button1", " js1_button1"] {
            invalid(&mut doc, bind("spaceship_movement", "v_afterburner", bad));
        }
    }

    #[test]
    fn bind_rejects_bad_names() {
        let mut doc = doc("profile.xml");
        for bad in [
            "", "a b", "a\"b", "a'b", "a<b", "a>b", "a&b", "a\tb", "a\nb",
        ] {
            invalid(&mut doc, bind(bad, "v_afterburner", "kb1_x"));
            invalid(&mut doc, bind("spaceship_movement", bad, "kb1_x"));
            invalid(&mut doc, unbind(bad, "v_afterburner", slot(Joystick, 1)));
            invalid(
                &mut doc,
                unbind("spaceship_movement", bad, slot(Joystick, 1)),
            );
            invalid(&mut doc, reset(bad, "v_afterburner", Joystick));
            invalid(&mut doc, reset("spaceship_movement", bad, Joystick));
        }
    }

    #[test]
    fn reset_removes_rebind_then_empty_action_then_empty_map() {
        let original = fixture_text("profile.xml");
        let mut doc = doc("profile.xml");

        doc.apply(&reset("spaceship_movement", "v_strafe_up", Joystick))
            .unwrap();
        let step1 = replaced_once(&original, "    <rebind input=\"js2_button5\"/>\r\n", "");
        assert_eq!(text(&doc.to_bytes()), step1);

        doc.apply(&reset("spaceship_targeting", "v_target_pin", Joystick))
            .unwrap();
        let step2 = replaced_once(
            &step1,
            "   <action name=\"v_target_pin\">\r\n    <rebind input=\"js2_hat1_up\"/>\r\n   </action>\r\n",
            "",
        );
        assert_eq!(text(&doc.to_bytes()), step2);

        doc.apply(&reset(
            "spaceship_targeting",
            "v_target_cycle_all_fwd",
            Mouse,
        ))
        .unwrap();
        doc.apply(&reset(
            "spaceship_targeting",
            "v_target_cycle_all_fwd",
            Joystick,
        ))
        .unwrap();
        let step3 = replaced_once(
            &step2,
            "  <actionmap name=\"spaceship_targeting\">\r\n   <action name=\"v_target_cycle_all_fwd\">\r\n    <rebind input=\"mo1_mwheel_up\"/>\r\n    <rebind input=\"js3_button1\"/>\r\n   </action>\r\n  </actionmap>\r\n",
            "",
        );
        assert_eq!(text(&doc.to_bytes()), step3);
        assert_eq!(doc.maps(), ["spaceship_movement", "player"]);
    }

    #[test]
    fn reset_absent_is_invalid() {
        let mut doc = doc("profile.xml");
        invalid(
            &mut doc,
            reset("spaceship_movement", "v_afterburner", Mouse),
        );
        invalid(&mut doc, reset("spaceship_movement", "v_missing", Joystick));
        invalid(&mut doc, reset("missing_map", "v_afterburner", Joystick));
        invalid(
            &mut doc,
            reset("spaceship_movement", "v_afterburner", Keyboard),
        );
    }

    fn binds_of_action(doc: &ActionMaps, map: &str, action: &str, kind: DeviceKind) -> Vec<String> {
        doc.binds_of(map, action, kind)
            .iter()
            .map(|b| b.input.clone())
            .collect()
    }

    #[test]
    fn real_fixtures_round_trip_byte_for_byte() {
        for name in ["real_profile.xml", "real_layout.xml"] {
            let bytes = fixture(name);
            assert_eq!(parse(&bytes).unwrap().to_bytes(), bytes, "{name}");
        }
        let profile = doc("real_profile.xml");
        assert_eq!(profile.kind(), FileKind::Profile);
        assert_eq!(profile.bindings().len(), 14);
        let layout = doc("real_layout.xml");
        assert_eq!(layout.kind(), FileKind::Layout);
        assert_eq!(layout.bindings().len(), 9);
    }

    #[test]
    fn bind_replaces_every_rebind_of_the_same_kind() {
        let original = fixture_text("real_layout.xml");
        let mut doc = doc("real_layout.xml");
        doc.apply(&bind(
            "spaceship_weapons",
            "v_attack1_group1",
            "js4_button3",
        ))
        .unwrap();
        assert_eq!(
            text(&doc.to_bytes()),
            replaced_once(
                &original,
                "   <rebind input=\"js1_button1\"/>\r\n   <rebind input=\"js2_ \"/>\r\n",
                "   <rebind input=\"js4_button3\"/>\r\n"
            )
        );
        assert_eq!(
            binds_of_action(&doc, "spaceship_weapons", "v_attack1_group1", Joystick),
            ["js4_button3"]
        );
        assert_eq!(
            binds_of_action(&doc, "spaceship_weapons", "v_attack1_group1", Keyboard),
            ["kb1_space"]
        );
    }

    #[test]
    fn bind_leaves_inputs_without_a_device_prefix_alone() {
        let mut doc = parse(&profile_with_maps(
            "  <actionmap name=\"m\">\n   <action name=\"a\">\n    <rebind input=\"button5\"/>\n    <rebind input=\"js1_x\"/>\n   </action>\n  </actionmap>\n",
        ))
        .unwrap();
        doc.apply(&bind("m", "a", "js2_y")).unwrap();
        let inputs: Vec<_> = doc.bindings().iter().map(|b| b.input.as_str()).collect();
        assert_eq!(inputs, ["button5", "js2_y"]);
    }

    #[test]
    fn bind_accepts_mouse_button_in_keyboard_kind() {
        let original = fixture_text("real_profile.xml");
        let mut doc = doc("real_profile.xml");
        doc.apply(&bind(
            "seat_general",
            "v_operator_mode_cycle_forward",
            "kb1_mouse3",
        ))
        .unwrap();
        assert_eq!(
            text(&doc.to_bytes()),
            replaced_once(&original, "\"kb1_ \"/>", "\"kb1_mouse3\"/>")
        );
        let keyboard = doc.binds_of("seat_general", "v_operator_mode_cycle_forward", Keyboard);
        assert_eq!(keyboard.len(), 1);
        assert_eq!(keyboard[0].input, "kb1_mouse3");
        assert_eq!(keyboard[0].slot, Some(slot(Keyboard, 1)));
        assert!(
            doc.binds_of("seat_general", "v_operator_mode_cycle_forward", Mouse)
                .is_empty()
        );
    }

    #[test]
    fn bind_keeps_a_joystick_bind_when_the_new_one_is_a_keyboard_key() {
        let original = fixture_text("real_profile.xml");
        let row = "    <rebind input=\"js4_button21\"/>\r\n";
        assert_eq!(
            applied(
                "real_profile.xml",
                bind("seat_general", "v_set_flight_mode", "kb1_mouse3")
            ),
            replaced_once(
                &original,
                row,
                &format!("{row}    <rebind input=\"kb1_mouse3\"/>\r\n")
            )
        );
    }

    #[test]
    fn unbind_writes_prefix_and_one_space() {
        let original = fixture_text("real_profile.xml");
        let mut doc = doc("real_profile.xml");
        doc.apply(&unbind(
            "seat_general",
            "v_set_flight_mode",
            slot(Joystick, 1),
        ))
        .unwrap();
        assert_eq!(
            text(&doc.to_bytes()),
            replaced_once(
                &original,
                "    <rebind input=\"js4_button21\"/>\r\n",
                "    <rebind input=\"js1_ \"/>\r\n"
            )
        );
        let binds = doc.binds_of("seat_general", "v_set_flight_mode", Joystick);
        assert_eq!(binds.len(), 1);
        assert_eq!(binds[0].input, "js1_ ");
        assert!(is_unbind(&binds[0].input));

        let layout = applied(
            "real_layout.xml",
            unbind("spaceship_weapons", "v_attack1_group1", slot(Keyboard, 1)),
        );
        assert!(layout.contains("   <rebind input=\"kb1_ \"/>\r\n"));
        assert!(!layout.contains("kb1_space"));
    }

    #[test]
    fn unbind_creates_the_action_when_the_file_has_none() {
        let layout = applied(
            "real_layout.xml",
            unbind("vehicle_general", "v_horn", slot(Mouse, 1)),
        );
        assert!(layout.contains(
            " <actionmap name=\"vehicle_general\">\r\n  <action name=\"v_horn\">\r\n   <rebind input=\"mo1_ \"/>\r\n  </action>\r\n </actionmap>\r\n</ActionMaps>"
        ));
    }

    #[test]
    fn unbind_rejects_instance_zero() {
        let mut doc = doc("real_profile.xml");
        invalid(
            &mut doc,
            unbind("seat_general", "v_eject", slot(Joystick, 0)),
        );
    }

    #[test]
    fn is_unbind_means_a_prefix_and_one_space() {
        for unbound in ["kb1_ ", "js2_ ", "mo1_ ", "gp1_ "] {
            assert!(is_unbind(unbound), "{unbound:?}");
        }
        for bound in [
            "js1_button1",
            "kb1_mouse3",
            "js1_",
            "",
            " ",
            "button1",
            "js1_  ",
        ] {
            assert!(!is_unbind(bound), "{bound:?}");
        }
    }

    #[test]
    fn binds_of_picks_one_action_and_kind() {
        let doc = doc("real_profile.xml");
        assert_eq!(
            binds_of_action(
                &doc,
                "spaceship_salvage",
                "v_salvage_cycle_modifiers_focused",
                Keyboard
            ),
            ["kb1_mouse3"]
        );
        assert_eq!(
            binds_of_action(
                &doc,
                "spaceship_salvage",
                "v_salvage_cycle_modifiers_focused",
                Joystick
            ),
            ["js1_button15"]
        );
        assert_eq!(
            binds_of_action(&doc, "spaceship_movement", "v_roll_right", Mouse),
            ["mo1_ "]
        );
        assert_eq!(
            binds_of_action(
                &doc,
                "spaceship_movement",
                "v_toggle_landing_system",
                Gamepad
            ),
            ["gp1_ "]
        );
        assert!(doc.binds_of("seat_general", "v_eject", Gamepad).is_empty());
        assert!(doc.binds_of("no_such_map", "v_eject", Keyboard).is_empty());
        assert!(
            doc.binds_of("seat_general", "no_such_action", Keyboard)
                .is_empty()
        );
    }

    #[test]
    fn reset_removes_all_of_a_kind_and_empty_parents() {
        let original = fixture_text("real_layout.xml");
        let mut doc = doc("real_layout.xml");

        doc.apply(&reset("spaceship_weapons", "v_attack1_group1", Joystick))
            .unwrap();
        let step1 = replaced_once(
            &original,
            "   <rebind input=\"js1_button1\"/>\r\n   <rebind input=\"js2_ \"/>\r\n",
            "",
        );
        assert_eq!(text(&doc.to_bytes()), step1);

        doc.apply(&reset("spaceship_weapons", "v_attack1_group1", Keyboard))
            .unwrap();
        let step2 = replaced_once(
            &step1,
            "  <action name=\"v_attack1_group1\">\r\n   <rebind input=\"kb1_space\"/>\r\n  </action>\r\n",
            "",
        );
        assert_eq!(text(&doc.to_bytes()), step2);

        doc.apply(&reset("spaceship_weapons", "v_attack_group2", Joystick))
            .unwrap();
        let step3 = replaced_once(
            &step2,
            " <actionmap name=\"spaceship_weapons\">\r\n  <action name=\"v_attack_group2\">\r\n   <rebind input=\"js1_button6\"/>\r\n  </action>\r\n </actionmap>\r\n",
            "",
        );
        assert_eq!(text(&doc.to_bytes()), step3);
        assert_eq!(doc.maps(), ["spaceship_movement", "spaceship_power"]);
    }

    #[test]
    fn reset_of_one_kind_keeps_the_others() {
        let original = fixture_text("real_profile.xml");
        assert_eq!(
            applied(
                "real_profile.xml",
                reset("spaceship_movement", "v_roll_right", Mouse)
            ),
            replaced_once(&original, "    <rebind input=\"mo1_ \"/>\r\n", "")
        );
    }

    #[test]
    fn swap_joysticks_exchanges_prefixes_and_option_instances() {
        let original = fixture_text("profile.xml");
        let mut doc = doc("profile.xml");
        doc.apply(&swap(1, 2)).unwrap();
        let expected = exchanged(
            &exchanged(&original, "\"js1_", "\"js2_"),
            "type=\"joystick\" instance=\"1\"",
            "type=\"joystick\" instance=\"2\"",
        );
        assert_eq!(text(&doc.to_bytes()), expected);
        assert!(expected.contains("<rebind input=\"js2_button3\"/>"));
        assert!(expected.contains("<rebind input=\"js1_button5\"/>"));
        assert!(expected.contains("<rebind input=\"js3_button1\"/>"));

        let sticks: Vec<_> = doc
            .devices()
            .iter()
            .filter(|d| d.slot.kind == Joystick)
            .map(|d| (d.slot.instance, d.product.as_deref().unwrap()))
            .collect();
        assert_eq!(
            sticks,
            [
                (2, "T16000M Stick {B10A044F-0000-0000-0000-504944564944}"),
                (1, "T16000M Throttle {B10B044F-0000-0000-0000-504944564944}"),
            ]
        );

        let unlisted = applied("profile.xml", swap(3, 1));
        let expected = replaced_once(
            &exchanged(&original, "\"js1_", "\"js3_"),
            "type=\"joystick\" instance=\"1\"",
            "type=\"joystick\" instance=\"3\"",
        );
        assert_eq!(unlisted, expected);
        assert!(unlisted.contains("<rebind input=\"js1_button1\"/>"));
    }

    #[test]
    fn swap_joysticks_rejects_equal_or_zero() {
        let mut doc = doc("profile.xml");
        for (a, b) in [(1, 1), (0, 2), (2, 0), (0, 0)] {
            invalid(&mut doc, swap(a, b));
        }
    }

    #[test]
    fn swap_twice_restores_original_bytes() {
        for name in RECOGNISED {
            for (a, b) in [(1, 2), (1, 3), (2, 7)] {
                let bytes = fixture(name);
                let mut doc = parse(&bytes).unwrap();
                doc.apply(&swap(a, b)).unwrap();
                doc.apply(&swap(b, a)).unwrap();
                assert_eq!(doc.to_bytes(), bytes, "{name} {a} {b}");
            }
        }
    }

    #[test]
    fn unknown_elements_comments_and_attributes_survive_every_op() {
        let original = fixture_text("unknown_bits.xml");
        let survivors = [
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n",
            "  <futureThing mode=\"x\">\n   <child value=\"1\"/>\n  </futureThing>\n",
            "<action name=\"v_afterburner\" activationMode=\"hold\">\n    <!-- tuned for the stick -->\n",
        ];
        let untouched_rebind = "<rebind input=\"js1_button3\" curve=\"soft\"/>\n";
        let ops = [
            bind("spaceship_movement", "v_afterburner", "js1_button4"),
            bind("spaceship_movement", "v_afterburner", "mo1_mouse1"),
            bind("spaceship_movement", "v_brake", "kb1_b"),
            bind("vehicle_general", "v_horn", "kb1_h"),
            reset("spaceship_movement", "v_afterburner", Keyboard),
            reset("spaceship_weapons", "v_attack1", Joystick),
            unbind("spaceship_movement", "v_afterburner", slot(Keyboard, 1)),
            swap(1, 2),
        ];
        for op in ops {
            let edited = applied("unknown_bits.xml", op.clone());
            for survivor in survivors {
                assert!(edited.contains(survivor), "{op:?} lost {survivor:?}");
            }
        }
        assert_eq!(
            applied(
                "unknown_bits.xml",
                bind("spaceship_movement", "v_afterburner", "js1_button4")
            ),
            replaced_once(&original, "\"js1_button3\"", "\"js1_button4\"")
        );
        assert!(
            applied(
                "unknown_bits.xml",
                bind("spaceship_movement", "v_afterburner", "kb1_g")
            )
            .contains(untouched_rebind)
        );

        let original = fixture_text("two_profiles.xml");
        let other_profile = &original[..original.find(" <ActionProfiles version=\"1\" optionsVersion=\"2\" rebindVersion=\"2\" profileName=\"default\">").unwrap()];
        assert!(other_profile.contains("<unexpected/>"));
        for op in [
            swap(1, 2),
            bind("spaceship_movement", "v_afterburner", "js1_button4"),
            bind("spaceship_movement", "v_new", "js1_button4"),
            reset("spaceship_movement", "v_afterburner", Joystick),
        ] {
            let edited = applied("two_profiles.xml", op.clone());
            assert!(edited.starts_with(other_profile), "{op:?}");
        }
    }

    #[test]
    fn not_recognised_fixtures_fail_with_format_not_recognised() {
        let fixtures = [
            ("wrong_root.xml", 1),
            ("action_with_other_child.xml", 6),
            ("actionmap_without_name.xml", 3),
            ("doctype.xml", 1),
        ];
        let inline: [(&[u8], usize); 6] = [
            (
                b"<ActionMaps>\n <actionmap name=\"m\">\n  <action name=\"a\">\n   <rebind/>\n  </action>\n </actionmap>\n</ActionMaps>\n",
                4,
            ),
            (
                b"<ActionMaps>\n <actionmap name=\"m\">\n  <action>\n  </action>\n </actionmap>\n</ActionMaps>\n",
                3,
            ),
            (
                b"<ActionMaps>\n <actionmap name=\"m\">\n  <rebind input=\"kb1_a\"/>\n </actionmap>\n</ActionMaps>\n",
                3,
            ),
            (
                b"<ActionMaps>\n <ActionProfiles profileName=\"default\">\n  <actionmap name=\"m\">\n   <action name=\"a\">\n    <rebind input=\"kb1_a\"><x/></rebind>\n    <bogus/>\n   </action>\n  </actionmap>\n </ActionProfiles>\n</ActionMaps>\n",
                6,
            ),
            (b"<ActionMaps/>\n<ActionMaps/>\n", 2),
            (b"\n\n", 1),
        ];
        let cases = fixtures
            .iter()
            .map(|(name, line)| (name.to_string(), fixture(name), *line))
            .chain(
                inline
                    .iter()
                    .map(|(bytes, line)| (text(bytes), bytes.to_vec(), *line)),
            );
        for (label, bytes, expected_line) in cases {
            let (line, message) = not_recognised(&bytes);
            assert!(
                message.starts_with("format not recognised: "),
                "{label}: {message}"
            );
            assert!(message.len() > "format not recognised: ".len(), "{label}");
            assert_eq!(line, expected_line, "{label}: {message}");
        }
    }

    #[test]
    fn malformed_xml_is_parse_error_without_panic() {
        for (bytes, expected_line) in [
            (
                &b"<ActionMaps>\n <actionmap name=\"m\">\n</ActionMaps>\n"[..],
                3,
            ),
            (b"<ActionMaps>\n <actionmap name=\"m/>\n</ActionMaps>\n", 2),
            (
                b"<ActionMaps>\n <actionmap name=\"\xff\"/>\n</ActionMaps>\n",
                2,
            ),
        ] {
            let (line, message) = not_recognised(bytes);
            assert!(!message.starts_with("format not recognised"), "{message}");
            assert_eq!(line, expected_line, "{message}");
        }
    }

    #[test]
    fn empty_input_is_not_recognised() {
        for empty in [&b""[..], b"\xef\xbb\xbf"] {
            let (line, message) = not_recognised(empty);
            assert_eq!(message, "format not recognised: empty file");
            assert_eq!(line, 1);
        }
    }

    #[test]
    fn layout_file_name_validates() {
        assert_eq!(
            layout_file_name("my-layout_2").unwrap(),
            "layout_my-layout_2_exported.xml"
        );
        assert_eq!(
            layout_file_name("  Trimmed ").unwrap(),
            "layout_Trimmed_exported.xml"
        );
        for bad in ["", "   ", "a b", "a/b", "a\\b", "a.b", "caf\u{e9}"] {
            assert!(
                matches!(layout_file_name(bad), Err(Error::Invalid(_))),
                "{bad:?}"
            );
        }
    }
}
