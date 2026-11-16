//! A scrolling grid of tiles under day headings that builds only the rows in view.

use crate::theme::{self, layout, space};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{Column, Row, Space, container, responsive, scrollable};
use iced::{Alignment, Element, Length, Padding, Size};
use std::ops::Range;
use std::rc::Rc;

pub const GRID_ID: &str = "screenshots-grid";
const OVERSCAN: usize = 2;
const GAP: f32 = space::MD;
const GUTTER: f32 = space::SM;

/// Where the grid is scrolled to and how big its viewport is.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Window {
    pub offset: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Heading(usize),
    Tiles {
        day: usize,
        first: usize,
        len: usize,
    },
}

/// The rows of the grid for one content width.
#[derive(Debug, Clone)]
pub struct Plan {
    pub columns: usize,
    pub tile: Size,
    items: Vec<Item>,
    tops: Vec<f32>,
    total: f32,
    day_starts: Vec<usize>,
}

impl Plan {
    pub fn new(day_sizes: &[usize], width: f32) -> Self {
        let usable = (width - GUTTER).max(layout::SHOT_TILE_MIN_WIDTH);
        let columns =
            (((usable + GAP) / (layout::SHOT_TILE_MIN_WIDTH + GAP)).floor() as usize).max(1);
        let tile_width = ((usable - GAP * (columns - 1) as f32) / columns as f32).floor();
        let tile = Size::new(tile_width, (tile_width * layout::SHOT_TILE_ASPECT).round());
        let mut plan = Self {
            columns,
            tile,
            items: Vec::new(),
            tops: Vec::new(),
            total: 0.0,
            day_starts: Vec::new(),
        };
        let mut start = 0;
        for (day, &len) in day_sizes.iter().enumerate() {
            plan.day_starts.push(start);
            start += len;
            plan.push(Item::Heading(day));
            for first in (0..len).step_by(columns) {
                plan.push(Item::Tiles {
                    day,
                    first,
                    len: columns.min(len - first),
                });
            }
        }
        plan
    }

    fn push(&mut self, item: Item) {
        self.tops.push(self.total);
        self.total += self.height(item);
        self.items.push(item);
    }

    fn height(&self, item: Item) -> f32 {
        match item {
            Item::Heading(_) => layout::SHOT_HEADING_HEIGHT,
            Item::Tiles { .. } => self.tile.height + GAP,
        }
    }

    /// Items in view plus [`OVERSCAN`] on each side.
    pub fn visible(&self, offset: f32, height: f32) -> Range<usize> {
        if self.items.is_empty() {
            return 0..0;
        }
        let offset = offset.max(0.0);
        let first = self
            .tops
            .partition_point(|top| *top <= offset)
            .saturating_sub(1);
        let last = self
            .tops
            .partition_point(|top| *top < offset + height.max(0.0));
        first.saturating_sub(OVERSCAN)..(last + OVERSCAN).min(self.items.len())
    }

    /// Positions in the shown list of the tiles that `items` covers.
    pub fn tiles_in(&self, items: Range<usize>) -> Range<usize> {
        let mut covered = items.filter_map(|index| match self.items[index] {
            Item::Tiles { day, first, len } => {
                let start = self.day_starts[day] + first;
                Some(start..start + len)
            }
            Item::Heading(_) => None,
        });
        let Some(first) = covered.next() else {
            return 0..0;
        };
        let end = covered.next_back().map_or(first.end, |last| last.end);
        first.start..end
    }

    fn above(&self, items: &Range<usize>) -> f32 {
        self.tops.get(items.start).copied().unwrap_or(self.total)
    }

    fn below(&self, items: &Range<usize>) -> f32 {
        self.total - self.tops.get(items.end).copied().unwrap_or(self.total)
    }
}

