use crate::{DeviceClass, Raw};

const ABS_HAT0X: u16 = 0x10;
const ABS_HAT3Y: u16 = 0x17;
const BTN_JOYSTICK: u16 = 0x120;
const BTN_JOYSTICK_LAST: u16 = 0x12f;
const BTN_SOUTH: u16 = 0x130;
const BTN_MISC: u16 = 0x100;
const BTN_DIGI: u16 = 0x140;
const BTN_DIGI_LAST: u16 = 0x14f;
const BTN_TRIGGER_HAPPY: u16 = 0x2c0;
const BTN_TRIGGER_HAPPY_LAST: u16 = 0x2e7;
const LAST_KEYBOARD_KEY: u16 = 0xff;
pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_ABS: u16 = 0x03;
pub const SYN_REPORT: u16 = 0;
pub const SYN_DROPPED: u16 = 3;

pub fn classify(keys: &[u16], axes: &[u16], accelerometer: bool) -> Option<DeviceClass> {
    if accelerometer {
        return None;
    }
    if keys.contains(&BTN_SOUTH) {
        return Some(DeviceClass::Gamepad);
    }
    let joystick_key = keys.iter().any(|k| {
        (BTN_JOYSTICK..=BTN_JOYSTICK_LAST).contains(k)
            || (BTN_TRIGGER_HAPPY..=BTN_TRIGGER_HAPPY_LAST).contains(k)
    });
    let non_stick_key = keys.iter().any(|k| {
        (1..=LAST_KEYBOARD_KEY).contains(k)
            || (BTN_MISC..BTN_JOYSTICK).contains(k)
            || (BTN_DIGI..=BTN_DIGI_LAST).contains(k)
    });
    let plain_axes = !axes.is_empty() && !non_stick_key;
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

pub struct Layout<'a> {
    pub class: DeviceClass,
    pub keys: &'a [u16],
    pub ranges: &'a [(u16, i32, i32)],
}

