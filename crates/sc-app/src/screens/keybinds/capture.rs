//! The game's names for keyboard keys and mouse buttons, and the capture of one input.
//!
//! The names follow the community documentation of actionmaps.xml and have not been
//! checked against a real file, so each lives on one line of a table below.

use iced::keyboard::key::{Code, Physical};
use iced::mouse::{self, Button, ScrollDelta};
use iced::{Event, keyboard};

/// `(physical key, game name, label on a key cap)`.
const KEYS: &[(Code, &str, &str)] = &[
    (Code::KeyA, "a", "A"),
    (Code::KeyB, "b", "B"),
    (Code::KeyC, "c", "C"),
    (Code::KeyD, "d", "D"),
    (Code::KeyE, "e", "E"),
    (Code::KeyF, "f", "F"),
    (Code::KeyG, "g", "G"),
    (Code::KeyH, "h", "H"),
    (Code::KeyI, "i", "I"),
    (Code::KeyJ, "j", "J"),
    (Code::KeyK, "k", "K"),
    (Code::KeyL, "l", "L"),
    (Code::KeyM, "m", "M"),
    (Code::KeyN, "n", "N"),
    (Code::KeyO, "o", "O"),
    (Code::KeyP, "p", "P"),
    (Code::KeyQ, "q", "Q"),
    (Code::KeyR, "r", "R"),
    (Code::KeyS, "s", "S"),
    (Code::KeyT, "t", "T"),
    (Code::KeyU, "u", "U"),
    (Code::KeyV, "v", "V"),
    (Code::KeyW, "w", "W"),
    (Code::KeyX, "x", "X"),
    (Code::KeyY, "y", "Y"),
    (Code::KeyZ, "z", "Z"),
    (Code::Digit0, "0", "0"),
    (Code::Digit1, "1", "1"),
    (Code::Digit2, "2", "2"),
    (Code::Digit3, "3", "3"),
    (Code::Digit4, "4", "4"),
    (Code::Digit5, "5", "5"),
    (Code::Digit6, "6", "6"),
    (Code::Digit7, "7", "7"),
    (Code::Digit8, "8", "8"),
    (Code::Digit9, "9", "9"),
    (Code::F1, "f1", "F1"),
    (Code::F2, "f2", "F2"),
    (Code::F3, "f3", "F3"),
    (Code::F4, "f4", "F4"),
    (Code::F5, "f5", "F5"),
    (Code::F6, "f6", "F6"),
    (Code::F7, "f7", "F7"),
    (Code::F8, "f8", "F8"),
    (Code::F9, "f9", "F9"),
    (Code::F10, "f10", "F10"),
    (Code::F11, "f11", "F11"),
    (Code::F12, "f12", "F12"),
    (Code::F13, "f13", "F13"),
    (Code::F14, "f14", "F14"),
    (Code::F15, "f15", "F15"),
    (Code::ArrowUp, "up", "Up"),
    (Code::ArrowDown, "down", "Down"),
    (Code::ArrowLeft, "left", "Left"),
    (Code::ArrowRight, "right", "Right"),
    (Code::Space, "space", "Space"),
    (Code::Tab, "tab", "Tab"),
    (Code::Enter, "enter", "Enter"),
    (Code::Backspace, "backspace", "Backspace"),
    (Code::Insert, "insert", "Insert"),
    (Code::Delete, "delete", "Delete"),
    (Code::Home, "home", "Home"),
    (Code::End, "end", "End"),
    (Code::PageUp, "pgup", "Page Up"),
    (Code::PageDown, "pgdn", "Page Down"),
    (Code::Numpad0, "np_0", "Num 0"),
    (Code::Numpad1, "np_1", "Num 1"),
    (Code::Numpad2, "np_2", "Num 2"),
    (Code::Numpad3, "np_3", "Num 3"),
    (Code::Numpad4, "np_4", "Num 4"),
    (Code::Numpad5, "np_5", "Num 5"),
    (Code::Numpad6, "np_6", "Num 6"),
    (Code::Numpad7, "np_7", "Num 7"),
    (Code::Numpad8, "np_8", "Num 8"),
    (Code::Numpad9, "np_9", "Num 9"),
    (Code::NumpadAdd, "np_add", "Num +"),
    (Code::NumpadSubtract, "np_subtract", "Num -"),
    (Code::NumpadMultiply, "np_multiply", "Num *"),
    (Code::NumpadDivide, "np_divide", "Num /"),
    (Code::NumpadDecimal, "np_period", "Num ."),
    (Code::NumpadEnter, "np_enter", "Num Enter"),
    (Code::Minus, "minus", "-"),
    (Code::Equal, "equals", "="),
    (Code::BracketLeft, "lbracket", "["),
    (Code::BracketRight, "rbracket", "]"),
    (Code::Semicolon, "semicolon", ";"),
    (Code::Quote, "apostrophe", "'"),
    (Code::Comma, "comma", ","),
    (Code::Period, "period", "."),
    (Code::Slash, "slash", "/"),
    (Code::Backslash, "backslash", "\\"),
    (Code::Backquote, "tilde", "~"),
];

