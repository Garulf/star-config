use crate::theme::{self, layout};
use iced::Element;
use iced::widget::{Space, container};

/// The amber bar beside an edited row. It keeps its space when unchanged so rows stay aligned.
pub fn change_marker<'a, M: 'a>(changed: bool) -> Element<'a, M> {
    container(Space::new())
        .width(layout::MARKER_WIDTH)
        .height(layout::MARKER_HEIGHT)
        .style(theme::marker(changed))
        .into()
}
