use super::data::badge;
use super::icon::icon_toned;
use crate::theme::{self, Tone, layout, size, space};
use iced::widget::{button, container, row, text};
use iced::{Alignment, Element, Length, Padding};

/// A sidebar entry. `pending > 0` adds a count badge.
pub fn nav_item<'a, M: Clone + 'a>(
    svg: &'static [u8],
    label: &'a str,
    active: bool,
    pending: usize,
    on_press: M,
) -> Element<'a, M> {
    let tone = if active { Tone::Accent } else { Tone::Muted };
    let mut content = row![
        container(icon_toned(svg, layout::ICON, tone))
            .height(layout::NAV_HEIGHT - 2.0 * space::XS)
            .align_y(Alignment::Center),
        text(label)
            .size(size::BODY)
            .wrapping(text::Wrapping::Word)
            .width(Length::Fill),
    ]
    .spacing(space::SM)
    .align_y(Alignment::Center);
    if pending > 0 {
        content = content.push(badge(pending));
    }
    button(content)
        .padding(Padding {
            top: space::XS,
            right: space::XS,
            bottom: space::XS,
            left: space::SM,
        })
        .width(Length::Fill)
        .style(theme::nav(active))
        .on_press(on_press)
        .into()
}
