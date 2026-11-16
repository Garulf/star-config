use super::controls::button;
use super::icon::icon_toned;
use super::typography::muted;
use crate::icons;
use crate::theme::{self, ButtonKind, FONT_SEMIBOLD, Tone, layout, size, space};
use iced::widget::{column, container, text};
use iced::{Alignment, Element, Length};

/// A centred message for a screen with nothing to show, with an optional primary action.
pub fn empty_state<'a, M: Clone + 'a>(
    title: &'a str,
    body: &'a str,
    action: Option<(&'a str, M)>,
) -> Element<'a, M> {
    empty_state_icon(icons::INFO, title, body, action)
}

/// [`empty_state`] with a chosen icon in the circle.
pub fn empty_state_icon<'a, M: Clone + 'a>(
    svg: &'static [u8],
    title: &'a str,
    body: &'a str,
    action: Option<(&'a str, M)>,
) -> Element<'a, M> {
    let glyph = container(icon_toned(svg, layout::ICON_EMPTY, Tone::Muted))
        .center(layout::ICON_EMPTY_FRAME)
        .style(theme::icon_frame);
    let mut content = column![
        glyph,
        column![
            text(title)
                .size(size::LABEL)
                .font(FONT_SEMIBOLD)
                .align_x(Alignment::Center),
            muted(body).align_x(Alignment::Center),
        ]
        .spacing(space::XS)
        .align_x(Alignment::Center),
    ]
    .spacing(space::LG)
    .align_x(Alignment::Center)
    .max_width(layout::DIALOG_WIDTH);
    if let Some((label, message)) = action {
        content = content.push(button(label, ButtonKind::Primary, Some(message)));
    }
    container(content)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding(space::XXL)
        .into()
}