impl Layout<'_> {
    pub fn axis_reading(&self, code: u16, value: i32) -> Option<Raw> {
        let name = match self.class {
            DeviceClass::Stick => stick_axis(code)?,
            DeviceClass::Gamepad => pad_axis(code)?,
        };
        let &(_, min, max) = self.ranges.iter().find(|r| r.0 == code)?;
        Some(Raw::Axis {
            name,
            value,
            min,
            max,
        })
    }

    pub fn translate(&self, kind: u16, code: u16, value: i32) -> Option<Raw> {
        match kind {
            EV_KEY if value == 1 => {
                let name = match self.class {
                    DeviceClass::Stick => stick_button(self.keys, code)?,
                    DeviceClass::Gamepad => pad_button(code)?.to_string(),
                };
                Some(Raw::Pressed(name))
            }
            EV_ABS => match hat(code, value, self.class) {
                Some(name) => Some(Raw::Pressed(name)),
                None => self.axis_reading(code, value),
            },
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct Batcher {
    pending: Vec<Raw>,
    resyncing: bool,
}

impl Batcher {
    pub fn feed(
        &mut self,
        layout: &Layout,
        (kind, code, value): (u16, u16, i32),
    ) -> Option<Vec<Raw>> {
        if kind == EV_SYN && code == SYN_DROPPED {
            self.pending.clear();
            self.resyncing = true;
        } else if kind == EV_SYN && code == SYN_REPORT {
            if std::mem::take(&mut self.resyncing) || self.pending.is_empty() {
                return None;
            }
            return Some(std::mem::take(&mut self.pending));
        } else if !self.resyncing {
            self.pending.extend(layout.translate(kind, code, value));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_by_capabilities() {
        assert_eq!(
            classify(&[0x130, 0x131, 0x133], &[0, 1], false),
            Some(DeviceClass::Gamepad)
        );
        let joystick: Vec<u16> = (0x120..=0x12f).collect();
        assert_eq!(
            classify(&joystick, &[0, 1], false),
            Some(DeviceClass::Stick)
        );
        assert_eq!(classify(&[0x2c0], &[], false), Some(DeviceClass::Stick));
        assert_eq!(classify(&[30, 48], &[], false), None);
        assert_eq!(classify(&[0x110], &[], false), None);
        assert_eq!(classify(&[0x14a], &[0, 1], false), None);
        assert_eq!(classify(&[], &[0, 1, 6], false), Some(DeviceClass::Stick));
    }

    #[test]
    fn classify_rejects_look_alikes() {
        let tablet_pad: Vec<u16> = (0x100..=0x109).collect();
        assert_eq!(classify(&tablet_pad, &[0, 1, 8], false), None, "tablet pad");
        assert_eq!(
            classify(&[0x110, 0x111], &[0, 1], false),
            None,
            "absolute pointer"
        );
        assert_eq!(
            classify(&[], &[0, 1, 2, 3, 4, 5], true),
            None,
            "motion sensor"
        );
        assert_eq!(classify(&[0x140, 0x14a], &[0, 1], false), None, "tablet");
        assert_eq!(
            classify(&[0x130], &[0, 1], true),
            None,
            "pad flagged as sensor"
        );
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
        assert_eq!(hat(0x12, 1, pad), None, "second hat is not the d-pad");
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

    fn stick_layout() -> Layout<'static> {
        Layout {
            class: DeviceClass::Stick,
            keys: &[0x120, 0x121],
            ranges: &[(0, 0, 1023), (0x10, -1, 1)],
        }
    }

    #[test]
    fn only_a_fresh_press_counts() {
        let l = stick_layout();
        assert_eq!(
            l.translate(EV_KEY, 0x121, 1),
            Some(Raw::Pressed("button2".into()))
        );
        assert_eq!(l.translate(EV_KEY, 0x121, 2), None, "repeat");
        assert_eq!(l.translate(EV_KEY, 0x121, 0), None, "release");
        assert_eq!(l.translate(EV_KEY, 0x122, 1), None, "unknown key");
    }

    #[test]
    fn hat_centre_is_not_a_press_and_axes_carry_range() {
        let l = stick_layout();
        assert_eq!(l.translate(EV_ABS, 0x10, 0), None);
        assert_eq!(
            l.translate(EV_ABS, 0x10, 1),
            Some(Raw::Pressed("hat1_right".into()))
        );
        assert_eq!(
            l.translate(EV_ABS, 0, 600),
            Some(Raw::Axis {
                name: "x",
                value: 600,
                min: 0,
                max: 1023
            })
        );
        assert_eq!(l.translate(EV_ABS, 8, 5), None);
    }

    #[test]
    fn batches_end_at_syn_report() {
        let l = stick_layout();
        let mut b = Batcher::default();
        assert_eq!(b.feed(&l, (EV_KEY, 0x120, 1)), None);
        assert_eq!(b.feed(&l, (EV_KEY, 0x120, 2)), None);
        assert_eq!(
            b.feed(&l, (EV_SYN, SYN_REPORT, 0)),
            Some(vec![Raw::Pressed("button1".into())])
        );
        assert_eq!(b.feed(&l, (EV_SYN, SYN_REPORT, 0)), None, "empty batch");
    }

    #[test]
    fn a_dropped_block_is_discarded_through_its_report() {
        let l = stick_layout();
        let mut b = Batcher::default();
        assert_eq!(b.feed(&l, (EV_KEY, 0x120, 1)), None);
        assert_eq!(b.feed(&l, (EV_SYN, SYN_DROPPED, 0)), None);
        assert_eq!(b.feed(&l, (EV_KEY, 0x121, 1)), None, "compensation events");
        assert_eq!(b.feed(&l, (EV_SYN, SYN_REPORT, 0)), None);
        assert_eq!(b.feed(&l, (EV_KEY, 0x121, 1)), None);
        assert_eq!(
            b.feed(&l, (EV_SYN, SYN_REPORT, 0)),
            Some(vec![Raw::Pressed("button2".into())])
        );
    }
}
