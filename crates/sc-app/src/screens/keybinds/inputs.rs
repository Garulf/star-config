//! Input names per device: validation, key cap labels, and display names.

use super::capture;
use sc_core::actionmaps::{DeviceKind, Slot, format_input, parse_input};
use sc_core::defaults::GameData;

pub const AXES: [&str; 8] = ["x", "y", "z", "rotx", "roty", "rotz", "slider1", "slider2"];
pub const HAT_DIRECTIONS: [&str; 4] = ["up", "down", "left", "right"];
pub const QUICK_BUTTONS: u32 = 32;
/// The game numbers sticks js1 to js8.
pub const STICKS: u32 = 8;
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

/// What a typed input is checked against besides our own tables.
#[derive(Debug, Clone, Copy, Default)]
pub struct Known<'a> {
    /// Every key name the game's data lists is accepted.
    pub game: Option<&'a GameData>,
    /// The cell's game default, after its device prefix, which is always accepted.
    pub default: Option<&'a str>,
}

pub fn judge(slot: Slot, typed: &str, known: Known<'_>) -> Typed {
    let rest = typed.trim();
    if rest.is_empty() {
        return Typed::Empty;
    }
    let input = format_input(slot, rest);
    let round_trips = parse_input(&input) == Some((slot, rest));
    let is_default = known
        .default
        .is_some_and(|default| default.trim().eq_ignore_ascii_case(rest));
    if round_trips && (is_default || is_known(slot.kind, rest, known.game)) {
        Typed::Valid(input)
    } else {
        Typed::Invalid(format!("\"{rest}\" is not an input the game knows"))
    }
}

fn is_known(kind: DeviceKind, rest: &str, game: Option<&GameData>) -> bool {
    let lower = rest.to_ascii_lowercase();
    let parts: Vec<&str> = lower.split('+').collect();
    let Some((last, modifiers)) = parts.split_last() else {
        return false;
    };
    let named = |part: &str| game.is_some_and(|game| game_names(game, kind, part));
    let modifiers_ok = modifiers.iter().all(|part| {
        capture::is_modifier(part)
            || named(part)
            || (kind == DeviceKind::Gamepad && is_gamepad(part))
    });
    match kind {
        DeviceKind::Keyboard => {
            parts
                .iter()
                .all(|part| capture::is_key(part) || capture::is_modifier(part) || named(part))
                || (modifiers_ok && capture::is_mouse(last))
        }
        DeviceKind::Mouse => modifiers_ok && (capture::is_mouse(last) || named(last)),
        DeviceKind::Joystick => modifiers_ok && (is_joystick(last) || named(last)),
        DeviceKind::Gamepad => modifiers_ok && (is_gamepad(last) || named(last)),
    }
}

/// The keyboard column takes mouse buttons and the mouse column keyboard modifiers, so
/// either one's names count for both.
fn game_names(game: &GameData, kind: DeviceKind, part: &str) -> bool {
    let kinds: &[DeviceKind] = match kind {
        DeviceKind::Keyboard | DeviceKind::Mouse => &[DeviceKind::Keyboard, DeviceKind::Mouse],
        DeviceKind::Gamepad => &[DeviceKind::Gamepad],
        DeviceKind::Joystick => &[DeviceKind::Joystick],
    };
    kinds
        .iter()
        .any(|kind| game.input_names.contains_key(&(*kind, part.to_string())))
}

