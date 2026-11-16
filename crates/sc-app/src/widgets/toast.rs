use super::banner::{icon_of, tone_of};
use super::controls::{icon_button, icon_only};
use super::icon::icon_toned;
use crate::icons;
use crate::screen::Toast;
use crate::theme::{self, ButtonKind, layout, size, space};
use iced::widget::{Column, container, row, text};
use iced::{Alignment, Element, Padding};

/// The root keeps at most this many toasts; older ones are dropped first.
pub const MAX_TOASTS: usize = 3;

/// Toasts stacked oldest first. Place it at the bottom right of the content area.
pub fn toast_stack<'a, M: Clone + 'a>(
    toasts: &'a [Toast],
    on_dismiss: impl Fn(usize) -> M,
    on_undo: impl Fn(usize) -> M,
) -> Element<'a, M> {
    let mut stack = Column::new().spacing(space::SM).align_x(Alignment::End);
    for (index, toast) in toasts.iter().enumerate() {
        let mut content = row![
            icon_toned(icon_of(toast.kind), layout::ICON, tone_of(toast.kind)),
            text(&toast.text).size(size::BODY).width(iced::Length::Fill),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center);
        if toast.undo.is_some() {
            content = content.push(icon_button(
                icons::UNDO,
                "Undo",
                ButtonKind::Ghost,
                Some(on_undo(index)),
            ));
        }
        content = content.push(icon_only(icons::CLOSE, Some(on_dismiss(index))));
        stack = stack.push(
            container(content)
                .padding(Padding {
                    top: space::SM,
                    right: space::SM,
                    bottom: space::SM,
                    left: space::LG,
                })
                .width(layout::TOAST_WIDTH)
                .style(theme::toast),
        );
    }
    stack.into()
}