const MODIFIERS: &[(Code, &str, &str)] = &[
    (Code::ShiftLeft, "lshift", "L Shift"),
    (Code::ShiftRight, "rshift", "R Shift"),
    (Code::ControlLeft, "lctrl", "L Ctrl"),
    (Code::ControlRight, "rctrl", "R Ctrl"),
    (Code::AltLeft, "lalt", "L Alt"),
    (Code::AltRight, "ralt", "R Alt"),
];

const MOUSE: &[(Button, &str, &str)] = &[
    (Button::Left, "mouse1", "Left click"),
    (Button::Right, "mouse2", "Right click"),
    (Button::Middle, "mouse3", "Middle click"),
    (Button::Back, "mouse4", "Mouse 4"),
    (Button::Forward, "mouse5", "Mouse 5"),
];

pub const WHEEL_UP: &str = "mwheel_up";
pub const WHEEL_DOWN: &str = "mwheel_down";

/// Mouse inputs that no button press produces: the wheel and the axes.
const MOUSE_OTHER: &[(&str, &str)] = &[
    (WHEEL_UP, "Wheel up"),
    (WHEEL_DOWN, "Wheel down"),
    ("maxis_x", "Mouse X"),
    ("maxis_y", "Mouse Y"),
];

fn code(physical: &Physical) -> Option<Code> {
    match physical {
        Physical::Code(code) => Some(*code),
        Physical::Unidentified(_) => None,
    }
}

fn find(table: &[(Code, &'static str, &'static str)], physical: &Physical) -> Option<&'static str> {
    let code = code(physical)?;
    table
        .iter()
        .find(|(known, _, _)| *known == code)
        .map(|(_, name, _)| *name)
}

/// The game's name for a key other than a modifier.
pub fn key_name(physical: &Physical) -> Option<&'static str> {
    find(KEYS, physical)
}

pub fn modifier_name(physical: &Physical) -> Option<&'static str> {
    find(MODIFIERS, physical)
}

pub fn mouse_name(button: Button) -> Option<&'static str> {
    MOUSE
        .iter()
        .find(|(known, _, _)| *known == button)
        .map(|(_, name, _)| *name)
}

pub fn wheel_name(delta: ScrollDelta) -> Option<&'static str> {
    let y = match delta {
        ScrollDelta::Lines { y, .. } | ScrollDelta::Pixels { y, .. } => y,
    };
    if y > 0.0 {
        Some(WHEEL_UP)
    } else if y < 0.0 {
        Some(WHEEL_DOWN)
    } else {
        None
    }
}

pub fn is_modifier(name: &str) -> bool {
    MODIFIERS.iter().any(|(_, known, _)| *known == name)
}

pub fn is_key(name: &str) -> bool {
    KEYS.iter().any(|(_, known, _)| *known == name)
}

pub fn is_mouse(name: &str) -> bool {
    MOUSE.iter().any(|(_, known, _)| *known == name)
        || MOUSE_OTHER.iter().any(|(known, _)| *known == name)
}

