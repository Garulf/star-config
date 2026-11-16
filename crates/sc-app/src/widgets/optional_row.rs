use super::change_marker::change_marker;
use super::icon::icon_toned;
use super::typography::{hint, label};
use crate::icons;
use crate::theme::{self, Tone, layout, size, space};
use iced::widget::{Column, Space, button, container, row, text, tooltip};
use iced::{Alignment, Element, Length, Padding};

const ROW_PADDING: Padding = Padding {
    top: space::MD,
    right: space::LG,
    bottom: space::MD,
    left: space::XS,
};

const NOT_SET: &str = "Not set. The game uses its default.";
pub const REMOVE_HINT: &str = "Remove from the file so the game uses its default";

/// What a changed setting held on disk before the staged edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Was {
    Value(String),
    NotSet,
}

impl Was {
    pub fn text(&self) -> String {
        match self {
            Was::Value(value) if value.is_empty() => "Was empty".to_string(),
            Was::Value(value) => format!("Was {value}"),
            Was::NotSet => "Was not set".to_string(),
        }
    }
}

/// A setting that a file may or may not hold. Rows built from it line up with
/// [`super::setting_row`] rows in the same card.
pub struct OptionalRow<'a, M> {
    pub label: &'a str,
    /// The technical name, shown small beside the label.
    pub name: Option<&'a str>,
    pub hint: Option<&'a str>,
    /// False when the file does not have the setting, so the game uses its default.
    pub set: bool,
    /// `Some` when the staged value differs from the file.
    pub was: Option<Was>,
    /// The message that stages the removal. Shown as a quiet icon button when the setting is set.
    pub remove: Option<M>,
}

/// One setting with its control, a `Not set` caption when the file lacks it, the change
/// marker with the earlier value, and a quiet remove button that sends the game back to its default.
pub fn optional_row<'a, M: Clone + 'a>(
    row_data: OptionalRow<'a, M>,
    control: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    let changed = row_data.was.is_some();
    let labels = labels(&row_data).width(Length::Fill);
    let action: Element<'a, M> = match row_data.remove.filter(|_| row_data.set) {
        Some(remove) => remove_button(remove),
        None => Space::new().into(),
    };
    container(
        row![
            container(change_marker(changed))
                .height(layout::ROW_MIN_HEIGHT - ROW_PADDING.top - ROW_PADDING.bottom)
                .align_y(Alignment::Center),
            row![
                labels,
                control.into(),
                container(action)
                    .width(layout::ROW_ACTION_WIDTH)
                    .align_x(Alignment::End),
            ]
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

fn remove_button<'a, M: Clone + 'a>(remove: M) -> Element<'a, M> {
    tooltip(
        button(icon_toned(icons::UNDO, layout::ICON, Tone::Muted))
            .padding(space::SM)
            .style(theme::ghost)
            .on_press(remove),
        container(hint(REMOVE_HINT))
            .padding(space::SM)
            .style(theme::toast),
        tooltip::Position::Left,
    )
    .into()
}

fn labels<'a, M: 'a>(row_data: &OptionalRow<'a, M>) -> Column<'a, M> {
    let mut title = row![label(row_data.label)]
        .spacing(space::SM)
        .align_y(Alignment::Center);
    if let Some(name) = row_data.name.filter(|name| *name != row_data.label) {
        title = title.push(
            text(name)
                .size(size::CAPTION)
                .font(theme::FONT_MONO)
                .style(theme::caption),
        );
    }
    let mut labels = Column::new().push(title).spacing(space::XS / 2.0);
    if let Some(hint_text) = row_data.hint {
        labels = labels.push(hint(hint_text));
    }
    if !row_data.set {
        labels = labels.push(hint(NOT_SET));
    }
    if let Some(was) = &row_data.was {
        labels = labels.push(hint(was.text()));
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn was_names_the_earlier_value() {
        assert_eq!(Was::Value("90".into()).text(), "Was 90");
        assert_eq!(Was::Value(String::new()).text(), "Was empty");
        assert_eq!(Was::NotSet.text(), "Was not set");
    }
}
