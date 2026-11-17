//! Advanced controls in actionmaps.xml: option group overrides and per-axis device options.

use super::{
    Device, DeviceKind, OPTIONS, Slot, Tree, attr, attr_value, check_name, device, kind_name,
    not_added,
};
use crate::error::{Error, Result};
use crate::xml_edit::{self, Attr, Element, escape_attr};
use std::cmp::Reverse;
use std::path::Path;

const DEVICE_OPTIONS: &str = "deviceoptions";
const AXIS_LINE: &str = "option";
const CURVE: &str = "nonlinearity_curve";
const POINT: &str = "point";

/// The `deviceoptions` of one product: the axis values the game keeps per device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceOptions {
    pub product: String,
    pub axes: Vec<AxisOption>,
}

/// One value of one axis. A line the game wrote twice is listed once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxisOption {
    pub input: String,
    pub field: AxisField,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisField {
    Deadzone,
    Saturation,
    Acceleration,
}

impl AxisField {
    const ALL: [Self; 3] = [Self::Deadzone, Self::Saturation, Self::Acceleration];

    fn attr_name(self) -> &'static str {
        match self {
            Self::Deadzone => "deadzone",
            Self::Saturation => "saturation",
            Self::Acceleration => "acceleration",
        }
    }
}

/// The option groups the user changed on one `options` slot. A slot never changed has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupOptions {
    pub kind: DeviceKind,
    pub instance: u32,
    pub product: Option<String>,
    pub groups: Vec<GroupSetting>,
}

/// One group element. Values stay as the file wrote them. `extra` holds attributes this app does not edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupSetting {
    pub group: String,
    pub invert: Option<bool>,
    pub exponent: Option<String>,
    pub sensitivity: Option<String>,
    pub curve: Option<Vec<(String, String)>>,
    pub extra: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupField {
    Invert,
    Exponent,
    Sensitivity,
}

impl GroupField {
    const ALL: [Self; 3] = [Self::Invert, Self::Exponent, Self::Sensitivity];

    fn attr_name(self) -> &'static str {
        match self {
            Self::Invert => "invert",
            Self::Exponent => "exponent",
            Self::Sensitivity => "sensitivity",
        }
    }

    fn check(self, value: &str) -> Result<()> {
        match self {
            Self::Invert if matches!(value, "0" | "1") => Ok(()),
            Self::Invert => Err(Error::Invalid(format!("invert is 0 or 1, not {value:?}"))),
            Self::Exponent | Self::Sensitivity => check_number(value),
        }
    }
}

pub(super) fn read_device_options(tree: &Tree) -> Vec<DeviceOptions> {
    let mut found: Vec<DeviceOptions> = Vec::new();
    for block in children_named(tree, tree.container, DEVICE_OPTIONS) {
        let Some(product) = attr_value(tree.element(block), "name") else {
            continue;
        };
        let at = found
            .iter()
            .position(|options| options.product == product)
            .unwrap_or_else(|| {
                found.push(DeviceOptions {
                    product: product.into(),
                    axes: Vec::new(),
                });
                found.len() - 1
            });
        for line in children_named(tree, block, AXIS_LINE) {
            let line = tree.element(line);
            let Some(input) = attr_value(line, "input") else {
                continue;
            };
            for field in AxisField::ALL {
                let Some(value) = attr_value(line, field.attr_name()) else {
                    continue;
                };
                let axes = &mut found[at].axes;
                if !axes.iter().any(|a| a.input == input && a.field == field) {
                    axes.push(AxisOption {
                        input: input.into(),
                        field,
                        value: value.into(),
                    });
                }
            }
        }
    }
    found
}

pub(super) fn read_group_options(tree: &Tree) -> Vec<GroupOptions> {
    children_named(tree, tree.container, OPTIONS)
        .filter_map(|index| {
            let Device { slot, product } = device(tree.element(index))?;
            let mut groups: Vec<GroupSetting> = Vec::new();
            for &child in &tree.children[index] {
                let setting = group_setting(tree, child);
                if !groups.iter().any(|g| g.group == setting.group) {
                    groups.push(setting);
                }
            }
            Some(GroupOptions {
                kind: slot.kind,
                instance: slot.instance,
                product,
                groups,
            })
        })
        .collect()
}

fn group_setting(tree: &Tree, index: usize) -> GroupSetting {
    let element = tree.element(index);
    let text = |field: GroupField| attr_value(element, field.attr_name()).map(String::from);
    GroupSetting {
        group: element.name.clone(),
        invert: attr_value(element, GroupField::Invert.attr_name()).and_then(|v| match v {
            "1" => Some(true),
            "0" => Some(false),
            _ => None,
        }),
        exponent: text(GroupField::Exponent),
        sensitivity: text(GroupField::Sensitivity),
        curve: children_named(tree, index, CURVE)
            .next()
            .map(|curve| points(tree, curve)),
        extra: element
            .attrs
            .iter()
            .filter(|a| GroupField::ALL.iter().all(|f| f.attr_name() != a.name))
            .map(|a| (a.name.clone(), a.value.clone()))
            .collect(),
    }
}

