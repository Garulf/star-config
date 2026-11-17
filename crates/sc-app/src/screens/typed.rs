//! The control a catalogue setting gets from its type, shared by the Config and Comms screens.

use crate::theme::{self, layout, space};
use crate::widgets::{muted, pick_list, slider, text_field, toggle};
use iced::widget::{container, row};
use iced::{Alignment, Element, Length};
use std::borrow::Cow;
use std::fmt;

pub enum Kind<'a> {
    Bool,
    Range { min: f64, max: f64, step: f64 },
    Number,
    Choice(Cow<'a, [(String, String)]>),
    Text,
}

#[derive(Debug, Clone, PartialEq)]
struct Choice {
    value: String,
    label: String,
}

impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// The control for one setting. `value: None` means the file does not have it: the control
/// shows `default` in muted colours, and touching it sets the value. `on_set: None` shows the
/// value read-only.
pub fn control<'a, M, F>(
    kind: Kind<'a>,
    value: Option<&'a str>,
    default: Option<&'a str>,
    on_set: Option<F>,
) -> Element<'a, M>
where
    M: Clone + 'a,
    F: Fn(String) -> M + Clone + 'a,
{
    match kind {
        Kind::Bool => switch(value, default, on_set),
        Kind::Range { min, max, step } => match on_set {
            Some(f) => range(value, default, (min, max, step), f),
            None => read_only(value.or(default), layout::NUMBER_WIDTH),
        },
        Kind::Number => field(value, default, layout::NUMBER_WIDTH, on_set),
        Kind::Choice(options) => choice(&options, value, default, on_set),
        Kind::Text => field(value, default, layout::CONTROL_WIDTH, on_set),
    }
}

fn switch<'a, M, F>(
    value: Option<&'a str>,
    default: Option<&'a str>,
    on_set: Option<F>,
) -> Element<'a, M>
where
    M: Clone + 'a,
    F: Fn(String) -> M + Clone + 'a,
{
    let on = value.or(default).is_some_and(|shown| shown.trim() == "1");
    let style = if value.is_none() {
        theme::toggle_unset
    } else {
        theme::toggle
    };
    match on_set {
        Some(f) => toggle(on, move |on| f(bool_value(on))).style(style).into(),
        None => iced::widget::toggler(on)
            .size(layout::TOGGLER_SIZE)
            .style(theme::toggle)
            .into(),
    }
}

fn bool_value(on: bool) -> String {
    if on { "1" } else { "0" }.to_string()
}

fn range<'a, M, F>(
    value: Option<&'a str>,
    default: Option<&'a str>,
    (min, max, step): (f64, f64, f64),
    on_set: F,
) -> Element<'a, M>
where
    M: Clone + 'a,
    F: Fn(String) -> M + Clone + 'a,
{
    let position = value
        .or(default)
        .and_then(|shown| shown.trim().parse::<f64>().ok())
        .unwrap_or(min)
        .clamp(min, max);
    let style = if value.is_none() {
        theme::range_unset
    } else {
        theme::range
    };
    let slide = on_set.clone();
    row![
        slider(min..=max, position, move |x| slide(format_step(x, step)))
            .step(step)
            .style(style),
        field(value, default, layout::NUMBER_WIDTH, Some(on_set)),
    ]
    .spacing(space::MD)
    .align_y(Alignment::Center)
    .into()
}

/// A text field whose placeholder is the default, so an unset value shows the default muted.
fn field<'a, M, F>(
    value: Option<&'a str>,
    default: Option<&'a str>,
    width: f32,
    on_set: Option<F>,
) -> Element<'a, M>
where
    M: Clone + 'a,
    F: Fn(String) -> M + Clone + 'a,
{
    let Some(f) = on_set else {
        return read_only(value.or(default), width);
    };
    let input = text_field(default.unwrap_or_default(), value.unwrap_or_default(), f).width(width);
    if width == layout::NUMBER_WIDTH {
        input.align_x(Alignment::End).into()
    } else {
        input.into()
    }
}

fn read_only<'a, M: 'a>(shown: Option<&'a str>, width: f32) -> Element<'a, M> {
    container(muted(shown.unwrap_or_default()))
        .width(width)
        .align_x(Alignment::End)
        .clip(true)
        .into()
}

fn choice<'a, M, F>(
    options: &[(String, String)],
    value: Option<&'a str>,
    default: Option<&'a str>,
    on_set: Option<F>,
) -> Element<'a, M>
where
    M: Clone + 'a,
    F: Fn(String) -> M + Clone + 'a,
{
    let items: Vec<Choice> = options
        .iter()
        .map(|(value, label)| Choice {
            value: value.clone(),
            label: label.clone(),
        })
        .collect();
    let label_of = |wanted: Option<&str>| {
        wanted.and_then(|wanted| items.iter().find(|item| item.value == wanted).cloned())
    };
    let selected = label_of(value);
    let placeholder = label_of(default).map_or_else(String::new, |item| item.label);
    let Some(f) = on_set else {
        let shown = selected.map_or(placeholder, |item| item.label);
        return container(muted(shown))
            .width(Length::Fixed(layout::CONTROL_WIDTH))
            .into();
    };
    pick_list(items, selected, move |item: Choice| f(item.value))
        .placeholder(placeholder)
        .width(layout::CONTROL_WIDTH)
        .into()
}

/// A slider position written with as many decimals as its step has.
pub fn format_step(value: f64, step: f64) -> String {
    let decimals = step
        .to_string()
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len().min(6));
    format!("{value:.decimals$}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_step_uses_the_step_precision() {
        assert_eq!(format_step(0.25, 0.01), "0.25");
        assert_eq!(format_step(2.5, 0.5), "2.5");
        assert_eq!(format_step(3.0, 1.0), "3");
        assert_eq!(format_step(0.1 + 0.2, 0.01), "0.30");
    }
}
