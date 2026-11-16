use crate::theme::{self, layout, space};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{Column, Id, Space, container, responsive, scrollable};
use iced::{Element, Length};
use std::ops::Range;
use std::rc::Rc;

const OVERSCAN: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VirtualListState {
    pub offset: f32,
    pub viewport_height: f32,
}

impl VirtualListState {
    /// Indices of the rows to build: the ones in view plus two rows of overscan each side.
    pub fn visible(&self, count: usize, row_height: f32) -> Range<usize> {
        if count == 0 || row_height <= 0.0 {
            return 0..0;
        }
        let offset = self.offset.max(0.0);
        let first = (offset / row_height).floor() as usize;
        let last = ((offset + self.viewport_height.max(0.0)) / row_height).ceil() as usize;
        let end = (last + OVERSCAN).min(count);
        let start = first.saturating_sub(OVERSCAN).min(end);
        start..end
    }
}

/// A scrolling list that builds only the visible rows. `row` is called for visible indices only.
///
/// The viewport height is measured at layout time, so `state.viewport_height` only matters
/// when the list is placed somewhere with unbounded height. Keep `state.offset` in sync by
/// storing the value `on_scroll` reports.
pub fn virtual_list<'a, M: 'a>(
    state: &'a VirtualListState,
    count: usize,
    row_height: f32,
    on_scroll: impl Fn(f32) -> M + 'a,
    row: impl Fn(usize) -> Element<'a, M> + 'a,
) -> Element<'a, M> {
    build(None, state, count, row_height, on_scroll, row)
}

/// [`virtual_list`] with a scrollable `id`, so a screen can move it with
/// `operation::scroll_to`.
pub fn virtual_list_with_id<'a, M: 'a>(
    id: Id,
    state: &'a VirtualListState,
    count: usize,
    row_height: f32,
    on_scroll: impl Fn(f32) -> M + 'a,
    row: impl Fn(usize) -> Element<'a, M> + 'a,
) -> Element<'a, M> {
    build(Some(id), state, count, row_height, on_scroll, row)
}

fn build<'a, M: 'a>(
    id: Option<Id>,
    state: &'a VirtualListState,
    count: usize,
    row_height: f32,
    on_scroll: impl Fn(f32) -> M + 'a,
    row: impl Fn(usize) -> Element<'a, M> + 'a,
) -> Element<'a, M> {
    let on_scroll = Rc::new(on_scroll);
    let offset = state.offset;
    let fallback = if state.viewport_height > 0.0 {
        state.viewport_height
    } else {
        layout::VIRTUAL_FALLBACK_HEIGHT
    };
    responsive(move |size| {
        let viewport_height = if size.height.is_finite() {
            size.height
        } else {
            fallback
        };
        let range = VirtualListState {
            offset,
            viewport_height,
        }
        .visible(count, row_height);
        let above = range.start as f32 * row_height;
        let below = count.saturating_sub(range.end) as f32 * row_height;
        let mut content = Column::new()
            .width(Length::Fill)
            .push(Space::new().height(above));
        for index in range {
            content = content.push(container(row(index)).height(row_height).width(Length::Fill));
        }
        content = content.push(Space::new().height(below));
        let report = on_scroll.clone();
        let list = match id.clone() {
            Some(id) => scrollable(content).id(id),
            None => scrollable(content),
        };
        list.direction(Direction::Vertical(
            Scrollbar::new()
                .width(theme::SCROLLBAR_WIDTH)
                .scroller_width(theme::SCROLLBAR_WIDTH)
                .margin(space::XS / 2.0),
        ))
        .style(theme::scroll)
        .on_scroll(move |viewport| report(viewport.absolute_offset().y))
        .height(Length::Fill)
        .into()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text;
    use iced::{Settings, Size};
    use iced_test::simulator::Simulator;
    use std::cell::Cell;

    #[test]
    fn visible_range_covers_viewport_plus_overscan() {
        let state = VirtualListState {
            offset: 0.0,
            viewport_height: 400.0,
        };
        assert_eq!(state.visible(5000, 40.0), 0..12);
    }

    #[test]
    fn visible_range_clamps_at_end() {
        let state = VirtualListState {
            offset: 199_800.0,
            viewport_height: 400.0,
        };
        assert_eq!(state.visible(5000, 40.0), 4993..5000);
    }

    #[test]
    fn empty_list_has_no_rows() {
        let state = VirtualListState::default();
        assert_eq!(state.visible(0, 40.0), 0..0);
    }

    #[test]
    fn five_thousand_rows_build_only_visible_ones() {
        let calls = Cell::new(0usize);
        let state = VirtualListState {
            offset: 0.0,
            viewport_height: 400.0,
        };
        let list: Element<'_, ()> = virtual_list(
            &state,
            5000,
            40.0,
            |_| (),
            |index| {
                calls.set(calls.get() + 1);
                text(index.to_string()).into()
            },
        );
        let _ui = Simulator::with_size(Settings::default(), Size::new(400.0, 400.0), list);
        assert!(calls.get() > 0);
        assert!(calls.get() <= 14, "built {} rows", calls.get());
    }
}
