use super::controls::button;
use super::typography::{display, hint, label};
use crate::theme::{self, ButtonKind, FONT_SEMIBOLD, Tone, layout, size, space};
use iced::widget::text::IntoFragment;
use iced::widget::{self, Space, column, container, progress_bar, row, text};
use iced::{Alignment, Element, Length, Padding};

const PILL_PADDING: Padding = Padding {
    top: space::XS / 2.0,
    right: space::SM,
    bottom: space::XS / 2.0,
    left: space::SM,
};

/// A small pill naming a state such as Current, Unmatched, Interrupted or Changed.
pub fn tag<'a, M: 'a>(content: impl IntoFragment<'a>, tone: Tone) -> Element<'a, M> {
    container(text(content).size(size::CAPTION).font(FONT_SEMIBOLD))
        .padding(PILL_PADDING)
        .style(theme::tag(tone))
        .into()
}

/// An amber count bubble, for pending edits.
pub fn badge<'a, M: 'a>(count: usize) -> Element<'a, M> {
    container(
        text(count.to_string())
            .size(size::CAPTION)
            .font(FONT_SEMIBOLD)
            .style(theme::on_accent),
    )
    .padding(PILL_PADDING)
    .style(theme::badge)
    .into()
}

/// A keyboard key, for showing bound inputs such as `LCtrl` or `F`.
pub fn key_cap<'a, M: 'a>(content: impl IntoFragment<'a>) -> Element<'a, M> {
    container(
        text(content)
            .size(size::CAPTION)
            .font(FONT_SEMIBOLD)
            .wrapping(text::Wrapping::None),
    )
    .padding(PILL_PADDING)
    .style(theme::key_cap)
    .into()
}

/// A rounded pill with a coloured dot, for live status.
pub fn status_pill<'a, M: 'a>(content: impl IntoFragment<'a>, tone: Tone) -> Element<'a, M> {
    container(
        row![
            container(Space::new())
                .width(layout::DOT)
                .height(layout::DOT)
                .style(theme::dot(tone)),
            text(content).size(size::BODY),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: space::XS,
        right: space::MD,
        bottom: space::XS,
        left: space::MD,
    })
    .style(theme::pill)
    .into()
}

/// A labelled progress bar. `fraction` is clamped to 0..=1.
pub fn progress<'a, M: 'a>(fraction: f32, caption: impl IntoFragment<'a>) -> Element<'a, M> {
    let fraction = fraction.clamp(0.0, 1.0);
    column![
        row![
            hint(caption).width(Length::Fill),
            hint(format!("{:.0}%", fraction * 100.0)),
        ],
        progress_bar(0.0..=1.0, fraction)
            .girth(layout::PROGRESS_HEIGHT)
            .style(theme::meter),
    ]
    .spacing(space::XS)
    .into()
}

/// A summary figure for the Overview screen.
pub fn stat_card<'a, M: Clone + 'a>(
    caption: impl IntoFragment<'a>,
    value: impl IntoFragment<'a>,
    note: Option<&'a str>,
    action: Option<(&'a str, M)>,
) -> Element<'a, M> {
    let mut body = column![hint(caption), display(value)].spacing(space::XS);
    if let Some(note) = note {
        body = body.push(hint(note));
    }
    if let Some((text, message)) = action {
        body = body.push(Space::new().height(space::XS));
        body = body.push(button(text, ButtonKind::Secondary, Some(message)));
    }
    container(body)
        .padding(space::LG)
        .width(Length::Fill)
        .style(theme::card)
        .into()
}

/// A list and detail layout: a fixed-width card on the left, a filling card on the right.
pub fn split<'a, M: 'a>(
    left: impl Into<Element<'a, M>>,
    left_width: f32,
    right: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    row![
        container(left)
            .width(left_width)
            .height(Length::Fill)
            .style(theme::card),
        container(right)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(space::LG)
            .style(theme::card),
    ]
    .spacing(space::LG)
    .height(Length::Fill)
    .into()
}

const CELL_PADDING: Padding = Padding {
    top: 0.0,
    right: space::LG,
    bottom: 0.0,
    left: space::LG,
};

/// Column titles for a dense list. Use the same widths in every [`table_row`].
pub fn table_header<'a, M: 'a>(columns: &[(&'a str, Length)]) -> Element<'a, M> {
    let cells = columns.iter().map(|(title, width)| {
        container(
            text(*title)
                .size(size::CAPTION)
                .font(FONT_SEMIBOLD)
                .style(theme::muted),
        )
        .width(*width)
        .into()
    });
    container(row(cells).spacing(space::MD).align_y(Alignment::Center))
        .padding(CELL_PADDING)
        .height(layout::TABLE_ROW_HEIGHT)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .style(theme::table_header)
        .into()
}

/// One aligned row of a dense list. Cells are `(content, width)`; rows are
/// [`layout::TABLE_ROW_HEIGHT`] tall so they fit a `virtual_list`.
pub fn table_row<'a, M: Clone + 'a>(
    cells: Vec<(Element<'a, M>, Length)>,
    selected: bool,
    on_press: Option<M>,
) -> Element<'a, M> {
    let cells = cells
        .into_iter()
        .map(|(content, width)| container(content).width(width).into());
    widget::button(
        row(cells)
            .spacing(space::MD)
            .align_y(Alignment::Center)
            .height(Length::Fill),
    )
    .padding(CELL_PADDING)
    .height(layout::TABLE_ROW_HEIGHT)
    .width(Length::Fill)
    .style(theme::row_button(selected))
    .on_press_maybe(on_press)
    .into()
}

/// Plain text for a table cell.
pub fn cell<'a, M: 'a>(content: impl IntoFragment<'a>) -> Element<'a, M> {
    label(content).wrapping(text::Wrapping::None).into()
}
