//! Input names per device: validation, key cap labels, and display names.

use super::capture;
use sc_core::actionmaps::{DeviceKind, Slot, format_input, parse_input};

pub const AXES: [&str; 8] = ["x", "y", "z", "rotx", "roty", "rotz", "slider1", "slider2"];
pub const HAT_DIRECTIONS: [&str; 4] = ["up", "down", "left", "right"];
pub const QUICK_BUTTONS: u32 = 32;
const MAX_BUTTON: u32 = 128;
const MAX_HAT: u32 = 4;

/// Gamepad inputs, from the same community documentation as the keyboard names.
const GAMEPAD: &[(&str, &str)] = &[
    ("a", "A"),
    ("b", "B"),
    ("x", "X"),
    ("y", "Y"),
    ("back", "Back"),
    ("start", "Start"),
    ("shoulderl", "L shoulder"),
    ("shoulderr", "R shoulder"),
    ("triggerl", "L trigger"),
    ("triggerr", "R trigger"),
    ("triggerl_btn", "L trigger"),
    ("triggerr_btn", "R trigger"),
    ("thumbl", "L stick"),
    ("thumbr", "R stick"),
    ("thumblx", "L stick X"),
    ("thumbly", "L stick Y"),
    ("thumbrx", "R stick X"),
    ("thumbry", "R stick Y"),
    ("dpad_up", "D-pad up"),
    ("dpad_down", "D-pad down"),
    ("dpad_left", "D-pad left"),
    ("dpad_right", "D-pad right"),
];

/// What the text field holds, judged for one device column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typed {
    Empty,
    Invalid(String),
    /// The whole input, device prefix included, such as `kb1_lalt+f`.
    Valid(String),
}

pub fn judge(slot: Slot, typed: &str) -> Typed {
    let rest = typed.trim();
    if rest.is_empty() {
        return Typed::Empty;
    }
    let input = format_input(slot, rest);
    let round_trips = parse_input(&input) == Some((slot, rest));
    if round_trips && is_known(slot.kind, rest) {
        Typed::Valid(input)
    } else {
        Typed::Invalid(format!("\"{rest}\" is not an input the game knows"))
    }
}

fn is_known(kind: DeviceKind, rest: &str) -> bool {
    let lower = rest.to_ascii_lowercase();
    let parts: Vec<&str> = lower.split('+').collect();
    let Some((last, modifiers)) = parts.split_last() else {
        return false;
    };
    let modifiers_ok = modifiers.iter().all(|part| capture::is_modifier(part));
    match kind {
        DeviceKind::Keyboard => parts
            .iter()
            .all(|part| capture::is_key(part) || capture::is_modifier(part)),
        DeviceKind::Mouse => modifiers_ok && capture::is_mouse(last),
        DeviceKind::Joystick => modifiers_ok && is_joystick(last),
        DeviceKind::Gamepad => modifiers_ok && GAMEPAD.iter().any(|(name, _)| name == last),
    }
}