fn points(tree: &Tree, curve: usize) -> Vec<(String, String)> {
    children_named(tree, curve, POINT)
        .filter_map(|point| {
            let point = tree.element(point);
            Some((
                attr_value(point, "in")?.into(),
                attr_value(point, "out")?.into(),
            ))
        })
        .collect()
}

pub(super) fn set_group(
    path: &Path,
    text: &mut String,
    slot: Slot,
    group: &str,
    field: GroupField,
    value: &str,
) -> Result<()> {
    check_slot(slot)?;
    check_group(group)?;
    field.check(value)?;
    let tree = ensure_options(path, text, slot)?;
    let options =
        find_options(&tree, slot).ok_or_else(|| not_added(OPTIONS, kind_name(slot.kind)))?;
    let name = field.attr_name();
    match find_group(&tree, options, group) {
        None => {
            let child = format!("<{group} {name}=\"{}\"/>", escape_attr(value));
            xml_edit::append_child(text, tree.element(options), &child);
        }
        Some(index) => match attr(tree.element(index), name) {
            Some(existing) => xml_edit::set_attr(text, existing, value),
            None => xml_edit::insert_attr(text, tree.element(index), name, value),
        },
    }
    Ok(())
}

pub(super) fn reset_group(
    path: &Path,
    text: &mut String,
    slot: Slot,
    group: &str,
    field: GroupField,
) -> Result<()> {
    check_slot(slot)?;
    check_group(group)?;
    let tree = Tree::check(path, text)?;
    let Some(existing) = locate_group(&tree, slot, group)
        .and_then(|index| attr(tree.element(index), field.attr_name()))
    else {
        return Err(Error::Invalid(format!(
            "{group} has no {} setting on {} {}",
            field.attr_name(),
            kind_name(slot.kind),
            slot.instance
        )));
    };
    xml_edit::remove_attr(text, existing);
    tidy(path, text, slot, group)
}

pub(super) fn set_curve(
    path: &Path,
    text: &mut String,
    slot: Slot,
    group: &str,
    points: &[(String, String)],
) -> Result<()> {
    check_slot(slot)?;
    check_group(group)?;
    if points.is_empty() {
        return Err(Error::Invalid(
            "a curve needs at least one point; reset the curve to remove it".into(),
        ));
    }
    for (input, output) in points {
        check_number(input)?;
        check_number(output)?;
    }
    let tree = ensure_options(path, text, slot)?;
    let options =
        find_options(&tree, slot).ok_or_else(|| not_added(OPTIONS, kind_name(slot.kind)))?;
    if find_group(&tree, options, group).is_none() {
        xml_edit::append_child(text, tree.element(options), &format!("<{group}/>"));
    }
    let tree = Tree::check(path, text)?;
    let group_index = locate_group(&tree, slot, group).ok_or_else(|| not_added(OPTIONS, group))?;
    if children_named(&tree, group_index, CURVE).next().is_none() {
        xml_edit::append_child(text, tree.element(group_index), &format!("<{CURVE}/>"));
    }
    loop {
        let tree = Tree::check(path, text)?;
        let Some(old) = curve_of(&tree, slot, group)
            .and_then(|curve| children_named(&tree, curve, POINT).next())
        else {
            break;
        };
        xml_edit::remove_element(text, tree.element(old));
    }
    for (input, output) in points {
        let tree = Tree::check(path, text)?;
        let curve = curve_of(&tree, slot, group).ok_or_else(|| not_added(CURVE, group))?;
        let point = format!(
            "<{POINT} in=\"{}\" out=\"{}\"/>",
            escape_attr(input),
            escape_attr(output)
        );
        xml_edit::append_child(text, tree.element(curve), &point);
    }
    Ok(())
}

pub(super) fn reset_curve(path: &Path, text: &mut String, slot: Slot, group: &str) -> Result<()> {
    check_slot(slot)?;
    check_group(group)?;
    let tree = Tree::check(path, text)?;
    let Some(curve) = curve_of(&tree, slot, group) else {
        return Err(Error::Invalid(format!(
            "{group} has no response curve on {} {}",
            kind_name(slot.kind),
            slot.instance
        )));
    };
    xml_edit::remove_element(text, tree.element(curve));
    tidy(path, text, slot, group)
}

pub(super) fn set_axis(
    path: &Path,
    text: &mut String,
    product: &str,
    input: &str,
    field: AxisField,
    value: &str,
) -> Result<()> {
    check_product(product)?;
    check_name(input)?;
    check_number(value)?;
    let tree = Tree::check(path, text)?;
    let lines = axis_lines(&tree, product, input, field);
    if lines.is_empty() {
        return add_axis_line(path, text, product, input, field, value);
    }
    let mut attrs: Vec<&Attr> = lines
        .iter()
        .filter_map(|&line| attr(tree.element(line), field.attr_name()))
        .collect();
    attrs.sort_by_key(|attr| Reverse(attr.value_span.start));
    for attr in attrs {
        xml_edit::set_attr(text, attr, value);
    }
    Ok(())
}

