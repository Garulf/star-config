use super::controls::button;
use super::typography::label;
use crate::theme::{self, ButtonKind, FONT_SEMIBOLD, layout, size, space};
use iced::widget::{Row, Space, column, container, opaque, text};
use iced::{Element, Length};

/// A modal dialog over a dimmed backdrop. Put it on top of the window in a `stack`.
/// Actions are listed primary first and drawn right to left, so the primary one sits rightmost.
pub fn confirm<'a, M: Clone + 'a>(
    title: &'a str,
    body: &'a str,
    actions: Vec<(&'a str, ButtonKind, M)>,
) -> Element<'a, M> {
    let mut buttons = Row::new()
        .spacing(space::SM)
        .push(Space::new().width(Length::Fill));
    for (text_label, kind, message) in actions.into_iter().rev() {
        buttons = buttons.push(button(text_label, kind, Some(message)));
    }
    let card = container(
        column![
            text(title).size(size::LABEL).font(FONT_SEMIBOLD),
            label(body),
            buttons,
        ]
        .spacing(space::LG),
    )
    .padding(space::XL)
    .width(layout::DIALOG_WIDTH)
    .style(theme::dialog);
    opaque(
        container(card)
            .center(Length::Fill)
            .padding(space::XL)
            .style(theme::backdrop),
    )
}
