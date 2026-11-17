use crate::DeviceClass;

const ABS_HAT0X: u16 = 0x10;
const ABS_HAT3Y: u16 = 0x17;
const BTN_JOYSTICK: u16 = 0x120;
const BTN_JOYSTICK_LAST: u16 = 0x12f;
const BTN_SOUTH: u16 = 0x130;
const BTN_TOUCH: u16 = 0x14a;
const BTN_TRIGGER_HAPPY: u16 = 0x2c0;
const BTN_TRIGGER_HAPPY_LAST: u16 = 0x2e7;
const LAST_KEYBOARD_KEY: u16 = 0xff;

pub fn classify(keys: &[u16], axes: &[u16]) -> Option<DeviceClass> {
    if keys.contains(&BTN_SOUTH) {
        return Some(DeviceClass::Gamepad);
    }
    let joystick_key = keys.iter().any(|k| {
        (BTN_JOYSTICK..=BTN_JOYSTICK_LAST).contains(k)
            || (BTN_TRIGGER_HAPPY..=BTN_TRIGGER_HAPPY_LAST).contains(k)
    });
    let letter_key = keys.iter().any(|k| (1..=LAST_KEYBOARD_KEY).contains(k));
    let touch = keys.contains(&BTN_TOUCH);
    let plain_axes = !axes.is_empty() && !touch && !letter_key;
    (joystick_key || plain_axes).then_some(DeviceClass::Stick)
}

pub fn stick_button(sorted_keys: &[u16], code: u16) -> Option<String> {
    let index = sorted_keys.iter().position(|&k| k == code)?;
    Some(format!("button{}", index + 1))
}

pub fn stick_axis(code: u16) -> Option<&'static str> {
    Some(match code {
        0x00 => "x",
        0x01 => "y",
        0x02 => "z",
        0x03 => "rotx",
        0x04 => "roty",
        0x05 => "rotz",
        0x06 => "slider1",
        0x07 => "slider2",
        _ => return None,
    })
}

pub fn pad_axis(code: u16) -> Option<&'static str> {
    Some(match code {
        0x00 => "thumblx",
        0x01 => "thumbly",
        0x03 => "thumbrx",
        0x04 => "thumbry",
        0x02 => "triggerl_btn",
        0x05 => "triggerr_btn",
        _ => return None,
    })
}

pub fn pad_button(code: u16) -> Option<&'static str> {
    Some(match code {
        0x130 => "a",
        0x131 => "b",
        0x133 => "x",
        0x134 => "y",
        0x136 => "shoulderl",
        0x137 => "shoulderr",
        0x13a => "back",
        0x13b => "start",
        0x13d => "thumbl",
        0x13e => "thumbr",
        0x220 => "dpad_up",
        0x221 => "dpad_down",
        0x222 => "dpad_left",
        0x223 => "dpad_right",
        _ => return None,
    })
}

