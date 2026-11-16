mod attr_edit;
pub mod backups;
pub mod channels;
pub mod comms;
pub mod config;
mod edits;
pub mod keybinds;
pub mod maintenance;
pub mod overview;
pub mod screenshots;
pub mod settings;
pub mod starstrings;
mod typed;
mod units;

use crate::screen::Screen;
use crate::widgets::{empty_state, page_fixed};
use iced::Element;

pub(crate) fn placeholder<'a, M: Clone + 'a>(screen: Screen, subtitle: &'a str) -> Element<'a, M> {
    page_fixed(
        screen.label(),
        Some(subtitle),
        Vec::new(),
        empty_state(
            "Coming in a later milestone",
            "This screen is a placeholder for now.",
            None,
        ),
    )
}
