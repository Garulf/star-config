//! The grid page: filter or selection bar above the day-grouped tiles.

use super::grid::virtual_grid;
use super::icons as glyphs;
use super::library::{
    ARCHIVE, Period, Source, channel_label, count_line, file_name, screenshots as count_of, taken,
};
use super::{Busy, Message, State, Thumb};
use crate::context::Context;
use crate::screen::Screen;
use crate::screens::backups::when;
use crate::theme::{self, ButtonKind, Tone, layout, size, space};
use crate::widgets::{
    button, empty_state, empty_state_icon, hint, icon_button, icon_toned, muted, page_fixed,
    pick_list, segmented, strong, tag,
};
use iced::widget::text::Wrapping;
use iced::widget::{
    Row, Space, button as tile_button, checkbox, column, container, image, row, stack, text,
};
use iced::{Alignment, ContentFit, Element, Length, Padding, Size};
use sc_core::screenshots::Shot;
use std::path::Path;

pub const SUBTITLE: &str = "Captures from every channel of this install, newest first.";
pub const EMPTY_TITLE: &str = "No screenshots yet";
pub const EMPTY_BODY: &str =
    "Press Print Screen in the game. Captures from every channel of this install show up here.";
const LOOKING: &str = "Looking for screenshots...";

impl State {
    pub(super) fn grid_page<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let title = Screen::Screenshots.label();
        let trailing = vec![self.refresh_button()];
        if !self.loaded {
            let looking = container(muted(LOOKING)).center(Length::Fill);
            return page_fixed(title, Some(SUBTITLE), trailing, looking);
        }
        if self.shots.is_empty() {
            let empty = empty_state_icon(crate::icons::SCREENSHOTS, EMPTY_TITLE, EMPTY_BODY, None);
            return page_fixed(title, Some(SUBTITLE), trailing, empty);
        }
        let bar = if self.selected.is_empty() {
            self.filter_bar(ctx)
        } else {
            self.selection_bar()
        };
        let body = column![bar, self.tiles()].spacing(space::LG);
        page_fixed(title, Some(SUBTITLE), trailing, body)
    }

    fn refresh_button(&self) -> Element<'_, Message> {
        let (label, on_press) = if self.scanning {
            ("Refreshing...", None)
        } else {
            ("Refresh", Some(Message::Refresh))
        };
        icon_button(glyphs::REFRESH, label, ButtonKind::Secondary, on_press)
    }

    fn tiles(&self) -> Element<'_, Message> {
        if self.shown.is_empty() {
            return empty_state(
                "Nothing matches these filters",
                "Try another channel or period, or turn off Favourites.",
                Some(("Show all", Message::ShowAll)),
            );
        }
        virtual_grid(
            &self.day_sizes,
            self.window.offset,
            Message::Scrolled,
            |day| self.day_heading(day),
            |position, size| self.tile(position, size),
        )
    }

    fn filter_bar<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let source = |label: &str, source: Source| {
            let active = self.filter.source == source;
            (label.to_string(), active, Message::Source(source))
        };
        let mut sources = vec![source("All", Source::All)];
        for channel in &ctx.channels {
            sources.push(source(&channel.name, Source::Channel(channel.name.clone())));
        }
        if self.has_archived() || self.filter.source == Source::Archive {
            sources.push(source(ARCHIVE, Source::Archive));
        }
        let favourites = segmented(vec![(
            "Favourites",
            self.filter.favourites,
            Message::Favourites(!self.filter.favourites),
        )]);
        let period = pick_list(&Period::ALL[..], Some(self.filter.period), Message::Period)
            .width(layout::SHOT_PERIOD_WIDTH);
        bar(vec![
            segmented(sources),
            favourites,
            period.into(),
            Space::new().width(Length::Fill).into(),
            muted(count_line(self.shown.len(), self.shown_bytes)).into(),
        ])
    }

    fn selection_bar(&self) -> Element<'_, Message> {
        let paths: Vec<_> = self.selected.iter().cloned().collect();
        let idle = self.busy.is_none();
        let favourite = if self.all_favourites(&paths) {
            "Remove favourite"
        } else {
            "Favourite"
        };
        let can_archive = idle && !self.archivable(self.selected.iter()).is_empty();
        let delete = if self.busy == Some(Busy::Deleting) {
            "Moving to the trash..."
        } else {
            "Delete"
        };
        bar(vec![
            strong(format!("{} selected", self.selected.len())).into(),
            Space::new().width(space::SM).into(),
            button(
                favourite,
                ButtonKind::Secondary,
                idle.then_some(Message::FavouriteSelected),
            ),
            button(
                "Archive",
                ButtonKind::Secondary,
                can_archive.then_some(Message::ArchiveSelected),
            ),
            button(
                delete,
                ButtonKind::Danger,
                idle.then_some(Message::DeleteSelected),
            ),
            Space::new().width(Length::Fill).into(),
            button(
                "Clear selection",
                ButtonKind::Ghost,
                Some(Message::ClearSelection),
            ),
        ])
    }

    fn has_archived(&self) -> bool {
        self.shots.iter().any(|shot| shot.channel.is_none())
    }

    fn day_heading(&self, day: usize) -> Element<'_, Message> {
        let day = &self.days[day];
        row![strong(day.heading.as_str()), hint(count_of(day.len))]
            .spacing(space::SM)
            .align_y(Alignment::Center)
            .into()
    }

    fn tile(&self, position: usize, size: Size) -> Element<'_, Message> {
        let shot = &self.shots[self.shown[position]];
        let inner = Size::new(
            size.width - 2.0 * layout::SHOT_TILE_RING,
            size.height - 2.0 * layout::SHOT_TILE_RING,
        );
        let selected = self.selected.contains(&shot.path);
        let overlay = column![
            self.tile_top(shot, selected),
            Space::new().height(Length::Fill),
            tile_strip(shot),
        ]
        .width(inner.width)
        .height(inner.height);
        tile_button(stack![self.picture(shot, inner), overlay])
            .padding(layout::SHOT_TILE_RING)
            .style(theme::shot_tile(selected))
            .on_press(Message::Open(shot.path.clone()))
            .into()
    }

    fn picture(&self, shot: &Shot, inner: Size) -> Element<'_, Message> {
        match self.thumbs.get(&shot.path) {
            Some(Thumb::Ready(handle)) => image(handle.clone())
                .width(inner.width)
                .height(inner.height)
                .content_fit(ContentFit::Cover)
                .into(),
            Some(Thumb::Unreadable) => generic_tile(&shot.path, inner),
            Some(Thumb::Pending) | None => container(Space::new())
                .width(inner.width)
                .height(inner.height)
                .style(theme::shot_placeholder)
                .into(),
        }
    }

    fn tile_top(&self, shot: &Shot, selected: bool) -> Element<'_, Message> {
        let path = shot.path.clone();
        let tick = checkbox(selected)
            .on_toggle(move |on| Message::Tick(path.clone(), on))
            .size(layout::CHECKBOX_SIZE)
            .style(theme::check);
        let mut top = Row::new()
            .push(container(tick).padding(space::XS).style(theme::shot_chip))
            .push(Space::new().width(Length::Fill));
        if self.is_favourite(&shot.path) {
            top = top.push(
                container(icon_toned(glyphs::STAR_FILLED, layout::ICON, Tone::Accent))
                    .padding(space::XS)
                    .style(theme::shot_chip),
            );
        }
        top.padding(space::SM).into()
    }
}

