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
                    let past_threshold = (value - rest).abs() * 2 > max - min;
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
        assert_eq!(
            l.feed(0, &[axis("slider1", 100, 0, 255)]),
            Some("slider1".into())
        );
    }

    #[test]
    fn jitter_below_half_range_never_binds() {
        let mut l = Listener::new();
        assert_eq!(l.feed(0, &[axis("x", 500, 0, 1000)]), None);
        for v in [503, 496, 900, 1000] {
            assert_eq!(l.feed(0, &[axis("x", v, 0, 1000)]), None, "value {v}");
        }
        let mut l = Listener::new();
        assert_eq!(l.feed(0, &[axis("x", 400, 0, 1000)]), None);
        assert_eq!(l.feed(0, &[axis("x", 1000, 0, 1000)]), Some("x".into()));
    }

    #[test]
    fn rest_is_per_device() {
        let mut l = Listener::new();
        assert_eq!(l.feed(0, &[axis("x", 0, 0, 100)]), None);
        assert_eq!(l.feed(1, &[axis("x", 100, 0, 100)]), None);
        assert_eq!(l.feed(1, &[axis("x", 0, 0, 100)]), Some("x".into()));
    }
}
