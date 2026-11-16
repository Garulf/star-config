use super::typography::heading;
use crate::theme::{self, space};
use iced::widget::{Column, column, container, rule};
use iced::{Element, Length, Padding};

/// A titled group: a small muted heading above a bordered card holding `body`.
pub fn section<'a, M: 'a>(title: &'a str, body: impl Into<Element<'a, M>>) -> Element<'a, M> {
    column![
        container(heading(title.to_uppercase())).padding(Padding {
            left: space::XS,
            ..Padding::ZERO
        }),
        container(body)
            .width(Length::Fill)
            .style(theme::card)
            .clip(true),
    ]
    .spacing(space::SM)
    .into()
}

/// Stacks rows with a one-pixel divider between each pair, for the body of a [`section`].
pub fn rows<'a, M: 'a>(items: impl IntoIterator<Item = Element<'a, M>>) -> Element<'a, M> {
    let mut list = Column::new();
    for (index, item) in items.into_iter().enumerate() {
        if index > 0 {
            list = list.push(divider());
        }
        list = list.push(item);
    }
    list.into()
}

/// A full-width hairline in the border colour.
pub fn divider<'a, M: 'a>() -> Element<'a, M> {
    rule::horizontal(theme::DIVIDER)
        .style(theme::divider)
        .into()
}
