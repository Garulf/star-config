#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod evdev_map;
#[cfg(target_os = "linux")]
mod linux;
mod listener;
mod session;

pub use listener::Listener;
pub use session::Session;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceClass {
    Stick,
    Gamepad,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceInfo {
    pub vendor: u16,
    pub product: u16,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heard {
    pub device: DeviceInfo,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Raw {
    Pressed(String),
    Axis {
        name: &'static str,
        value: i32,
        min: i32,
        max: i32,
    },
}

pub type DeviceKey = usize;

pub trait Backend {
    fn devices(&self) -> &[DeviceInfo];
    fn unreadable(&self) -> usize;
    fn read(&mut self) -> Vec<(DeviceKey, Result<Vec<Raw>, ()>)>;
}

impl Backend for Box<dyn Backend + Send> {
    fn devices(&self) -> &[DeviceInfo] {
        (**self).devices()
    }

    fn unreadable(&self) -> usize {
        (**self).unreadable()
    }

    fn read(&mut self) -> Vec<(DeviceKey, Result<Vec<Raw>, ()>)> {
        (**self).read()
    }
}

#[cfg(not(target_os = "linux"))]
struct NoDevices;

#[cfg(not(target_os = "linux"))]
impl Backend for NoDevices {
    fn devices(&self) -> &[DeviceInfo] {
        &[]
    }

    fn unreadable(&self) -> usize {
        0
    }

    fn read(&mut self) -> Vec<(DeviceKey, Result<Vec<Raw>, ()>)> {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
pub fn open(class: DeviceClass) -> Session<Box<dyn Backend + Send>> {
    Session::new(Box::new(linux::open(class)))
}

#[cfg(not(target_os = "linux"))]
pub fn open(_class: DeviceClass) -> Session<Box<dyn Backend + Send>> {
    Session::new(Box::new(NoDevices))
}