fn add_axis_line(
    path: &Path,
    text: &mut String,
    product: &str,
    input: &str,
    field: AxisField,
    value: &str,
) -> Result<()> {
    let tree = Tree::check(path, text)?;
    if device_blocks(&tree, product).next().is_none() {
        let block = format!("<{DEVICE_OPTIONS} name=\"{}\"/>", escape_attr(product));
        insert_slot_element(text, &tree, &[DEVICE_OPTIONS], &block);
    }
    let tree = Tree::check(path, text)?;
    let block = device_blocks(&tree, product)
        .next()
        .ok_or_else(|| not_added(DEVICE_OPTIONS, product))?;
    let line = format!(
        "<{AXIS_LINE} input=\"{}\" {}=\"{}\"/>",
        escape_attr(input),
        field.attr_name(),
        escape_attr(value)
    );
    xml_edit::append_child(text, tree.element(block), &line);
    Ok(())
}

pub(super) fn reset_axis(
    path: &Path,
    text: &mut String,
    product: &str,
    input: &str,
    field: AxisField,
) -> Result<()> {
    check_product(product)?;
    check_name(input)?;
    let tree = Tree::check(path, text)?;
    let mut attrs: Vec<&Attr> = axis_lines(&tree, product, input, field)
        .into_iter()
        .filter_map(|line| attr(tree.element(line), field.attr_name()))
        .collect();
    if attrs.is_empty() {
        return Err(Error::Invalid(format!(
            "{product:?} has no {} for {input}",
            field.attr_name()
        )));
    }
    attrs.sort_by_key(|attr| Reverse(attr.value_span.start));
    for attr in attrs {
        xml_edit::remove_attr(text, attr);
    }
    remove_spent(path, text, |tree, text| {
        spent_line(tree, text, product, input)
    })?;
    remove_spent(path, text, |tree, text| spent_block(tree, text, product))
}

fn remove_spent(
    path: &Path,
    text: &mut String,
    find: impl Fn(&Tree, &str) -> Option<usize>,
) -> Result<()> {
    loop {
        let tree = Tree::check(path, text)?;
        let Some(spent) = find(&tree, text) else {
            return Ok(());
        };
        xml_edit::remove_element(text, tree.element(spent));
    }
}

fn spent_line(tree: &Tree, text: &str, product: &str, input: &str) -> Option<usize> {
    device_blocks(tree, product)
        .flat_map(|block| children_named(tree, block, AXIS_LINE))
        .find(|&line| {
            let line = tree.element(line);
            attr_value(line, "input") == Some(input)
                && line.attrs.len() == 1
                && is_hollow(text, line)
        })
}

fn spent_block(tree: &Tree, text: &str, product: &str) -> Option<usize> {
    device_blocks(tree, product).find(|&block| {
        let block = tree.element(block);
        block.attrs.len() == 1 && is_hollow(text, block)
    })
}

fn axis_lines(tree: &Tree, product: &str, input: &str, field: AxisField) -> Vec<usize> {
    device_blocks(tree, product)
        .flat_map(|block| children_named(tree, block, AXIS_LINE))
        .filter(|&line| {
            let line = tree.element(line);
            attr_value(line, "input") == Some(input) && attr(line, field.attr_name()).is_some()
        })
        .collect()
}

fn device_blocks<'a>(tree: &'a Tree, product: &'a str) -> impl Iterator<Item = usize> + 'a {
    children_named(tree, tree.container, DEVICE_OPTIONS)
        .filter(move |&block| attr_value(tree.element(block), "name") == Some(product))
}

fn children_named<'a>(
    tree: &'a Tree,
    parent: usize,
    name: &'a str,
) -> impl Iterator<Item = usize> + 'a {
    tree.children[parent]
        .iter()
        .copied()
        .filter(move |&index| tree.element(index).name == name)
}

fn find_options(tree: &Tree, slot: Slot) -> Option<usize> {
    children_named(tree, tree.container, OPTIONS)
        .find(|&index| device(tree.element(index)).is_some_and(|found| found.slot == slot))
}

fn find_group(tree: &Tree, options: usize, group: &str) -> Option<usize> {
    children_named(tree, options, group).next()
}

fn locate_group(tree: &Tree, slot: Slot, group: &str) -> Option<usize> {
    find_options(tree, slot).and_then(|options| find_group(tree, options, group))
}

fn curve_of(tree: &Tree, slot: Slot, group: &str) -> Option<usize> {
    locate_group(tree, slot, group).and_then(|index| children_named(tree, index, CURVE).next())
}

/// Adds the slot's `options` element, next to the game's own, when the file has none.
fn ensure_options(path: &Path, text: &mut String, slot: Slot) -> Result<Tree> {
    let tree = Tree::check(path, text)?;
    if find_options(&tree, slot).is_some() {
        return Ok(tree);
    }
    let options = format!(
        "<{OPTIONS} type=\"{}\" instance=\"{}\"/>",
        kind_name(slot.kind),
        slot.instance
    );
    insert_slot_element(text, &tree, &[OPTIONS, DEVICE_OPTIONS], &options);
    Tree::check(path, text)
}

/// Places a new element after the last element of the first of these names the file has, as the game orders them.
fn insert_slot_element(text: &mut String, tree: &Tree, after: &[&str], xml: &str) {
    let anchor = after
        .iter()
        .find_map(|name| children_named(tree, tree.container, name).last());
    match anchor {
        Some(anchor) => xml_edit::insert_after(text, tree.element(anchor), xml),
        None => xml_edit::append_child(text, tree.element(tree.container), xml),
    }
}

