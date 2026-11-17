use crate::Raw;

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PadState {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub thumb_lx: i16,
    pub thumb_ly: i16,
    pub thumb_rx: i16,
    pub thumb_ry: i16,
}

const BUTTON_BITS: [(u16, &str); 14] = [
    (0x1000, "a"),
    (0x2000, "b"),
    (0x4000, "x"),
    (0x8000, "y"),
    (0x0020, "back"),
    (0x0010, "start"),
    (0x0100, "shoulderl"),
    (0x0200, "shoulderr"),
    (0x0040, "thumbl"),
    (0x0080, "thumbr"),
    (0x0001, "dpad_up"),
    (0x0002, "dpad_down"),
    (0x0004, "dpad_left"),
    (0x0008, "dpad_right"),
];

fn trigger(name: &'static str, value: u8) -> Raw {
    Raw::Axis {
        name,
        value: i32::from(value),
        min: 0,
        max: i32::from(u8::MAX),
    }
}

fn thumb(name: &'static str, value: i16) -> Raw {
    Raw::Axis {
        name,
        value: i32::from(value),
        min: i32::from(i16::MIN),
        max: i32::from(i16::MAX),
    }
}

pub fn pad_raws(previous: Option<PadState>, now: PadState) -> Vec<Raw> {
    let mut raws = Vec::new();
    if let Some(before) = previous {
        let newly_set = now.buttons & !before.buttons;
        raws.extend(
            BUTTON_BITS
                .iter()
                .filter(|(bit, _)| newly_set & bit != 0)
                .map(|(_, name)| Raw::Pressed((*name).to_string())),
        );
    }
    raws.push(trigger("triggerl_btn", now.left_trigger));
    raws.push(trigger("triggerr_btn", now.right_trigger));
    raws.push(thumb("thumblx", now.thumb_lx));
    raws.push(thumb("thumbly", now.thumb_ly));
    raws.push(thumb("thumbrx", now.thumb_rx));
    raws.push(thumb("thumbry", now.thumb_ry));
    raws
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUTTONS: [(u16, &str); 14] = [
        (0x1000, "a"),
        (0x2000, "b"),
        (0x4000, "x"),
        (0x8000, "y"),
        (0x0020, "back"),
        (0x0010, "start"),
        (0x0100, "shoulderl"),
        (0x0200, "shoulderr"),
        (0x0040, "thumbl"),
        (0x0080, "thumbr"),
        (0x0001, "dpad_up"),
        (0x0002, "dpad_down"),
        (0x0004, "dpad_left"),
        (0x0008, "dpad_right"),
    ];

    fn pressed(raws: &[Raw]) -> Vec<&str> {
        raws.iter()
            .filter_map(|r| match r {
                Raw::Pressed(name) => Some(name.as_str()),
                Raw::Axis { .. } => None,
            })
            .collect()
    }

    #[test]
    fn xinput_first_reading_is_the_baseline() {
        let now = PadState {
            buttons: 0x1000,
            ..PadState::default()
        };
        let raws = pad_raws(None, now);
        assert!(pressed(&raws).is_empty());
        assert!(raws.iter().any(|r| matches!(r, Raw::Axis { .. })));
    }

    #[test]
    fn button_bits_name_pad_inputs() {
        for (bit, name) in BUTTONS {
            let now = PadState {
                buttons: bit,
                ..PadState::default()
            };
            let raws = pad_raws(Some(PadState::default()), now);
            assert_eq!(pressed(&raws), vec![name], "bit {bit:#06x}");
            assert!(pressed(&pad_raws(Some(now), now)).is_empty(), "held {name}");
        }
    }

    #[test]
    fn triggers_and_thumbs_are_axes() {
        let now = PadState {
            left_trigger: 200,
            thumb_rx: -32768,
            ..PadState::default()
        };
        let raws = pad_raws(Some(PadState::default()), now);
        assert!(raws.contains(&Raw::Axis {
            name: "triggerl_btn",
            value: 200,
            min: 0,
            max: 255
        }));
        assert!(raws.contains(&Raw::Axis {
            name: "thumbrx",
            value: -32768,
            min: -32768,
            max: 32767
        }));
    }
}
