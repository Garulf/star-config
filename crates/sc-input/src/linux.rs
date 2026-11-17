use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use std::os::fd::AsFd;

use evdev::Device;
use evdev::raw_stream::RawDevice;

use crate::evdev_map::{self, Batcher, Layout};
use crate::{Backend, DeviceClass, DeviceInfo, DeviceKey, Raw};

const INPUT_DIR: &str = "/dev/input";
const UDEV_DATA_DIR: &str = "/run/udev/data";
const JOYSTICK_TAG: &str = "E:ID_INPUT_JOYSTICK=1";
const PROP_ACCELEROMETER: u16 = 0x06;

struct Opened {
    device: RawDevice,
    class: DeviceClass,
    keys: Vec<u16>,
    ranges: Vec<(u16, i32, i32)>,
    batcher: Batcher,
    initial: Option<Vec<Raw>>,
}

pub struct EvdevBackend {
    opened: Vec<Opened>,
    infos: Vec<DeviceInfo>,
    unreadable: usize,
}

pub fn open(class: DeviceClass) -> EvdevBackend {
    let mut backend = EvdevBackend {
        opened: Vec::new(),
        infos: Vec::new(),
        unreadable: 0,
    };
    for path in event_nodes() {
        match open_nonblocking(&path) {
            Ok(device) => backend.adopt(device, class),
            Err(e) if e.kind() == ErrorKind::PermissionDenied && tagged_joystick(&path) => {
                backend.unreadable += 1;
            }
            Err(_) => {}
        }
    }
    backend
}

fn event_nodes() -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(INPUT_DIR) else {
        return Vec::new();
    };
    let mut nodes: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("event"))
        })
        .collect();
    nodes.sort();
    nodes
}

fn tagged_joystick(node: &Path) -> bool {
    let Ok(meta) = fs::metadata(node) else {
        return false;
    };
    let file = Path::new(UDEV_DATA_DIR).join(format!("c13:{}", minor(meta.rdev())));
    fs::read_to_string(file).is_ok_and(|data| has_joystick_tag(&data))
}

fn minor(rdev: u64) -> u64 {
    ((rdev >> 12) & 0xffff_ff00) | (rdev & 0xff)
}

fn has_joystick_tag(udev_data: &str) -> bool {
    udev_data.lines().any(|line| line == JOYSTICK_TAG)
}

fn open_nonblocking(path: &Path) -> std::io::Result<RawDevice> {
    let probe = Device::open(path)?;
    probe.set_nonblocking(true)?;
    let shared_description = probe.as_fd().try_clone_to_owned()?;
    RawDevice::from_fd(shared_description)
}

impl EvdevBackend {
    fn adopt(&mut self, device: RawDevice, wanted: DeviceClass) {
        let keys: Vec<u16> = device
            .supported_keys()
            .map(|set| set.iter().map(|k| k.0).collect())
            .unwrap_or_default();
        let axes: Vec<u16> = device
            .supported_absolute_axes()
            .map(|set| set.iter().map(|a| a.0).collect())
            .unwrap_or_default();
        let accelerometer = device
            .properties()
            .iter()
            .any(|p| p.0 == PROP_ACCELEROMETER);
        if evdev_map::classify(&keys, &axes, accelerometer) != Some(wanted) {
            return;
        }
        let Ok(state) = device.get_abs_state() else {
            return;
        };
        let ranges: Vec<(u16, i32, i32)> = axes
            .iter()
            .filter_map(|&code| {
                let info = state.get(usize::from(code))?;
                Some((code, info.minimum, info.maximum))
            })
            .collect();
        let layout = Layout {
            class: wanted,
            keys: &keys,
            ranges: &ranges,
        };
        let initial: Vec<Raw> = axes
            .iter()
            .filter_map(|&code| {
                let info = state.get(usize::from(code))?;
                layout.axis_reading(code, info.value)
            })
            .collect();
        let id = device.input_id();
        self.infos.push(DeviceInfo {
            vendor: id.vendor(),
            product: id.product(),
            name: device.name().unwrap_or_default().to_string(),
        });
        self.opened.push(Opened {
            device,
            class: wanted,
            keys,
            ranges,
            batcher: Batcher::default(),
            initial: (!initial.is_empty()).then_some(initial),
        });
    }
}

impl Opened {
    fn drain(&mut self) -> Result<Vec<Vec<Raw>>, ()> {
        let events: Vec<(u16, u16, i32)> = match self.device.fetch_events() {
            Ok(events) => events
                .map(|e| (e.event_type().0, e.code(), e.value()))
                .collect(),
            Err(e) if e.kind() == ErrorKind::WouldBlock => Vec::new(),
            Err(_) => return Err(()),
        };
        let layout = Layout {
            class: self.class,
            keys: &self.keys,
            ranges: &self.ranges,
        };
        let mut batches: Vec<Vec<Raw>> = self.initial.take().into_iter().collect();
        batches.extend(
            events
                .into_iter()
                .filter_map(|event| self.batcher.feed(&layout, event)),
        );
        Ok(batches)
    }
}

impl Backend for EvdevBackend {
    fn devices(&self) -> &[DeviceInfo] {
        &self.infos
    }

    /// Counts `/dev/input/event*` nodes that could not be opened for reading
    /// (permission denied) and that udev tags as joysticks: the node's
    /// `/run/udev/data/c13:<minor>` file must exist and contain the line
    /// `E:ID_INPUT_JOYSTICK=1`. A missing udev file means the node is not counted.
    fn unreadable(&self) -> usize {
        self.unreadable
    }

    fn read(&mut self) -> Vec<(DeviceKey, Result<Vec<Raw>, ()>)> {
        let mut out = Vec::new();
        for (key, opened) in self.opened.iter_mut().enumerate() {
            match opened.drain() {
                Ok(batches) => out.extend(batches.into_iter().map(|b| (key, Ok(b)))),
                Err(()) => out.push((key, Err(()))),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minor_of_event_node() {
        assert_eq!(minor(0xd_43), 0x43);
        assert_eq!(minor((13 << 8) | 67), 67);
        assert_eq!(minor(0x100d00), 256);
    }

    #[test]
    fn joystick_tag_must_be_its_own_line() {
        assert!(has_joystick_tag(
            "S:input/x\nE:ID_INPUT_JOYSTICK=1\nG:seat\n"
        ));
        assert!(!has_joystick_tag("E:ID_INPUT_JOYSTICK=0\n"));
        assert!(!has_joystick_tag("E:ID_INPUT_KEYBOARD=1\n"));
        assert!(!has_joystick_tag(""));
    }
}