pub fn hat(code: u16, value: i32, class: DeviceClass) -> Option<String> {
    if !(ABS_HAT0X..=ABS_HAT3Y).contains(&code) || value == 0 {
        return None;
    }
    let pair = (code - ABS_HAT0X) / 2;
    let vertical = (code - ABS_HAT0X) % 2 == 1;
    let direction = match (vertical, value < 0) {
        (false, true) => "left",
        (false, false) => "right",
        (true, true) => "up",
        (true, false) => "down",
    };
    match class {
        DeviceClass::Stick => Some(format!("hat{}_{direction}", pair + 1)),
        DeviceClass::Gamepad if pair == 0 => Some(format!("dpad_{direction}")),
        DeviceClass::Gamepad => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_by_capabilities() {
        assert_eq!(
            classify(&[0x130, 0x131, 0x133], &[0, 1]),
            Some(DeviceClass::Gamepad)
        );
        let joystick: Vec<u16> = (0x120..=0x12f).collect();
        assert_eq!(classify(&joystick, &[0, 1]), Some(DeviceClass::Stick));
        assert_eq!(classify(&[0x2c0], &[]), Some(DeviceClass::Stick));
        assert_eq!(classify(&[30, 48], &[]), None);
        assert_eq!(classify(&[0x110], &[]), None);
        assert_eq!(classify(&[0x14a], &[0, 1]), None);
        assert_eq!(classify(&[], &[0, 1, 6]), Some(DeviceClass::Stick));
    }

    #[test]
    fn buttons_count_in_code_order() {
        let keys = [0x120, 0x121, 0x12f, 0x2c0, 0x2c1];
        assert_eq!(stick_button(&keys, 0x120).as_deref(), Some("button1"));
        assert_eq!(stick_button(&keys, 0x12f).as_deref(), Some("button3"));
        assert_eq!(stick_button(&keys, 0x2c0).as_deref(), Some("button4"));
        assert_eq!(stick_button(&keys, 0x2c1).as_deref(), Some("button5"));
        assert_eq!(stick_button(&keys, 0x122), None);
    }

    #[test]
    fn stick_axes_follow_hid_usage() {
        let expected = [
            (0, Some("x")),
            (1, Some("y")),
            (2, Some("z")),
            (3, Some("rotx")),
            (4, Some("roty")),
            (5, Some("rotz")),
            (6, Some("slider1")),
            (7, Some("slider2")),
            (8, None),
        ];
        for (code, name) in expected {
            assert_eq!(stick_axis(code), name, "code {code}");
        }
    }

    #[test]
    fn hats_and_dpad() {
        let stick = DeviceClass::Stick;
        let pad = DeviceClass::Gamepad;
        assert_eq!(hat(0x10, -1, stick).as_deref(), Some("hat1_left"));
        assert_eq!(hat(0x10, 1, stick).as_deref(), Some("hat1_right"));
        assert_eq!(hat(0x11, -1, stick).as_deref(), Some("hat1_up"));
        assert_eq!(hat(0x11, 1, stick).as_deref(), Some("hat1_down"));
        assert_eq!(hat(0x12, 1, stick).as_deref(), Some("hat2_right"));
        assert_eq!(hat(0x10, -1, pad).as_deref(), Some("dpad_left"));
        assert_eq!(hat(0x10, 1, pad).as_deref(), Some("dpad_right"));
        assert_eq!(hat(0x11, -1, pad).as_deref(), Some("dpad_up"));
        assert_eq!(hat(0x11, 1, pad).as_deref(), Some("dpad_down"));
        assert_eq!(hat(0x10, 0, stick), None);
        assert_eq!(hat(0x00, 1, stick), None);
    }

    #[test]
    fn pad_names() {
        assert_eq!(pad_button(0x130), Some("a"));
        assert_eq!(pad_button(0x131), Some("b"));
        assert_eq!(
            pad_button(0x133),
            Some("x"),
            "follows xpad's BTN_X/BTN_Y naming (0x133 BTN_X, alias BTN_NORTH)"
        );
        assert_eq!(
            pad_button(0x134),
            Some("y"),
            "follows xpad's BTN_X/BTN_Y naming (0x134 BTN_Y, alias BTN_WEST)"
        );
        assert_eq!(pad_button(0x13a), Some("back"));
        assert_eq!(pad_button(0x13b), Some("start"));
        assert_eq!(pad_button(0x136), Some("shoulderl"));
        assert_eq!(pad_button(0x137), Some("shoulderr"));
        assert_eq!(pad_button(0x13d), Some("thumbl"));
        assert_eq!(pad_button(0x13e), Some("thumbr"));
        assert_eq!(pad_button(0x220), Some("dpad_up"));
        assert_eq!(pad_button(0x221), Some("dpad_down"));
        assert_eq!(pad_button(0x222), Some("dpad_left"));
        assert_eq!(pad_button(0x223), Some("dpad_right"));
        assert_eq!(pad_button(0x138), None);
        let axes = [
            (0, "thumblx"),
            (1, "thumbly"),
            (3, "thumbrx"),
            (4, "thumbry"),
            (2, "triggerl_btn"),
            (5, "triggerr_btn"),
        ];
        for (code, name) in axes {
            assert_eq!(pad_axis(code), Some(name), "code {code}");
        }
        assert_eq!(pad_axis(6), None);
    }
}
