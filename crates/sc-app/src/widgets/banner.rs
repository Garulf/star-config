use super::controls::button;
use super::icon::icon_toned;
use crate::icons;
use crate::screen::ToastKind;
use crate::theme::{self, ButtonKind, Tone, layout, space};
use iced::widget::{container, row, text};
use iced::{Alignment, Element, Length, Padding};

pub(crate) fn tone_of(kind: ToastKind) -> Tone {
    match kind {
        ToastKind::Info => Tone::Muted,
        ToastKind::Success => Tone::Success,
        ToastKind::Warning => Tone::Warning,
        ToastKind::Error => Tone::Danger,
    }
}

pub(crate) fn icon_of(kind: ToastKind) -> &'static [u8] {
    match kind {
        ToastKind::Info => icons::INFO,
        ToastKind::Success => icons::CHECK,
        ToastKind::Warning | ToastKind::Error => icons::WARNING,
    }
}

/// A full-width notice above the page content, with an optional action button.
pub fn banner<'a, M: Clone + 'a>(
    kind: ToastKind,
    message: &'a str,
    action: Option<(&'a str, M)>,
) -> Element<'a, M> {
    let mut content = row![
        icon_toned(icon_of(kind), layout::ICON, tone_of(kind)),
        text(message).size(theme::size::BODY).width(Length::Fill),
    ]
    .spacing(space::MD)
    .align_y(Alignment::Center);
    if let Some((label, message)) = action {
        content = content.push(button(label, ButtonKind::Secondary, Some(message)));
    }
    container(content)
        .padding(Padding {
            top: space::SM,
            right: space::SM,
            bottom: space::SM,
            left: space::LG,
        })
        .width(Length::Fill)
        .style(theme::banner(tone_of(kind)))
        .into()
}
