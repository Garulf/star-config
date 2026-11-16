use super::change_marker::change_marker;
use super::typography::{hint, label};
use crate::theme::{layout, space};
use iced::widget::{Column, container, row};
use iced::{Alignment, Element, Length, Padding};

const ROW_PADDING: Padding = Padding {
    top: space::MD,
    right: space::LG,
    bottom: space::MD,
    left: space::XS,
};

fn labelled_row<'a, M: 'a>(
    labels: Column<'a, M>,
    changed: bool,
    control: Element<'a, M>,
) -> Element<'a, M> {
    container(
        row![
            container(change_marker(changed))
                .height(layout::ROW_MIN_HEIGHT - ROW_PADDING.top - ROW_PADDING.bottom)
                .align_y(Alignment::Center),
            row![labels.width(Length::Fill), control]
                .spacing(space::LG)
                .align_y(Alignment::Center),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center),
    )
    .padding(ROW_PADDING)
    .width(Length::Fill)
    .into()
}

/// One setting: label and optional hint on the left, the control on the right,
/// and the change marker when the value differs from what is on disk.
pub fn setting_row<'a, M: 'a>(
    label_text: &'a str,
    hint_text: Option<&'a str>,
    changed: bool,
    control: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    let mut labels = Column::new().push(label(label_text));
    if let Some(hint_text) = hint_text {
        labels = labels.push(hint(hint_text));
    }
    labelled_row(labels, changed, control.into())
}

/// Like [`setting_row`], and when `changed` it also names the value on disk.
pub fn field_row<'a, M: 'a>(
    label_text: &'a str,
    hint_text: Option<&'a str>,
    changed: bool,
    previous: Option<String>,
    control: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    let mut labels = Column::new().push(label(label_text));
    if let Some(hint_text) = hint_text {
        labels = labels.push(hint(hint_text));
    }
    if changed && let Some(previous) = previous {
        labels = labels.push(hint(format!("Was {previous}")));
    }
    labelled_row(labels, changed, control.into())
}
