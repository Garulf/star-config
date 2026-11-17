use std::collections::HashSet;

use crate::{Backend, DeviceInfo, DeviceKey, Heard, Listener};

pub struct Session<B: Backend> {
    backend: B,
    listener: Listener,
    failed: HashSet<DeviceKey>,
}

impl<B: Backend> Session<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            listener: Listener::new(),
            failed: HashSet::new(),
        }
    }

    pub fn devices(&self) -> &[DeviceInfo] {
        self.backend.devices()
    }

    pub fn unreadable(&self) -> usize {
        self.backend.unreadable()
    }

    pub fn poll(&mut self) -> Option<Heard> {
        let mut heard = None;
        for (key, result) in self.backend.read() {
            if self.failed.contains(&key) {
                continue;
            }
            let Ok(batch) = result else {
                self.failed.insert(key);
                continue;
            };
            let input = self.listener.feed(key, &batch);
            if heard.is_none()
                && let Some(input) = input
                && let Some(device) = self.backend.devices().get(key)
            {
                heard = Some(Heard {
                    device: device.clone(),
                    input,
                });
            }
        }
        heard
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::Raw;

    type Step = Vec<(DeviceKey, Result<Vec<Raw>, ()>)>;

    struct FakeBackend {
        devices: Vec<DeviceInfo>,
        script: VecDeque<Step>,
    }

    impl Backend for FakeBackend {
        fn devices(&self) -> &[DeviceInfo] {
            &self.devices
        }

        fn unreadable(&self) -> usize {
            0
        }

        fn read(&mut self) -> Step {
            self.script.pop_front().unwrap_or_default()
        }
    }

    fn info(name: &str) -> DeviceInfo {
        DeviceInfo {
            vendor: 0x231D,
            product: 0x0201,
            name: name.into(),
        }
    }

    fn pressed(name: &str) -> Result<Vec<Raw>, ()> {
        Ok(vec![Raw::Pressed(name.into())])
    }

    #[test]
    fn poll_returns_heard_with_the_device() {
        let device = info("VKB R");
        let mut s = Session::new(FakeBackend {
            devices: vec![device.clone()],
            script: VecDeque::from([vec![(0, pressed("button7"))]]),
        });
        assert_eq!(
            s.poll(),
            Some(Heard {
                device,
                input: "button7".into()
            })
        );
    }

    #[test]
    fn a_device_that_fails_is_dropped() {
        let second = info("Second");
        let mut s = Session::new(FakeBackend {
            devices: vec![info("First"), second.clone()],
            script: VecDeque::from([
                vec![(0, Err(()))],
                vec![(0, pressed("button1")), (1, pressed("button2"))],
                vec![(0, pressed("button3"))],
            ]),
        });
        assert_eq!(s.poll(), None);
        assert_eq!(
            s.poll(),
            Some(Heard {
                device: second,
                input: "button2".into()
            })
        );
        assert_eq!(s.poll(), None);
    }
}