/// The key cap label for a keyboard or mouse name.
pub fn label_of(name: &str) -> Option<&'static str> {
    KEYS.iter()
        .chain(MODIFIERS)
        .find(|(_, known, _)| *known == name)
        .map(|(_, _, label)| *label)
        .or_else(|| {
            MOUSE
                .iter()
                .find(|(_, known, _)| *known == name)
                .map(|(_, _, label)| *label)
        })
        .or_else(|| {
            MOUSE_OTHER
                .iter()
                .find(|(known, _)| *known == name)
                .map(|(_, label)| *label)
        })
}

/// An Escape press, which stops any listening cell.
pub fn stops_listening(event: &Event) -> bool {
    matches!(
        event,
        Event::Keyboard(keyboard::Event::KeyPressed { physical_key, .. })
            if code(physical_key) == Some(Code::Escape)
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Waiting,
    /// The input as the game writes it after the device prefix, such as `lalt+f`.
    Done(String),
    Cancelled,
    Unknown,
}

/// Turns raw key and mouse events into one input. Modifiers held when another key or a
/// mouse button goes down become a combination; a modifier pressed and released alone
/// is an input by itself.
#[derive(Debug, Clone, Default)]
pub struct Capture {
    held: Vec<&'static str>,
}

impl Capture {
    pub fn event(&mut self, event: &Event, mouse_allowed: bool) -> Outcome {
        match event {
            Event::Keyboard(keyboard::Event::KeyPressed {
                physical_key,
                repeat,
                ..
            }) => self.pressed(physical_key, *repeat),
            Event::Keyboard(keyboard::Event::KeyReleased { physical_key, .. }) => {
                self.released(physical_key)
            }
            Event::Mouse(mouse::Event::ButtonPressed(button)) if mouse_allowed => {
                match mouse_name(*button) {
                    Some(name) => Outcome::Done(self.combine(name)),
                    None => Outcome::Unknown,
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if mouse_allowed => {
                match wheel_name(*delta) {
                    Some(name) => Outcome::Done(self.combine(name)),
                    None => Outcome::Waiting,
                }
            }
            _ => Outcome::Waiting,
        }
    }

    fn pressed(&mut self, physical: &Physical, repeat: bool) -> Outcome {
        if code(physical) == Some(Code::Escape) {
            self.held.clear();
            return Outcome::Cancelled;
        }
        if repeat {
            return Outcome::Waiting;
        }
        if let Some(modifier) = modifier_name(physical) {
            if !self.held.contains(&modifier) {
                self.held.push(modifier);
            }
            return Outcome::Waiting;
        }
        match key_name(physical) {
            Some(name) => Outcome::Done(self.combine(name)),
            None => Outcome::Unknown,
        }
    }

    fn released(&mut self, physical: &Physical) -> Outcome {
        let Some(modifier) = modifier_name(physical) else {
            return Outcome::Waiting;
        };
        if !self.held.contains(&modifier) {
            return Outcome::Waiting;
        }
        let input = self.held.join("+");
        self.held.clear();
        Outcome::Done(input)
    }

    fn combine(&mut self, name: &str) -> String {
        let mut parts: Vec<&str> = std::mem::take(&mut self.held);
        parts.push(name);
        parts.join("+")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::{Key, Location, Modifiers};

    fn press(code: Code) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Unidentified,
            modified_key: Key::Unidentified,
            physical_key: Physical::Code(code),
            location: Location::Standard,
            modifiers: Modifiers::empty(),
            text: None,
            repeat: false,
        })
    }

    fn release(code: Code) -> Event {
        Event::Keyboard(keyboard::Event::KeyReleased {
            key: Key::Unidentified,
            modified_key: Key::Unidentified,
            physical_key: Physical::Code(code),
            location: Location::Standard,
            modifiers: Modifiers::empty(),
        })
    }

    fn name(code: Code) -> Option<&'static str> {
        key_name(&Physical::Code(code))
    }

    #[test]
    fn letters() {
        assert_eq!(name(Code::KeyA), Some("a"));
        assert_eq!(name(Code::KeyZ), Some("z"));
        assert_eq!(name(Code::Digit0), Some("0"));
        assert_eq!(label_of("w"), Some("W"));
    }

    #[test]
    fn f_keys() {
        assert_eq!(name(Code::F1), Some("f1"));
        assert_eq!(name(Code::F12), Some("f12"));
        assert_eq!(name(Code::F15), Some("f15"));
        assert_eq!(label_of("f15"), Some("F15"));
    }

    #[test]
    fn numpad() {
        assert_eq!(name(Code::Numpad0), Some("np_0"));
        assert_eq!(name(Code::Numpad9), Some("np_9"));
        assert_eq!(name(Code::NumpadAdd), Some("np_add"));
        assert_eq!(name(Code::NumpadSubtract), Some("np_subtract"));
        assert_eq!(name(Code::NumpadMultiply), Some("np_multiply"));
        assert_eq!(name(Code::NumpadDivide), Some("np_divide"));
        assert_eq!(name(Code::NumpadDecimal), Some("np_period"));
        assert_eq!(name(Code::NumpadEnter), Some("np_enter"));
    }

    #[test]
    fn modifiers_by_side() {
        let modifier = |code| modifier_name(&Physical::Code(code));
        assert_eq!(modifier(Code::ShiftLeft), Some("lshift"));
        assert_eq!(modifier(Code::ShiftRight), Some("rshift"));
        assert_eq!(modifier(Code::ControlLeft), Some("lctrl"));
        assert_eq!(modifier(Code::ControlRight), Some("rctrl"));
        assert_eq!(modifier(Code::AltLeft), Some("lalt"));
        assert_eq!(modifier(Code::AltRight), Some("ralt"));
        assert_eq!(name(Code::AltLeft), None);

        let mut capture = Capture::default();
        assert_eq!(
            capture.event(&press(Code::AltRight), false),
            Outcome::Waiting
        );
        assert_eq!(
            capture.event(&press(Code::KeyF), false),
            Outcome::Done("ralt+f".into())
        );
        assert_eq!(
            capture.event(&press(Code::ShiftLeft), false),
            Outcome::Waiting
        );
        assert_eq!(
            capture.event(&release(Code::ShiftLeft), false),
            Outcome::Done("lshift".into())
        );
    }

    #[test]
    fn mouse_buttons() {
        assert_eq!(mouse_name(Button::Left), Some("mouse1"));
        assert_eq!(mouse_name(Button::Right), Some("mouse2"));
        assert_eq!(mouse_name(Button::Middle), Some("mouse3"));
        assert_eq!(mouse_name(Button::Back), Some("mouse4"));
        assert_eq!(mouse_name(Button::Forward), Some("mouse5"));
        assert_eq!(
            wheel_name(ScrollDelta::Lines { x: 0.0, y: 1.0 }),
            Some(WHEEL_UP)
        );
        assert_eq!(
            wheel_name(ScrollDelta::Pixels { x: 0.0, y: -3.0 }),
            Some(WHEEL_DOWN)
        );

        let click = Event::Mouse(mouse::Event::ButtonPressed(Button::Back));
        let mut capture = Capture::default();
        assert_eq!(capture.event(&click, false), Outcome::Waiting);
        assert_eq!(
            capture.event(&press(Code::ControlLeft), true),
            Outcome::Waiting
        );
        assert_eq!(
            capture.event(&click, true),
            Outcome::Done("lctrl+mouse4".into())
        );
    }

    #[test]
    fn unknown_key_is_none() {
        assert_eq!(name(Code::CapsLock), None);
        assert_eq!(
            key_name(&Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified
            )),
            None
        );
        assert_eq!(mouse_name(Button::Other(9)), None);
        let mut capture = Capture::default();
        assert_eq!(
            capture.event(&press(Code::CapsLock), false),
            Outcome::Unknown
        );
        assert_eq!(
            capture.event(&press(Code::Escape), false),
            Outcome::Cancelled
        );
    }
}