fn is_gamepad(name: &str) -> bool {
    GAMEPAD.iter().any(|(known, _)| *known == name)
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

/// Key cap labels for a whole input, in the game's words where it has them and in ours
/// for the rest. An input without a device prefix is shown as written.
pub fn input_caps(game: Option<&GameData>, input: &str) -> Vec<String> {
    let Some((slot, rest)) = parse_input(input) else {
        return vec![input.to_string()];
    };
    rest.split('+')
        .map(str::trim)
        .map(|part| {
            game.and_then(|game| game.input_label(&format_input(slot, part)))
                .filter(|named| !named.eq_ignore_ascii_case(part))
                .unwrap_or_else(|| cap(slot.kind, part))
        })
        .collect()
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

pub fn kind_label(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Keyboard => "Keyboard",
        DeviceKind::Mouse => "Mouse",
        DeviceKind::Gamepad => "Gamepad",
        DeviceKind::Joystick => "Joystick",
    }
}

/// The stick as the game's files name it, such as `js2`.
pub fn stick_label(slot: Slot) -> String {
    format!("js{}", slot.instance)
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
    const GP: Slot = Slot {
        kind: DeviceKind::Gamepad,
        instance: 1,
    };
    const NONE: Known<'static> = Known {
        game: None,
        default: None,
    };

    fn judge(slot: Slot, typed: &str) -> Typed {
        super::judge(slot, typed, NONE)
    }

    const KEY_NAMES: &str = r#"<LocalizedKeys>
 <device name="keyboard">
  <Key name="escape" localizationString="@k_esc"/>
  <Key name="pause" localizationString="@k_pause"/>
 </device>
 <device name="mouse">
  <Key name="mouse1_2" localizationString="@m_both"/>
  <Key name="maxis_z" localizationString="@m_wheel_axis"/>
 </device>
 <device name="control_pad">
  <Key name="thumbl_up" localizationString="@p_lup"/>
  <Key name="thumbr_left" localizationString="@p_rleft"/>
  <Key name="triggerl_r_btn" localizationString="@p_both_triggers"/>
 </device>
 <device name="joystick1">
  <Key name="rotz_neg" localizationString="@j_twist_left"/>
 </device>
</LocalizedKeys>"#;

    fn game() -> GameData {
        GameData::from_sources(
            "test".into(),
            &sc_core::defaults::Sources {
                profile: "<profile/>".into(),
                attributes: "<Attributes/>".into(),
                key_names: KEY_NAMES.into(),
                strings: String::new(),
            },
        )
        .unwrap()
    }

    #[test]
    fn with_game_data_every_key_name_it_lists_is_accepted() {
        let game = game();
        let known = Known {
            game: Some(&game),
            default: None,
        };
        for (slot, typed) in [
            (KB, "escape"),
            (KB, "pause"),
            (KB, "lctrl+escape"),
            (KB, "mouse1_2"),
            (MOUSE, "mouse1_2"),
            (MOUSE, "maxis_z"),
            (GP, "thumbl_up"),
            (GP, "shoulderl+thumbr_left"),
            (GP, "triggerl_r_btn"),
            (JS2, "rotz_neg"),
        ] {
            assert!(
                matches!(super::judge(slot, typed, known), Typed::Valid(_)),
                "{typed}"
            );
            assert!(matches!(judge(slot, typed), Typed::Invalid(_)), "{typed}");
        }
        assert!(matches!(
            super::judge(GP, "escape", known),
            Typed::Invalid(_)
        ));
        assert!(matches!(
            super::judge(JS2, "thumbl_up", known),
            Typed::Invalid(_)
        ));
    }

    #[test]
    fn gamepad_buttons_combine_with_each_other() {
        assert_eq!(
            judge(GP, "shoulderl+y"),
            Typed::Valid("gp1_shoulderl+y".into())
        );
        assert_eq!(
            judge(GP, "shoulderl+shoulderr"),
            Typed::Valid("gp1_shoulderl+shoulderr".into())
        );
        assert!(matches!(judge(GP, "shoulderl+nope"), Typed::Invalid(_)));
        assert!(matches!(judge(KB, "shoulderl+f"), Typed::Invalid(_)));
    }

    #[test]
    fn the_cells_own_default_is_always_accepted() {
        for (slot, odd) in [(KB, "]"), (KB, "HMD_Pitch"), (MOUSE, "maxis_z")] {
            let known = Known {
                game: None,
                default: Some(odd),
            };
            assert!(matches!(judge(slot, odd), Typed::Invalid(_)), "{odd}");
            assert_eq!(
                super::judge(slot, odd, known),
                Typed::Valid(format_input(slot, odd)),
                "{odd}"
            );
        }
        let known = Known {
            game: None,
            default: Some("HMD_Pitch"),
        };
        assert!(matches!(
            super::judge(KB, "HMD_Yaw", known),
            Typed::Invalid(_)
        ));
    }

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
        assert_eq!(judge(KB, "mouse3"), Typed::Valid("kb1_mouse3".into()));
        assert_eq!(
            judge(KB, "lctrl+mouse4"),
            Typed::Valid("kb1_lctrl+mouse4".into())
        );
        assert!(matches!(judge(KB, "mouse3+f"), Typed::Invalid(_)));
        assert!(matches!(judge(JS2, "button0"), Typed::Invalid(_)));
    }

    #[test]
    fn caps_name_keys_buttons_and_axes() {
        assert_eq!(input_caps(None, "kb1_lalt+f"), ["L Alt", "F"]);
        assert_eq!(input_caps(None, "js1_button5"), ["Button 5"]);
        assert_eq!(input_caps(None, "js1_Button3"), ["Button 3"]);
        assert_eq!(input_caps(None, "js1_x"), ["X axis"]);
        assert_eq!(input_caps(None, "mo1_mwheel_down"), ["Wheel down"]);
        assert_eq!(input_caps(None, "kb1_oddkey"), ["oddkey"]);
    }

    #[test]
    fn names_read_like_words() {
        assert_eq!(
            product_name("VKB Gladiator NXT L  {0200231D-0000-0000-0000-504944564944}"),
            "VKB Gladiator NXT L"
        );
        assert_eq!(slot_label(JS2), "Joystick 2");
        assert_eq!(slot_label(KB), "Keyboard");
        assert_eq!(stick_label(JS2), "js2");
        assert_eq!(kind_label(DeviceKind::Gamepad), "Gamepad");
    }

    #[test]
    fn input_caps_use_our_names_without_game_data() {
        assert_eq!(input_caps(None, "kb1_lalt+f"), ["L Alt", "F"]);
        assert_eq!(input_caps(None, "kb1_mouse3"), ["Middle click"]);
        assert_eq!(input_caps(None, "js2_button5"), ["Button 5"]);
        assert_eq!(input_caps(None, "odd"), ["odd"]);
    }
}
