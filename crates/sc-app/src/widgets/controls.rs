use super::icon::{icon, icon_toned};
use crate::theme::{self, ButtonKind, Tone, layout, size, space};
use iced::widget::text::IntoFragment;
use iced::widget::{
    self, Checkbox, PickList, Slider, TextInput, Toggler, container, row, stack, text,
};
use iced::{Alignment, Element, Length, Padding};
use std::borrow::Borrow;
use std::ops::RangeInclusive;

/// The id every `search_field` carries, so Ctrl+F can focus the one on screen.
pub const SEARCH_ID: &str = "screen-search";

const BUTTON_PADDING: Padding = Padding {
    top: space::SM,
    right: space::LG,
    bottom: space::SM,
    left: space::LG,
};

const FIELD_PADDING: Padding = Padding {
    top: space::SM,
    right: space::MD,
    bottom: space::SM,
    left: space::MD,
};

/// A text button. `on_press: None` renders it disabled.
pub fn button<'a, M: Clone + 'a>(
    label: impl IntoFragment<'a>,
    kind: ButtonKind,
    on_press: Option<M>,
) -> Element<'a, M> {
    widget::button(text(label).size(size::BODY))
        .padding(BUTTON_PADDING)
        .style(theme::button_style(kind))
        .on_press_maybe(on_press)
        .into()
}

/// A button with a leading icon.
pub fn icon_button<'a, M: Clone + 'a>(
    svg: &'static [u8],
    label: impl IntoFragment<'a>,
    kind: ButtonKind,
    on_press: Option<M>,
) -> Element<'a, M> {
    let tint = match (kind, on_press.is_some()) {
        (_, false) => Tone::Muted,
        (ButtonKind::Primary, true) => return primary_icon_button(svg, label, on_press),
        _ => Tone::Neutral,
    };
    widget::button(
        row![
            icon_toned(svg, layout::ICON, tint),
            text(label).size(size::BODY)
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center),
    )
    .padding(BUTTON_PADDING)
    .style(theme::button_style(kind))
    .on_press_maybe(on_press)
    .into()
}

fn primary_icon_button<'a, M: Clone + 'a>(
    svg: &'static [u8],
    label: impl IntoFragment<'a>,
    on_press: Option<M>,
) -> Element<'a, M> {
    let glyph = widget::svg(widget::svg::Handle::from_memory(svg))
        .width(layout::ICON)
        .height(layout::ICON)
        .style(theme::icon_on_accent);
    widget::button(
        row![glyph, text(label).size(size::BODY)]
            .spacing(space::SM)
            .align_y(Alignment::Center),
    )
    .padding(BUTTON_PADDING)
    .style(theme::primary)
    .on_press_maybe(on_press)
    .into()
}

/// A square ghost button holding only an icon, for close and similar actions.
pub fn icon_only<'a, M: Clone + 'a>(svg: &'static [u8], on_press: Option<M>) -> Element<'a, M> {
    widget::button(icon(svg, layout::ICON))
        .padding(space::SM)
        .style(theme::ghost)
        .on_press_maybe(on_press)
        .into()
}

/// A row of mutually exclusive choices. Each item is `(label, active, message)`.
pub fn segmented<'a, M: Clone + 'a>(
    items: Vec<(impl IntoFragment<'a>, bool, M)>,
) -> Element<'a, M> {
    let buttons = items.into_iter().map(|(label, active, message)| {
        widget::button(text(label).size(size::BODY))
            .padding(Padding {
                top: space::XS,
                right: space::MD,
                bottom: space::XS,
                left: space::MD,
            })
            .style(theme::segment(active))
            .on_press(message)
            .into()
    });
    container(row(buttons).spacing(space::XS))
        .padding(space::XS / 2.0)
        .style(theme::segments)
        .into()
}

/// A single-line text input in the kit style.
pub fn text_field<'a, M: Clone + 'a>(
    placeholder: &str,
    value: &str,
    on_input: impl Fn(String) -> M + 'a,
) -> TextInput<'a, M> {
    widget::text_input(placeholder, value)
        .on_input(on_input)
        .padding(FIELD_PADDING)
        .size(size::BODY)
        .style(theme::text_field)
}

/// A short text input for numbers, right-aligned controls in a row.
pub fn number_field<'a, M: Clone + 'a>(
    value: &str,
    on_input: impl Fn(String) -> M + 'a,
) -> TextInput<'a, M> {
    text_field("", value, on_input)
        .width(layout::NUMBER_WIDTH)
        .align_x(Alignment::End)
}

/// A text input with a search icon and the shared [`SEARCH_ID`].
pub fn search_field<'a, M: Clone + 'a>(
    value: &str,
    placeholder: &str,
    on_input: impl Fn(String) -> M + 'a,
) -> Element<'a, M> {
    let input = text_field(placeholder, value, on_input)
        .id(SEARCH_ID)
        .padding(Padding {
            left: space::MD + layout::ICON + space::SM,
            ..FIELD_PADDING
        });
    let glyph = container(icon_toned(crate::icons::SEARCH, layout::ICON, Tone::Muted))
        .padding(Padding {
            left: space::MD,
            ..Padding::ZERO
        })
        .height(Length::Fill)
        .align_y(Alignment::Center);
    stack![input, glyph].width(layout::SEARCH_WIDTH).into()
}

/// An on/off switch.
pub fn toggle<'a, M: 'a>(value: bool, on_toggle: impl Fn(bool) -> M + 'a) -> Toggler<'a, M> {
    widget::toggler(value)
        .on_toggle(on_toggle)
        .size(layout::TOGGLER_SIZE)
        .style(theme::toggle)
}

/// A checkbox with a trailing label.
pub fn checkbox<'a, M: 'a>(
    label: impl IntoFragment<'a>,
    checked: bool,
    on_toggle: impl Fn(bool) -> M + 'a,
) -> Checkbox<'a, M> {
    widget::checkbox(checked)
        .label(label)
        .on_toggle(on_toggle)
        .size(layout::CHECKBOX_SIZE)
        .spacing(space::SM)
        .text_size(size::BODY)
        .style(theme::check)
}

/// A horizontal slider of fixed width.
pub fn slider<'a, T, M>(
    range: RangeInclusive<T>,
    value: T,
    on_change: impl Fn(T) -> M + 'a,
) -> Slider<'a, T, M>
where
    T: Copy + From<u8> + PartialOrd,
    M: Clone + 'a,
    f64: From<T>,
{
    widget::slider(range, value, on_change)
        .width(layout::SLIDER_WIDTH)
        .style(theme::range)
}

/// A dropdown of options shown with their `Display` text.
pub fn pick_list<'a, T, L, V, M>(
    options: L,
    selected: Option<V>,
    on_select: impl Fn(T) -> M + 'a,
) -> PickList<'a, T, L, V, M>
where
    T: ToString + PartialEq + Clone + 'a,
    L: Borrow<[T]> + 'a,
    V: Borrow<T> + 'a,
    M: Clone + 'a,
{
    widget::pick_list(options, selected, on_select)
        .padding(FIELD_PADDING)
        .text_size(size::BODY)
        .style(theme::pick)
        .menu_style(theme::pick_menu)
}
