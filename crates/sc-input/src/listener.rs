use std::collections::HashMap;

use crate::{DeviceKey, Raw};

#[derive(Default)]
pub struct Listener {
    rest: HashMap<(DeviceKey, &'static str), i32>,
}

impl Listener {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, device: DeviceKey, batch: &[Raw]) -> Option<String> {
        let mut moved = None;
        for raw in batch {
            match raw {
                Raw::Pressed(name) => return Some(name.clone()),
                Raw::Axis {
                    name,
                    value,
                    min,
                    max,
                } => {
                    let rest = *self.rest.entry((device, name)).or_insert(*value);
                    let past_threshold = (i64::from(*value) - i64::from(rest)).abs() * 4
                        > i64::from(*max) - i64::from(*min);
                    if past_threshold && moved.is_none() {
                        moved = Some((*name).to_string());
                    }
                }
            }
        }
        moved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axis(name: &'static str, value: i32, min: i32, max: i32) -> Raw {
        Raw::Axis {
            name,
            value,
            min,
            max,
        }
    }

    fn pressed(name: &str) -> Raw {
        Raw::Pressed(name.to_string())
    }

    #[test]
    fn first_press_wins() {
        let mut l = Listener::new();
        assert_eq!(l.feed(0, &[pressed("button3")]), Some("button3".into()));
    }

    #[test]
    fn press_beats_axis_in_one_batch() {
        let mut l = Listener::new();
        assert_eq!(l.feed(0, &[axis("x", 0, -60, 60)]), None);
        assert_eq!(
            l.feed(0, &[axis("x", 90, -60, 60), pressed("button1")]),
            Some("button1".into())
        );
    }

    #[test]
    fn resting_axis_at_an_end_needs_a_real_move() {
        let mut l = Listener::new();
        assert_eq!(l.feed(0, &[axis("slider1", 255, 0, 255)]), None);
        assert_eq!(l.feed(0, &[axis("slider1", 200, 0, 255)]), None);
        assert_eq!(l.feed(0, &[axis("slider1", 192, 0, 255)]), None);
        assert_eq!(
            l.feed(0, &[axis("slider1", 100, 0, 255)]),
            Some("slider1".into())
        );
    }

    #[test]
    fn jitter_below_quarter_range_never_binds() {
        let mut l = Listener::new();
        assert_eq!(l.feed(0, &[axis("x", 500, 0, 1000)]), None);
        for v in [503, 496, 600, 400, 700, 749, 750] {
            assert_eq!(l.feed(0, &[axis("x", v, 0, 1000)]), None, "value {v}");
        }
        assert_eq!(l.feed(0, &[axis("x", 751, 0, 1000)]), Some("x".into()));
    }

    #[test]
    fn centred_axis_binds_in_both_directions() {
        for target in [0, 65535] {
            let mut l = Listener::new();
            assert_eq!(l.feed(0, &[axis("x", 32767, 0, 65535)]), None);
            assert_eq!(l.feed(0, &[axis("x", target, 0, 65535)]), Some("x".into()));
        }
        for target in [-32768, 32767] {
            let mut l = Listener::new();
            assert_eq!(l.feed(0, &[axis("y", 0, -32768, 32767)]), None);
            assert_eq!(
                l.feed(0, &[axis("y", target, -32768, 32767)]),
                Some("y".into())
            );
        }
    }

    #[test]
    fn rest_is_per_device() {
        let mut l = Listener::new();
        assert_eq!(l.feed(0, &[axis("x", 0, 0, 100)]), None);
        assert_eq!(l.feed(1, &[axis("x", 100, 0, 100)]), None);
        assert_eq!(l.feed(1, &[axis("x", 0, 0, 100)]), Some("x".into()));
    }

    #[test]
    fn full_i32_range_does_not_overflow() {
        let mut l = Listener::new();
        let (min, max) = (i32::MIN, i32::MAX);
        assert_eq!(l.feed(0, &[axis("x", min, min, max)]), None);
        assert_eq!(l.feed(0, &[axis("x", min + 1000, min, max)]), None);
        assert_eq!(l.feed(0, &[axis("x", max, min, max)]), Some("x".into()));
    }
}