/// Drops the group element once it holds nothing and closes any element a removal left blank inside.
fn tidy(path: &Path, text: &mut String, slot: Slot, group: &str) -> Result<()> {
    let tree = Tree::check(path, text)?;
    if let Some(index) = locate_group(&tree, slot, group) {
        let element = tree.element(index);
        if element.attrs.is_empty() && is_hollow(text, element) {
            xml_edit::remove_element(text, element);
        } else {
            xml_edit::collapse_empty(text, element);
        }
    }
    let tree = Tree::check(path, text)?;
    if let Some(options) = find_options(&tree, slot) {
        xml_edit::collapse_empty(text, tree.element(options));
    }
    Ok(())
}

fn is_hollow(text: &str, element: &Element) -> bool {
    element
        .close_tag
        .as_ref()
        .is_none_or(|close| text[element.open_tag.end..close.start].trim().is_empty())
}

fn check_slot(slot: Slot) -> Result<()> {
    if slot.instance == 0 {
        return Err(Error::Invalid("device instances start at 1".into()));
    }
    Ok(())
}

fn check_group(group: &str) -> Result<()> {
    let mut chars = group.chars();
    let usable = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        && group != CURVE;
    if !usable {
        return Err(Error::Invalid(format!(
            "{group:?} is not a usable option group name"
        )));
    }
    Ok(())
}

fn check_product(product: &str) -> Result<()> {
    if product.trim().is_empty() {
        return Err(Error::Invalid("a device product name is needed".into()));
    }
    Ok(())
}

