use crate::Raw;

pub const AXIS_NAMES: [&str; 8] = ["x", "y", "z", "rotx", "roty", "rotz", "slider1", "slider2"];
pub const AXIS_RANGE: (i32, i32) = (0, 65535);
pub const DIPROP_RANGE_ID: usize = 4;
pub const DIPROP_GUIDANDPATH_ID: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StickState {
    pub axes: [i32; 8],
    pub povs: [u32; 4],
    pub buttons: [u8; 128],
}

impl Default for StickState {
    fn default() -> Self {
        Self {
            axes: [0; 8],
            povs: [u32::MAX; 4],
            buttons: [0; 128],
        }
    }
}

const BUTTON_DOWN: u8 = 0x80;
const POV_DIRECTIONS: [(u32, &str); 4] =
    [(0, "up"), (9000, "right"), (18000, "down"), (27000, "left")];

fn hat_name(index: usize, pov: u32) -> Option<String> {
    POV_DIRECTIONS
        .iter()
        .find(|(angle, _)| *angle == pov)
        .map(|(_, direction)| format!("hat{}_{direction}", index + 1))
}

fn presses(before: &StickState, now: &StickState) -> Vec<Raw> {
    let buttons = (0..now.buttons.len())
        .filter(|&i| now.buttons[i] & BUTTON_DOWN != 0 && before.buttons[i] & BUTTON_DOWN == 0)
        .map(|i| format!("button{}", i + 1));
    let hats = (0..now.povs.len())
        .filter(|&i| now.povs[i] != before.povs[i])
        .filter_map(|i| hat_name(i, now.povs[i]));
    buttons.chain(hats).map(Raw::Pressed).collect()
}

pub fn stick_raws(previous: Option<&StickState>, now: &StickState, present_axes: u8) -> Vec<Raw> {
    let mut raws = previous
        .map(|before| presses(before, now))
        .unwrap_or_default();
    let (min, max) = AXIS_RANGE;
    raws.extend(
        AXIS_NAMES
            .iter()
            .zip(now.axes)
            .enumerate()
            .filter(|(i, _)| present_axes & (1 << i) != 0)
            .map(|(_, (&name, value))| Raw::Axis {
                name,
                value,
                min,
                max,
            }),
    );
    raws
}

pub fn usb_id_from_guid_data1(data1: u32) -> (u16, u16) {
    ((data1 & 0xFFFF) as u16, (data1 >> 16) as u16)
}

pub fn property_key<T>(id: usize) -> *const T {
    core::ptr::without_provenance(id)
}

pub fn from_wide(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

pub fn xinput_owns(path: &str) -> bool {
    path.to_ascii_lowercase().contains("ig_")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pressed(raws: &[Raw]) -> Vec<&str> {
        raws.iter()
            .filter_map(|r| match r {
                Raw::Pressed(name) => Some(name.as_str()),
                Raw::Axis { .. } => None,
            })
            .collect()
    }

    fn axis_names(raws: &[Raw]) -> Vec<&'static str> {
        raws.iter()
            .filter_map(|r| match r {
                Raw::Axis { name, .. } => Some(*name),
                Raw::Pressed(_) => None,
            })
            .collect()
    }

    fn with_button(index: usize) -> StickState {
        let mut state = StickState::default();
        state.buttons[index] = 0x80;
        state
    }

    fn with_pov(index: usize, value: u32) -> StickState {
        let mut state = StickState::default();
        state.povs[index] = value;
        state
    }

    #[test]
    fn buttons_number_from_one() {
        let rest = StickState::default();
        assert_eq!(
            pressed(&stick_raws(Some(&rest), &with_button(0), 0)),
            ["button1"]
        );
        assert_eq!(
            pressed(&stick_raws(Some(&rest), &with_button(63), 0)),
            ["button64"]
        );
        let held = with_button(0);
        assert!(pressed(&stick_raws(Some(&held), &held, 0)).is_empty());
        assert!(pressed(&stick_raws(None, &held, 0xFF)).is_empty());
    }

    #[test]
    fn buttons_need_the_high_bit() {
        let mut low = StickState::default();
        low.buttons[2] = 0x01;
        assert!(pressed(&stick_raws(Some(&StickState::default()), &low, 0)).is_empty());
    }

    #[test]
    fn povs_are_hats() {
        let rest = StickState::default();
        for (value, name) in [
            (0, "hat1_up"),
            (9000, "hat1_right"),
            (18000, "hat1_down"),
            (27000, "hat1_left"),
        ] {
            assert_eq!(
                pressed(&stick_raws(Some(&rest), &with_pov(0, value), 0)),
                [name],
                "pov {value}"
            );
        }
        for value in [4500, 0xFFFF_FFFF] {
            assert!(
                pressed(&stick_raws(Some(&rest), &with_pov(0, value), 0)).is_empty(),
                "pov {value}"
            );
        }
        assert_eq!(
            pressed(&stick_raws(Some(&rest), &with_pov(1, 0), 0)),
            ["hat2_up"]
        );
        let up = with_pov(0, 0);
        assert!(pressed(&stick_raws(Some(&up), &up, 0)).is_empty());
        assert!(pressed(&stick_raws(None, &up, 0)).is_empty());
    }

    #[test]
    fn only_present_axes_are_reported() {
        let now = StickState {
            axes: [10, 20, 30, 40, 50, 60, 70, 80],
            ..StickState::default()
        };
        let raws = stick_raws(Some(&StickState::default()), &now, 0b0100_0011);
        assert_eq!(axis_names(&raws), ["x", "y", "slider1"]);
        let (min, max) = AXIS_RANGE;
        assert!(raws.contains(&Raw::Axis {
            name: "slider1",
            value: 70,
            min,
            max
        }));
        assert!(raws.contains(&Raw::Axis {
            name: "x",
            value: 10,
            min,
            max
        }));
    }

    #[test]
    fn first_reading_is_the_baseline() {
        let mut now = with_button(4);
        now.povs[0] = 0;
        let raws = stick_raws(None, &now, 0b1000_0000);
        assert!(pressed(&raws).is_empty());
        assert_eq!(axis_names(&raws), ["slider2"]);
    }

    #[test]
    fn guid_data1_holds_vid_and_pid() {
        assert_eq!(usb_id_from_guid_data1(0x0201_231D), (0x231D, 0x0201));
    }

    #[test]
    fn property_keys_are_small_integers() {
        assert_eq!(property_key::<u128>(DIPROP_RANGE_ID) as usize, 4);
        assert_eq!(property_key::<u128>(DIPROP_GUIDANDPATH_ID) as usize, 12);
    }

    #[test]
    fn wide_strings_stop_at_nul() {
        let mut buffer = [0u16; 8];
        for (slot, unit) in buffer.iter_mut().zip("VKB".encode_utf16()) {
            *slot = unit;
        }
        buffer[5] = u16::from(b'x');
        assert_eq!(from_wide(&buffer), "VKB");
        let full: Vec<u16> = "abc".encode_utf16().collect();
        assert_eq!(from_wide(&full), "abc");
    }

    #[test]
    fn xinput_paths_carry_ig() {
        assert!(xinput_owns(
            r"\\?\hid#vid_045e&pid_028e&ig_00#8&2b6fa4c&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}"
        ));
        assert!(xinput_owns(r"\\?\HID#VID_045E&PID_02FF&IG_00#9&1"));
        assert!(!xinput_owns(
            r"\\?\hid#vid_231d&pid_0201#7&1a2b3c4d&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}"
        ));
        assert!(!xinput_owns(""));
    }
}