fn numbered(name: &str, prefix: &str) -> Option<u32> {
    let digits = name.strip_prefix(prefix)?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn hat(name: &str) -> Option<(u32, &str)> {
    let (head, direction) = name.split_once('_')?;
    let number = numbered(head, "hat")?;
    HAT_DIRECTIONS
        .contains(&direction)
        .then_some((number, direction))
}

fn is_joystick(name: &str) -> bool {
    numbered(name, "button").is_some_and(|n| n <= MAX_BUTTON)
        || AXES.contains(&name)
        || hat(name).is_some_and(|(n, _)| n <= MAX_HAT)
}

/// Key cap labels for an input after its device prefix: `lalt+f` gives `L Alt`, `F`.
pub fn caps(kind: DeviceKind, rest: &str) -> Vec<String> {
    rest.split('+').map(|part| cap(kind, part.trim())).collect()
}

fn cap(kind: DeviceKind, part: &str) -> String {
    let lower = part.to_ascii_lowercase();
    let device = match kind {
        DeviceKind::Joystick => joystick_label(&lower),
        DeviceKind::Gamepad => GAMEPAD
            .iter()
            .find(|(name, _)| *name == lower)
            .map(|(_, label)| label.to_string()),
        DeviceKind::Keyboard | DeviceKind::Mouse => None,
    };
    device
        .or_else(|| capture::label_of(&lower).map(str::to_string))
        .unwrap_or_else(|| part.to_string())
}

fn joystick_label(name: &str) -> Option<String> {
    if let Some(n) = numbered(name, "button") {
        return Some(format!("Button {n}"));
    }
    if let Some((n, direction)) = hat(name) {
        return Some(format!("Hat {n} {direction}"));
    }
    if let Some(n) = numbered(name, "slider") {
        return Some(format!("Slider {n}"));
    }
    let axis = match name {
        "x" => "X axis",
        "y" => "Y axis",
        "z" => "Z axis",
        "rotx" => "X rotation",
        "roty" => "Y rotation",
        "rotz" => "Z rotation",
        _ => return None,
    };
    Some(axis.to_string())
}

/// The label a quick pick button shows for a joystick input.
pub fn quick_label(name: &str) -> String {
    if let Some(n) = numbered(name, "button") {
        return n.to_string();
    }
    if let Some((_, direction)) = hat(name) {
        return format!("Hat {direction}");
    }
    match name {
        "rotx" => "Rot X".to_string(),
        "roty" => "Rot Y".to_string(),
        "rotz" => "Rot Z".to_string(),
        "slider1" => "Slider 1".to_string(),
        "slider2" => "Slider 2".to_string(),
        other => other.to_ascii_uppercase(),
    }
}

pub fn quick_buttons() -> Vec<String> {
    (1..=QUICK_BUTTONS).map(|n| format!("button{n}")).collect()
}

pub fn quick_hats() -> Vec<String> {
    HAT_DIRECTIONS
        .iter()
        .map(|direction| format!("hat1_{direction}"))
        .collect()
}

pub fn slot_label(slot: Slot) -> String {
    let name = match slot.kind {
        DeviceKind::Keyboard => "Keyboard",
        DeviceKind::Mouse => "Mouse",
        DeviceKind::Joystick => return format!("Joystick {}", slot.instance),
        DeviceKind::Gamepad => "Gamepad",
    };
    if slot.instance == 1 {
        name.to_string()
    } else {
        format!("{name} {}", slot.instance)
    }
}

pub fn captures(kind: DeviceKind) -> bool {
    matches!(kind, DeviceKind::Keyboard | DeviceKind::Mouse)
}

const ACTION_PREFIXES: [&str; 2] = ["v_", "pc_"];

/// `spaceship_movement` becomes `Spaceship movement`, `v_toggle_mining_mode` becomes
/// `Toggle mining mode`. The game's own labels are not available.
pub fn humanise(raw: &str) -> String {
    let trimmed = ACTION_PREFIXES
        .iter()
        .find_map(|prefix| raw.strip_prefix(prefix))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(raw);
    capitalised(&trimmed.replace('_', " "))
}

fn capitalised(words: &str) -> String {
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A product name from the file without its device GUID.
pub fn product_name(product: &str) -> String {
    let without_guid = match product.find('{') {
        Some(start) => &product[..start],
        None => product,
    };
    without_guid
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const KB: Slot = Slot {
        kind: DeviceKind::Keyboard,
        instance: 1,
    };
    const JS2: Slot = Slot {
        kind: DeviceKind::Joystick,
        instance: 2,
    };
    const MOUSE: Slot = Slot {
        kind: DeviceKind::Mouse,
        instance: 1,
    };

    #[test]
    fn judge_accepts_known_inputs_and_names_bad_ones() {
        assert_eq!(judge(KB, "lalt+f"), Typed::Valid("kb1_lalt+f".into()));
        assert_eq!(judge(JS2, " button5 "), Typed::Valid("js2_button5".into()));
        assert_eq!(judge(JS2, "hat1_up"), Typed::Valid("js2_hat1_up".into()));
        assert_eq!(judge(JS2, "rotz"), Typed::Valid("js2_rotz".into()));
        assert_eq!(
            judge(MOUSE, "mwheel_up"),
            Typed::Valid("mo1_mwheel_up".into())
        );
        assert_eq!(judge(KB, ""), Typed::Empty);
        assert_eq!(
            judge(JS2, "buton5"),
            Typed::Invalid("\"buton5\" is not an input the game knows".into())
        );
        assert!(matches!(judge(KB, "mouse1"), Typed::Invalid(_)));
        assert!(matches!(judge(JS2, "button0"), Typed::Invalid(_)));
    }

    #[test]
    fn caps_name_keys_buttons_and_axes() {
        assert_eq!(caps(DeviceKind::Keyboard, "lalt+f"), ["L Alt", "F"]);
        assert_eq!(caps(DeviceKind::Joystick, "button5"), ["Button 5"]);
        assert_eq!(caps(DeviceKind::Joystick, "Button3"), ["Button 3"]);
        assert_eq!(caps(DeviceKind::Joystick, "x"), ["X axis"]);
        assert_eq!(caps(DeviceKind::Mouse, "mwheel_down"), ["Wheel down"]);
        assert_eq!(caps(DeviceKind::Keyboard, "oddkey"), ["oddkey"]);
    }

    #[test]
    fn names_read_like_words() {
        assert_eq!(humanise("spaceship_movement"), "Spaceship movement");
        assert_eq!(humanise("v_toggle_mining_mode"), "Toggle mining mode");
        assert_eq!(humanise("jump"), "Jump");
        assert_eq!(
            product_name("VKB Gladiator NXT L  {0200231D-0000-0000-0000-504944564944}"),
            "VKB Gladiator NXT L"
        );
        assert_eq!(slot_label(JS2), "Joystick 2");
        assert_eq!(slot_label(KB), "Keyboard");
    }
}
