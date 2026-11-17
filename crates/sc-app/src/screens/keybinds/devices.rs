//! Sticks and gamepads: which `jsN` a press belongs to, and the stream that reads them.

use super::Message;
use iced::futures::Stream;
use iced::futures::channel::mpsc::Sender;
use sc_core::actionmaps::UsbId;
use sc_input::{DeviceClass, DeviceInfo};
use std::time::Duration;

const POLL_EVERY: Duration = Duration::from_millis(10);

/// The stick instance a press from `device` binds to, and a note when the file cannot say
/// for sure. `picked` is the stick chosen in the panel's picker.
pub fn stick_for(ids: &[(u32, UsbId)], picked: u32, device: &DeviceInfo) -> (u32, Option<String>) {
    let id = UsbId {
        vendor: device.vendor,
        product: device.product,
    };
    let mut same: Vec<u32> = ids
        .iter()
        .filter(|(_, known)| *known == id)
        .map(|(instance, _)| *instance)
        .collect();
    same.sort_unstable();
    match same.as_slice() {
        [] => (
            picked,
            Some(format!(
                "{} is not in this file yet, so this used js{picked}, the stick picked. Pick another stick and press Set if that is wrong.",
                device.name
            )),
        ),
        [only] => (*only, None),
        [lowest, ..] => {
            let instance = if same.contains(&picked) {
                picked
            } else {
                *lowest
            };
            (
                instance,
                Some(format!(
                    "Sticks of the same model cannot be told apart, so this used js{instance}. Pick another stick and press Set if that is wrong."
                )),
            )
        }
    }
}

/// Reads sticks or gamepads until the subscription is dropped.
pub fn device_stream(class: &DeviceClass) -> impl Stream<Item = Message> + use<> {
    let class = *class;
    iced::stream::channel(1, async move |sender: Sender<Message>| {
        let _ = std::thread::Builder::new()
            .name("device-listener".into())
            .spawn(move || listen(class, sender));
    })
}

/// The Windows backends need opening, every read and closing on one thread, so the session
/// never leaves this one.
fn listen(class: DeviceClass, mut sender: Sender<Message>) {
    let mut session = sc_input::open(class);
    let found = Message::Devices {
        found: session.devices().to_vec(),
        unreadable: session.unreadable(),
    };
    if !deliver(&mut sender, found) {
        return;
    }
    while !sender.is_closed() {
        if let Some(heard) = session.poll()
            && !deliver(&mut sender, Message::Heard(heard))
        {
            return;
        }
        std::thread::sleep(POLL_EVERY);
    }
}

/// Sends `message`, waiting while the channel is full. False once nobody listens.
fn deliver(sender: &mut Sender<Message>, mut message: Message) -> bool {
    loop {
        match sender.try_send(message) {
            Ok(()) => return true,
            Err(err) if err.is_disconnected() => return false,
            Err(err) => {
                message = err.into_inner();
                std::thread::sleep(POLL_EVERY);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: UsbId = UsbId {
        vendor: 0x231D,
        product: 0x0200,
    };
    const RIGHT: UsbId = UsbId {
        vendor: 0x231D,
        product: 0x0201,
    };

    fn device(id: UsbId) -> DeviceInfo {
        DeviceInfo {
            vendor: id.vendor,
            product: id.product,
            name: "VKB".into(),
        }
    }

    #[test]
    fn one_match_binds_there_without_a_note() {
        let ids = [(1, LEFT), (2, RIGHT)];
        assert_eq!(stick_for(&ids, 1, &device(RIGHT)), (2, None));
    }

    #[test]
    fn identical_sticks_prefer_the_picked_one_then_the_lowest() {
        let ids = [(1, LEFT), (3, LEFT), (4, LEFT)];
        let (instance, note) = stick_for(&ids, 3, &device(LEFT));
        assert_eq!(instance, 3);
        assert!(note.unwrap().contains("cannot be told apart"));
        assert_eq!(stick_for(&ids, 2, &device(LEFT)).0, 1);
    }

    #[test]
    fn an_unknown_stick_uses_the_picked_one_and_names_it() {
        let (instance, note) = stick_for(&[(1, LEFT)], 2, &device(RIGHT));
        assert_eq!(instance, 2);
        let note = note.unwrap();
        assert!(note.contains("not in this file yet"), "{note}");
        assert!(note.contains("js2"), "{note}");
    }
}