fn check_number(value: &str) -> Result<()> {
    match value.parse::<f64>() {
        Ok(number) if number.is_finite() => Ok(()),
        _ => Err(Error::Invalid(format!("{value:?} is not a number"))),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ActionMaps, Op};
    use super::*;
    use crate::staging::Document;
    use DeviceKind::{Gamepad, Joystick, Keyboard, Mouse};
    use std::fs;

    const PROFILE: &str = "real_profile.xml";
    const STICK_L: &str = " VKBsim Gladiator EVO L    {00000000-0000-0000-0000-000000000000}";
    const STICK_R: &str = " VKBsim Gladiator EVO R    {00000000-0000-0000-0000-000000000000}";
    const PEDALS: &str = "Thrustmaster T-Pedals  {00000000-0000-0000-0000-000000000000}";

    fn fixture(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/actionmaps")
            .join(name);
        String::from_utf8(fs::read(path).unwrap()).unwrap()
    }

    fn parse(text: &str) -> ActionMaps {
        ActionMaps::parse(Path::new("actionmaps.xml"), text.as_bytes()).unwrap()
    }

    fn bytes_of(doc: &ActionMaps) -> String {
        String::from_utf8(doc.to_bytes()).unwrap()
    }

    fn after(text: &str, ops: &[Op]) -> String {
        let mut doc = parse(text);
        for op in ops {
            doc.apply(op).unwrap();
        }
        bytes_of(&doc)
    }

    fn invalid(text: &str, op: Op) -> String {
        let mut doc = parse(text);
        let before = doc.clone();
        let message = match doc.apply(&op) {
            Err(Error::Invalid(message)) => message,
            other => panic!("expected Invalid for {op:?}, got {other:?}"),
        };
        assert_eq!(doc, before, "{op:?}");
        message
    }

    fn replaced_once(haystack: &str, from: &str, to: &str) -> String {
        assert_eq!(haystack.matches(from).count(), 1, "{from:?}");
        haystack.replacen(from, to, 1)
    }

    fn replaced_within(haystack: &str, block: &str, from: &str, to: &str, times: usize) -> String {
        let start = haystack.find(block).unwrap();
        let end = start + haystack[start..].find("</deviceoptions>").unwrap();
        let region = &haystack[start..end];
        assert_eq!(region.matches(from).count(), times, "{from:?}");
        format!(
            "{}{}{}",
            &haystack[..start],
            region.replace(from, to),
            &haystack[end..]
        )
    }

    fn set_group(
        kind: DeviceKind,
        instance: u32,
        group: &str,
        field: GroupField,
        value: &str,
    ) -> Op {
        Op::SetGroup {
            kind,
            instance,
            group: group.into(),
            field,
            value: value.into(),
        }
    }

    fn reset_group(kind: DeviceKind, instance: u32, group: &str, field: GroupField) -> Op {
        Op::ResetGroup {
            kind,
            instance,
            group: group.into(),
            field,
        }
    }

    fn set_curve(kind: DeviceKind, instance: u32, group: &str, points: &[(&str, &str)]) -> Op {
        Op::SetCurve {
            kind,
            instance,
            group: group.into(),
            points: points
                .iter()
                .map(|(i, o)| (i.to_string(), o.to_string()))
                .collect(),
        }
    }

    fn reset_curve(kind: DeviceKind, instance: u32, group: &str) -> Op {
        Op::ResetCurve {
            kind,
            instance,
            group: group.into(),
        }
    }

    fn set_axis(product: &str, input: &str, field: AxisField, value: &str) -> Op {
        Op::SetAxis {
            product: product.into(),
            input: input.into(),
            field,
            value: value.into(),
        }
    }

    fn reset_axis(product: &str, input: &str, field: AxisField) -> Op {
        Op::ResetAxis {
            product: product.into(),
            input: input.into(),
            field,
        }
    }

    fn group_options(doc: &ActionMaps, kind: DeviceKind, instance: u32) -> &GroupOptions {
        doc.group_options()
            .iter()
            .find(|o| o.kind == kind && o.instance == instance)
            .unwrap()
    }

    fn setting<'a>(options: &'a GroupOptions, group: &str) -> &'a GroupSetting {
        options.groups.iter().find(|g| g.group == group).unwrap()
    }

    fn axis(input: &str, field: AxisField, value: &str) -> AxisOption {
        AxisOption {
            input: input.into(),
            field,
            value: value.into(),
        }
    }

    const CURVED: &str = "<ActionMaps>\n <ActionProfiles profileName=\"default\">\n  <deviceoptions name=\"Gone Stick  {0}\">\n   <option input=\"x\" deadzone=\"0.1\"/>\n  </deviceoptions>\n  <options type=\"joystick\" instance=\"1\" Product=\"Stick\">\n   <flight_move_pitch invert=\"1\" exponent=\"2\" custom=\"yes\">\n    <nonlinearity_curve>\n     <point in=\"0.1\" out=\"0.015\"/>\n     <point in=\"0.5\" out=\"0.2\"/>\n    </nonlinearity_curve>\n   </flight_move_pitch>\n   <flight_move_yaw>\n    <nonlinearity_curve>\n     <point in=\"0.4\" out=\"0.0\"/>\n    </nonlinearity_curve>\n   </flight_move_yaw>\n   <flight_view sensitivity=\"1.5\"/>\n  </options>\n </ActionProfiles>\n</ActionMaps>\n";

    #[test]
    fn reads_device_and_group_options_from_real_fixture() {
        let doc = parse(&fixture(PROFILE));

        let products: Vec<&str> = doc
            .device_options()
            .iter()
            .map(|d| d.product.as_str())
            .collect();
        assert_eq!(products, [STICK_L, STICK_R]);
        let left = &doc.device_options()[0];
        assert_eq!(
            left.axes,
            [
                axis("x", AxisField::Saturation, "0.89099997"),
                axis("y", AxisField::Saturation, "0.89099997"),
                axis("z", AxisField::Saturation, "0.89099997"),
            ]
        );
        let right = &doc.device_options()[1];
        assert_eq!(right.axes.len(), 10);
        assert!(
            right
                .axes
                .contains(&axis("roty", AxisField::Saturation, "0.99000001"))
        );
        assert!(
            right
                .axes
                .contains(&axis("x", AxisField::Deadzone, "0.0198"))
        );
        assert!(right.axes.contains(&axis("rotz", AxisField::Deadzone, "0")));

        assert_eq!(doc.group_options().len(), 6);
        let stick = group_options(&doc, Joystick, 1);
        assert_eq!(stick.product.as_deref(), Some(STICK_R));
        let names: Vec<&str> = stick.groups.iter().map(|g| g.group.as_str()).collect();
        assert_eq!(
            names,
            ["flight_move_pitch", "flight_move_yaw", "flight_view"]
        );
        let pitch = setting(stick, "flight_move_pitch");
        assert_eq!(pitch.exponent.as_deref(), Some("2"));
        assert_eq!(pitch.invert, None);
        assert_eq!(pitch.sensitivity, None);
        assert_eq!(pitch.curve, None);
        assert!(pitch.extra.is_empty());
        assert_eq!(
            setting(group_options(&doc, Joystick, 2), "flight_move_space_brake").invert,
            Some(true)
        );
        assert_eq!(
            setting(group_options(&doc, Gamepad, 1), "flight_view")
                .exponent
                .as_deref(),
            Some("1")
        );
        let keyboard = group_options(&doc, Keyboard, 1);
        assert!(keyboard.groups.is_empty());
        assert!(
            keyboard
                .product
                .as_deref()
                .unwrap()
                .starts_with("Wine Keyboard")
        );
    }

    #[test]
    fn reads_curves_extra_attributes_and_a_missing_product() {
        let doc = parse(CURVED);
        let pitch = setting(group_options(&doc, Joystick, 1), "flight_move_pitch");
        assert_eq!(pitch.invert, Some(true));
        assert_eq!(pitch.exponent.as_deref(), Some("2"));
        assert_eq!(
            pitch.curve,
            Some(vec![
                ("0.1".to_string(), "0.015".to_string()),
                ("0.5".to_string(), "0.2".to_string())
            ])
        );
        assert_eq!(pitch.extra, [("custom".to_string(), "yes".to_string())]);
        let view = setting(group_options(&doc, Joystick, 1), "flight_view");
        assert_eq!(view.sensitivity.as_deref(), Some("1.5"));
        assert_eq!(view.curve, None);
    }

    #[test]
    fn set_axis_updates_every_copy_only() {
        let original = fixture(PROFILE);
        let edited = after(
            &original,
            &[set_axis(STICK_R, "x", AxisField::Saturation, "0.95")],
        );
        let expected = replaced_within(
            &original,
            "EVO R",
            "<option input=\"x\" saturation=\"0.89099997\"/>",
            "<option input=\"x\" saturation=\"0.95\"/>",
            2,
        );
        assert_eq!(edited, expected);
        let doc = parse(&edited);
        assert!(
            doc.device_options()[1]
                .axes
                .contains(&axis("x", AxisField::Saturation, "0.95"))
        );
        assert_eq!(
            doc.device_options()[0].axes[0],
            axis("x", AxisField::Saturation, "0.89099997")
        );
    }

    #[test]
    fn set_axis_adds_a_line_and_reset_removes_it_again() {
        let original = fixture(PROFILE);
        let added = after(
            &original,
            &[set_axis(STICK_R, "rotz", AxisField::Saturation, "0.5")],
        );
        assert!(added.contains(
            "<option input=\"rotz\" saturation=\"0.5\"/>\r\n  </deviceoptions>\r\n  <options type=\"keyboard\""
        ));
        let restored = after(
            &added,
            &[reset_axis(STICK_R, "rotz", AxisField::Saturation)],
        );
        assert_eq!(restored, original);
    }

    #[test]
    fn set_axis_creates_the_device_and_reset_removes_it() {
        let original = fixture(PROFILE);
        let edited = after(
            &original,
            &[set_axis(
                "Unplugged Pad  {0}",
                "x",
                AxisField::Deadzone,
                "0.1",
            )],
        );
        let doc = parse(&edited);
        assert_eq!(doc.device_options().len(), 3);
        assert_eq!(doc.device_options()[2].product, "Unplugged Pad  {0}");
        assert_eq!(
            doc.device_options()[2].axes,
            [axis("x", AxisField::Deadzone, "0.1")]
        );
        let restored = after(
            &edited,
            &[reset_axis("Unplugged Pad  {0}", "x", AxisField::Deadzone)],
        );
        assert_eq!(restored, original);
    }

    #[test]
    fn reset_axis_removes_every_copy_and_keeps_other_fields() {
        let original = fixture(PROFILE);
        let edited = after(
            &original,
            &[reset_axis(STICK_R, "x", AxisField::Saturation)],
        );
        let expected = replaced_within(
            &original,
            "EVO R",
            "   <option input=\"x\" saturation=\"0.89099997\"/>\r\n",
            "",
            2,
        );
        assert_eq!(edited, expected);
        assert!(edited.contains("<option input=\"x\" deadzone=\"0.0198\"/>"));
    }

    #[test]
    fn reset_axis_keeps_a_line_that_has_other_values() {
        let text = "<ActionMaps>\n <ActionProfiles profileName=\"default\">\n  <deviceoptions name=\"Stick\">\n   <option input=\"x\" deadzone=\"0.1\" saturation=\"0.9\"/>\n  </deviceoptions>\n </ActionProfiles>\n</ActionMaps>\n";
        let edited = after(text, &[reset_axis("Stick", "x", AxisField::Deadzone)]);
        assert_eq!(edited, text.replace(" deadzone=\"0.1\"", ""));
    }

    #[test]
    fn orphan_device_options_are_kept() {
        let doc = parse(CURVED);
        assert_eq!(doc.device_options().len(), 1);
        assert_eq!(doc.device_options()[0].product, "Gone Stick  {0}");
        assert!(
            doc.group_options()
                .iter()
                .all(|o| o.product.as_deref() != Some("Gone Stick  {0}"))
        );
        assert_eq!(bytes_of(&doc), CURVED);

        let edited = after(
            CURVED,
            &[set_axis("Gone Stick  {0}", "x", AxisField::Deadzone, "0.2")],
        );
        assert_eq!(
            edited,
            CURVED.replace("deadzone=\"0.1\"", "deadzone=\"0.2\"")
        );
        let cleared = after(
            CURVED,
            &[reset_axis("Gone Stick  {0}", "x", AxisField::Deadzone)],
        );
        assert!(!cleared.contains("deviceoptions"));
    }

    #[test]
    fn set_group_creates_child_and_reset_removes_it() {
        let original = fixture(PROFILE);
        let edited = after(
            &original,
            &[set_group(
                Joystick,
                2,
                "flight_move_roll",
                GroupField::Invert,
                "1",
            )],
        );
        let expected = replaced_once(
            &original,
            "   <flight_move_space_brake invert=\"1\"/>\r\n  </options>\r\n  <options type=\"joystick\" instance=\"3\"",
            "   <flight_move_space_brake invert=\"1\"/>\r\n   <flight_move_roll invert=\"1\"/>\r\n  </options>\r\n  <options type=\"joystick\" instance=\"3\"",
        );
        assert_eq!(edited, expected);
        let doc = parse(&edited);
        assert_eq!(
            setting(group_options(&doc, Joystick, 2), "flight_move_roll").invert,
            Some(true)
        );
        let restored = after(
            &edited,
            &[reset_group(
                Joystick,
                2,
                "flight_move_roll",
                GroupField::Invert,
            )],
        );
        assert_eq!(restored, original);
    }

    #[test]
    fn set_group_on_an_empty_slot_opens_it_and_reset_closes_it_again() {
        let original = fixture(PROFILE);
        let edited = after(
            &original,
            &[set_group(
                Keyboard,
                1,
                "mouse_view_pitch",
                GroupField::Sensitivity,
                "0.5",
            )],
        );
        assert!(edited.contains("Product=\"Wine Keyboard  {00000000-0000-0000-0000-000000000000}\">\r\n   <mouse_view_pitch sensitivity=\"0.5\"/>\r\n  </options>"));
        let restored = after(
            &edited,
            &[reset_group(
                Keyboard,
                1,
                "mouse_view_pitch",
                GroupField::Sensitivity,
            )],
        );
        assert_eq!(restored, original);
    }

    #[test]
    fn set_group_changes_or_adds_one_attribute_in_place() {
        let original = fixture(PROFILE);
        let changed = after(
            &original,
            &[set_group(
                Joystick,
                1,
                "flight_move_pitch",
                GroupField::Exponent,
                "1.5",
            )],
        );
        assert_eq!(
            changed,
            replaced_once(
                &original,
                "<flight_move_pitch exponent=\"2\"/>",
                "<flight_move_pitch exponent=\"1.5\"/>"
            )
        );
        let added = after(
            &original,
            &[set_group(
                Joystick,
                1,
                "flight_move_pitch",
                GroupField::Sensitivity,
                "0.8",
            )],
        );
        assert_eq!(
            added,
            replaced_once(
                &original,
                "<flight_move_pitch exponent=\"2\"/>",
                "<flight_move_pitch exponent=\"2\" sensitivity=\"0.8\"/>"
            )
        );
        let restored = after(
            &added,
            &[reset_group(
                Joystick,
                1,
                "flight_move_pitch",
                GroupField::Sensitivity,
            )],
        );
        assert_eq!(restored, original);
        let emptied = after(
            &original,
            &[reset_group(
                Joystick,
                1,
                "flight_move_pitch",
                GroupField::Exponent,
            )],
        );
        assert_eq!(
            emptied,
            replaced_once(&original, "   <flight_move_pitch exponent=\"2\"/>\r\n", "")
        );
    }

    #[test]
    fn set_group_creates_a_missing_slot_and_keeps_it_bare_on_reset() {
        let original = fixture(PROFILE);
        let edited = after(
            &original,
            &[set_group(
                Joystick,
                6,
                "flight_view",
                GroupField::Exponent,
                "3",
            )],
        );
        assert!(edited.contains("  <options type=\"joystick\" instance=\"6\">\r\n   <flight_view exponent=\"3\"/>\r\n  </options>\r\n"));
        let mouse = after(
            &original,
            &[set_group(
                Mouse,
                1,
                "mouse_view_yaw",
                GroupField::Invert,
                "0",
            )],
        );
        assert!(!mouse.contains("type=\"mouse\""));
        assert_eq!(
            setting(group_options(&parse(&mouse), Keyboard, 1), "mouse_view_yaw").invert,
            Some(false)
        );
        assert_eq!(
            after(
                &mouse,
                &[reset_group(Mouse, 1, "mouse_view_yaw", GroupField::Invert)]
            ),
            original
        );
        let reset = after(
            &edited,
            &[reset_group(
                Joystick,
                6,
                "flight_view",
                GroupField::Exponent,
            )],
        );
        assert_eq!(
            reset,
            replaced_once(
                &original,
                "  <modifiers />",
                "  <options type=\"joystick\" instance=\"6\"/>\r\n  <modifiers />"
            )
        );
    }

    #[test]
    fn set_curve_writes_points_in_order() {
        let original = fixture(PROFILE);
        let points = [("0.1", "0.015"), ("0.5", "0.2"), ("0.3", "0.1")];
        let edited = after(
            &original,
            &[set_curve(Joystick, 1, "flight_move_pitch", &points)],
        );
        let expected = replaced_once(
            &original,
            "   <flight_move_pitch exponent=\"2\"/>\r\n",
            "   <flight_move_pitch exponent=\"2\">\r\n    <nonlinearity_curve>\r\n     <point in=\"0.1\" out=\"0.015\"/>\r\n     <point in=\"0.5\" out=\"0.2\"/>\r\n     <point in=\"0.3\" out=\"0.1\"/>\r\n    </nonlinearity_curve>\r\n   </flight_move_pitch>\r\n",
        );
        assert_eq!(edited, expected);
        let doc = parse(&edited);
        let curve = setting(group_options(&doc, Joystick, 1), "flight_move_pitch")
            .curve
            .clone()
            .unwrap();
        let wanted: Vec<(String, String)> = points
            .iter()
            .map(|(i, o)| (i.to_string(), o.to_string()))
            .collect();
        assert_eq!(curve, wanted);
    }

    #[test]
    fn set_curve_replaces_an_existing_curve() {
        let edited = after(
            CURVED,
            &[set_curve(
                Joystick,
                1,
                "flight_move_pitch",
                &[("0.2", "0.1")],
            )],
        );
        let expected = replaced_once(
            CURVED,
            "     <point in=\"0.1\" out=\"0.015\"/>\n     <point in=\"0.5\" out=\"0.2\"/>\n",
            "     <point in=\"0.2\" out=\"0.1\"/>\n",
        );
        assert_eq!(edited, expected);
    }

    #[test]
    fn set_curve_creates_the_group_and_reset_removes_it_again() {
        let original = fixture(PROFILE);
        let edited = after(
            &original,
            &[set_curve(Gamepad, 1, "flight_move_roll", &[("0.4", "0.0")])],
        );
        assert!(edited.contains("   <flight_move_roll>\r\n    <nonlinearity_curve>\r\n     <point in=\"0.4\" out=\"0.0\"/>\r\n    </nonlinearity_curve>\r\n   </flight_move_roll>\r\n"));
        let restored = after(&edited, &[reset_curve(Gamepad, 1, "flight_move_roll")]);
        assert_eq!(restored, original);

        let with_curve = after(
            &original,
            &[set_curve(
                Joystick,
                1,
                "flight_move_pitch",
                &[("0.4", "0.0")],
            )],
        );
        let restored = after(
            &with_curve,
            &[reset_curve(Joystick, 1, "flight_move_pitch")],
        );
        assert_eq!(restored, original);
    }

    #[test]
    fn reset_curve_removes_the_curve_only() {
        let edited = after(CURVED, &[reset_curve(Joystick, 1, "flight_move_pitch")]);
        let expected = replaced_once(
            CURVED,
            "   <flight_move_pitch invert=\"1\" exponent=\"2\" custom=\"yes\">\n    <nonlinearity_curve>\n     <point in=\"0.1\" out=\"0.015\"/>\n     <point in=\"0.5\" out=\"0.2\"/>\n    </nonlinearity_curve>\n   </flight_move_pitch>\n",
            "   <flight_move_pitch invert=\"1\" exponent=\"2\" custom=\"yes\"/>\n",
        );
        assert_eq!(edited, expected);

        let edited = after(CURVED, &[reset_curve(Joystick, 1, "flight_move_yaw")]);
        let expected = replaced_once(
            CURVED,
            "   <flight_move_yaw>\n    <nonlinearity_curve>\n     <point in=\"0.4\" out=\"0.0\"/>\n    </nonlinearity_curve>\n   </flight_move_yaw>\n",
            "",
        );
        assert_eq!(edited, expected);
    }

    #[test]
    fn reset_group_keeps_a_group_that_still_has_a_curve() {
        let edited = after(
            CURVED,
            &[
                reset_group(Joystick, 1, "flight_move_pitch", GroupField::Invert),
                reset_group(Joystick, 1, "flight_move_pitch", GroupField::Exponent),
            ],
        );
        assert_eq!(edited, CURVED.replace(" invert=\"1\" exponent=\"2\"", ""));
    }

    #[test]
    fn edits_leave_bindings_and_devices_alone() {
        let original = parse(&fixture(PROFILE));
        let mut doc = original.clone();
        doc.apply(&set_group(
            Joystick,
            3,
            "flight_view",
            GroupField::Exponent,
            "2",
        ))
        .unwrap();
        doc.apply(&set_axis(PEDALS, "x", AxisField::Deadzone, "0.1"))
            .unwrap();
        assert_eq!(doc.bindings(), original.bindings());
        assert_eq!(doc.devices(), original.devices());
    }

    #[test]
    fn bad_values_and_missing_targets_are_invalid_and_change_nothing() {
        let text = fixture(PROFILE);
        let bad = [
            set_group(Joystick, 0, "flight_view", GroupField::Exponent, "2"),
            set_group(Joystick, 1, "flight view", GroupField::Exponent, "2"),
            set_group(Joystick, 1, "1flight", GroupField::Exponent, "2"),
            set_group(Joystick, 1, "flight_view", GroupField::Exponent, "fast"),
            set_group(Joystick, 1, "flight_view", GroupField::Sensitivity, "NaN"),
            set_group(Joystick, 1, "flight_view", GroupField::Invert, "2"),
            set_curve(Joystick, 1, "flight_view", &[]),
            set_curve(Joystick, 1, "flight_view", &[("0.1", "x")]),
            set_axis(STICK_R, "x", AxisField::Deadzone, "wide"),
            set_axis("", "x", AxisField::Deadzone, "0.1"),
            set_axis(STICK_R, "", AxisField::Deadzone, "0.1"),
            reset_group(Joystick, 1, "flight_move_pitch", GroupField::Invert),
            reset_group(Joystick, 9, "flight_move_pitch", GroupField::Exponent),
            reset_group(Joystick, 1, "no_such_group", GroupField::Exponent),
            reset_curve(Joystick, 1, "flight_move_pitch"),
            reset_axis(STICK_R, "x", AxisField::Acceleration),
            reset_axis("No Such Stick", "x", AxisField::Saturation),
        ];
        for op in bad {
            assert!(!invalid(&text, op).is_empty());
        }
    }

    #[test]
    fn values_that_need_escaping_cannot_break_the_file() {
        let edited = after(
            &fixture(PROFILE),
            &[set_axis(
                "Stick \"A\" & <B>  {0}",
                "x",
                AxisField::Deadzone,
                "0.1",
            )],
        );
        let doc = parse(&edited);
        assert_eq!(doc.device_options()[2].product, "Stick \"A\" & <B>  {0}");
    }

    #[test]
    fn real_fixture_with_no_edits_reads_without_changing_bytes() {
        let original = fixture(PROFILE);
        assert_eq!(bytes_of(&parse(&original)), original);
    }
}