/// Builds the grid. `heading(day)` draws a day heading; `tile(position, size)` draws the
/// tile at that position of the shown list. Both are called for visible rows only.
pub fn virtual_grid<'a, M: 'a>(
    day_sizes: &'a [usize],
    offset: f32,
    on_scroll: impl Fn(Window) -> M + 'a,
    heading: impl Fn(usize) -> Element<'a, M> + 'a,
    tile: impl Fn(usize, Size) -> Element<'a, M> + 'a,
) -> Element<'a, M> {
    let on_scroll = Rc::new(on_scroll);
    responsive(move |size| {
        let plan = Plan::new(day_sizes, size.width);
        let height = if size.height.is_finite() {
            size.height
        } else {
            layout::VIRTUAL_FALLBACK_HEIGHT
        };
        let items = plan.visible(offset, height);
        let mut content = Column::new()
            .width(Length::Fill)
            .push(Space::new().height(plan.above(&items)));
        for index in items.clone() {
            let item = plan.items[index];
            let row: Element<'a, M> = match item {
                Item::Heading(day) => container(heading(day))
                    .height(plan.height(item))
                    .align_y(Alignment::End)
                    .padding(Padding {
                        bottom: space::SM,
                        ..Padding::ZERO
                    })
                    .into(),
                Item::Tiles { day, first, len } => {
                    let start = plan.day_starts[day] + first;
                    let tiles = (start..start + len).map(|position| tile(position, plan.tile));
                    container(Row::with_children(tiles).spacing(GAP))
                        .height(plan.height(item))
                        .into()
                }
            };
            content = content.push(row);
        }
        content = content.push(Space::new().height(plan.below(&items)));
        let report = on_scroll.clone();
        scrollable(content.padding(Padding {
            right: GUTTER,
            ..Padding::ZERO
        }))
        .id(GRID_ID)
        .direction(Direction::Vertical(
            Scrollbar::new()
                .width(theme::SCROLLBAR_WIDTH)
                .scroller_width(theme::SCROLLBAR_WIDTH)
                .margin(space::XS / 2.0),
        ))
        .style(theme::scroll)
        .on_scroll(move |viewport| {
            let bounds = viewport.bounds();
            report(Window {
                offset: viewport.absolute_offset().y,
                width: bounds.width,
                height: bounds.height,
            })
        })
        .height(Length::Fill)
        .into()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_follow_the_width() {
        let narrow = Plan::new(&[10], layout::SHOT_TILE_MIN_WIDTH);
        assert_eq!(narrow.columns, 1);
        let wide = Plan::new(&[10], 4.0 * (layout::SHOT_TILE_MIN_WIDTH + GAP) + GUTTER);
        assert_eq!(wide.columns, 4);
        assert!(wide.tile.width >= layout::SHOT_TILE_MIN_WIDTH);
    }

    #[test]
    fn days_start_with_a_heading_and_wrap_rows() {
        let plan = Plan::new(&[5, 2], 4.0 * (layout::SHOT_TILE_MIN_WIDTH + GAP) + GUTTER);
        assert_eq!(
            plan.items,
            [
                Item::Heading(0),
                Item::Tiles {
                    day: 0,
                    first: 0,
                    len: 4
                },
                Item::Tiles {
                    day: 0,
                    first: 4,
                    len: 1
                },
                Item::Heading(1),
                Item::Tiles {
                    day: 1,
                    first: 0,
                    len: 2
                },
            ]
        );
        assert_eq!(plan.tiles_in(0..5), 0..7);
        assert_eq!(plan.tiles_in(3..5), 5..7);
        assert_eq!(plan.tiles_in(0..1), 0..0);
    }

    #[test]
    fn visible_rows_stay_few_for_a_long_list() {
        let plan = Plan::new(&[2000], 1000.0);
        let items = plan.visible(5_000.0, 600.0);
        assert!(items.len() < 12, "{items:?}");
        let tiles = plan.tiles_in(items);
        assert!(tiles.len() <= 12 * plan.columns, "{tiles:?}");
        assert!(tiles.start > 0);
    }
}
