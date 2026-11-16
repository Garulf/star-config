use crate::theme::{self, FONT_SEMIBOLD, size};
use iced::widget::text::{IntoFragment, Text};

/// Page title. One per screen.
pub fn title<'a>(content: impl IntoFragment<'a>) -> Text<'a> {
    Text::new(content)
        .size(size::TITLE)
        .font(FONT_SEMIBOLD)
        .style(theme::title)
}

/// Group heading above a card, small and muted.
pub fn heading<'a>(content: impl IntoFragment<'a>) -> Text<'a> {
    Text::new(content)
        .size(size::CAPTION)
        .font(FONT_SEMIBOLD)
        .style(theme::muted)
}

/// Body text in the normal colour.
pub fn label<'a>(content: impl IntoFragment<'a>) -> Text<'a> {
    Text::new(content).size(size::BODY)
}

/// Body text with weight, for names and values that lead a row.
pub fn strong<'a>(content: impl IntoFragment<'a>) -> Text<'a> {
    Text::new(content).size(size::BODY).font(FONT_SEMIBOLD)
}

/// Body-size secondary text.
pub fn muted<'a>(content: impl IntoFragment<'a>) -> Text<'a> {
    Text::new(content).size(size::BODY).style(theme::muted)
}

/// Small muted text under a label.
pub fn hint<'a>(content: impl IntoFragment<'a>) -> Text<'a> {
    Text::new(content).size(size::CAPTION).style(theme::caption)
}

/// Large figure, for stat cards.
pub fn display<'a>(content: impl IntoFragment<'a>) -> Text<'a> {
    Text::new(content)
        .size(size::DISPLAY)
        .font(FONT_SEMIBOLD)
        .style(theme::title)
}
