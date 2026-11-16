use crate::theme::{self, Tone};
use iced::Element;
use iced::widget::svg;

/// An embedded SVG icon tinted with the text colour.
pub fn icon<'a, M: 'a>(svg_bytes: &'static [u8], size: f32) -> Element<'a, M> {
    icon_toned(svg_bytes, size, Tone::Neutral)
}

/// An embedded SVG icon tinted with a semantic tone.
pub fn icon_toned<'a, M: 'a>(svg_bytes: &'static [u8], size: f32, tone: Tone) -> Element<'a, M> {
    svg(svg::Handle::from_memory(svg_bytes))
        .width(size)
        .height(size)
        .style(theme::icon_tint(tone))
        .into()
}