fn bar<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    container(
        Row::with_children(items)
            .spacing(space::MD)
            .align_y(Alignment::Center),
    )
    .height(layout::CONTROL_HEIGHT)
    .align_y(Alignment::Center)
    .into()
}

fn tile_strip<'a>(shot: &'a Shot) -> Element<'a, Message> {
    let tone = if shot.channel.is_some() {
        Tone::Neutral
    } else {
        Tone::Muted
    };
    container(
        row![
            text(when::clock(taken(shot)))
                .size(size::CAPTION)
                .style(theme::on_image),
            Space::new().width(Length::Fill),
            tag(channel_label(shot), tone),
        ]
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: space::XS,
        right: space::SM,
        bottom: space::XS,
        left: space::SM,
    })
    .width(Length::Fill)
    .style(theme::shot_strip)
    .into()
}

fn generic_tile<'a>(path: &Path, inner: Size) -> Element<'a, Message> {
    let name = file_name(path);
    container(
        column![
            icon_toned(glyphs::FILE, layout::ICON_EMPTY, Tone::Muted),
            hint(name)
                .align_x(Alignment::Center)
                .wrapping(Wrapping::WordOrGlyph),
        ]
        .spacing(space::SM)
        .align_x(Alignment::Center),
    )
    .padding(Padding {
        top: space::SM,
        right: space::LG,
        bottom: space::XL,
        left: space::LG,
    })
    .center_x(inner.width)
    .center_y(inner.height)
    .style(theme::shot_placeholder)
    .into()
}
