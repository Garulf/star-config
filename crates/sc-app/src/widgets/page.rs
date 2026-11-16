use super::typography::{muted, title};
use crate::theme::{self, layout, space};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::IntoFragment;
use iced::widget::{Row, column, container, row, scrollable};
use iced::{Alignment, Element, Length, Padding};

fn header<'a, M: 'a>(
    heading: impl IntoFragment<'a>,
    subtitle: Option<&'a str>,
    trailing: Vec<Element<'a, M>>,
    max_width: f32,
) -> Element<'a, M> {
    let mut titles = column![title(heading)].spacing(space::XS);
    if let Some(subtitle) = subtitle {
        titles = titles.push(muted(subtitle));
    }
    let controls = Row::with_children(trailing)
        .spacing(space::SM)
        .align_y(Alignment::Center);
    container(
        row![titles.width(Length::Fill), controls]
            .spacing(space::LG)
            .align_y(Alignment::Center),
    )
    .max_width(max_width)
    .into()
}

const HEADER_PADDING: Padding = Padding {
    top: space::XL,
    right: space::XL,
    bottom: space::LG,
    left: space::XL,
};

const BODY_PADDING: Padding = Padding {
    top: 0.0,
    right: space::XL,
    bottom: space::XL,
    left: space::XL,
};

/// The frame of every screen: title, optional subtitle, controls on the right, and a
/// scrolling body capped at [`layout::CONTENT_MAX_WIDTH`].
pub fn page<'a, M: 'a>(
    heading: impl IntoFragment<'a>,
    subtitle: Option<&'a str>,
    trailing: Vec<Element<'a, M>>,
    body: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    let body = container(container(body).max_width(layout::CONTENT_MAX_WIDTH))
        .padding(BODY_PADDING)
        .width(Length::Fill);
    column![
        container(header(
            heading,
            subtitle,
            trailing,
            layout::CONTENT_MAX_WIDTH
        ))
        .padding(HEADER_PADDING),
        scrollable(body)
            .direction(Direction::Vertical(
                Scrollbar::new()
                    .width(theme::SCROLLBAR_WIDTH)
                    .scroller_width(theme::SCROLLBAR_WIDTH)
                    .margin(space::XS / 2.0),
            ))
            .style(theme::scroll)
            .height(Length::Fill),
    ]
    .into()
}

/// Like [`page`], but the body fills the remaining height without scrolling. Use it for
/// screens built on `split` or `virtual_list`, which scroll on their own.
pub fn page_fixed<'a, M: 'a>(
    heading: impl IntoFragment<'a>,
    subtitle: Option<&'a str>,
    trailing: Vec<Element<'a, M>>,
    body: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    column![
        container(header(heading, subtitle, trailing, f32::INFINITY)).padding(HEADER_PADDING),
        container(body)
            .padding(BODY_PADDING)
            .width(Length::Fill)
            .height(Length::Fill),
    ]
    .into()
}
