//! The viewer: the picture on a dark stage, with details and actions beside it.

use super::icons as glyphs;
use super::library::{ARCHIVE, extension, file_name, taken};
use super::{Busy, Message, RENAME_ID, Rename, Stage, State, Thumb, Viewer};
use crate::context::Context;
use crate::screens::backups::when;
use crate::screens::units;
use crate::theme::{self, ButtonKind, Tone, layout, size, space};
use crate::widgets::{
    button, hint, icon_button, icon_only, icon_toned, label, muted, section, text_field, title,
};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::{IntoFragment, Wrapping};
use iced::widget::{self, Column, column, container, image, row, scrollable, svg, text};
use iced::{Alignment, ContentFit, Element, Length, Padding};

const ACTION_PADDING: Padding = Padding {
    top: space::SM,
    right: space::MD,
    bottom: space::SM,
    left: space::MD,
};

const LIST_ACTION_PADDING: Padding = Padding {
    top: space::XS,
    right: space::MD,
    bottom: space::XS,
    left: space::MD,
};

impl State {
    pub(super) fn viewer_page<'a>(
        &'a self,
        ctx: &'a Context,
        viewer: &'a Viewer,
    ) -> Element<'a, Message> {
        let body = row![self.stage(viewer), self.info_panel(ctx, viewer)]
            .spacing(space::LG)
            .height(Length::Fill);
        column![self.viewer_bar(viewer), body]
            .spacing(space::LG)
            .padding(space::XL)
            .into()
    }

    fn viewer_bar<'a>(&'a self, viewer: &'a Viewer) -> Element<'a, Message> {
        let position = self.position(&viewer.path);
        let count = self.shown.len();
        let counter = position.map_or_else(String::new, |at| format!("{} of {count}", at + 1));
        let previous = position.filter(|at| *at > 0).map(|_| Message::Previous);
        let next = position.filter(|at| at + 1 < count).map(|_| Message::Next);
        row![
            icon_button(
                glyphs::ARROW_LEFT,
                "Back to all screenshots",
                ButtonKind::Ghost,
                Some(Message::Back)
            ),
            container(title(file_name(&viewer.path)).wrapping(Wrapping::WordOrGlyph))
                .width(Length::Fill),
            muted(counter),
            row![
                icon_only(glyphs::CHEVRON_LEFT, previous),
                icon_only(glyphs::CHEVRON_RIGHT, next),
            ]
            .spacing(space::XS),
        ]
        .spacing(space::LG)
        .align_y(Alignment::Center)
        .into()
    }

    fn stage<'a>(&'a self, viewer: &'a Viewer) -> Element<'a, Message> {
        let content: Element<'a, Message> = match &viewer.stage {
            Stage::Ready(picture) => image::viewer(picture.handle.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            Stage::Loading => match self.thumbs.get(&viewer.path) {
                Some(Thumb::Ready(handle)) => image(handle.clone())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(ContentFit::Contain)
                    .into(),
                _ => container(text("Loading...").size(size::BODY).style(theme::on_image))
                    .center(Length::Fill)
                    .into(),
            },
            Stage::Unreadable => container(
                column![
                    icon_toned(glyphs::FILE, layout::ICON_EMPTY, Tone::Muted),
                    text("This file cannot be shown as a picture.")
                        .size(size::BODY)
                        .style(theme::on_image),
                ]
                .spacing(space::MD)
                .align_x(Alignment::Center),
            )
            .center(Length::Fill)
            .into(),
        };
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(space::XS)
            .style(theme::image_stage)
            .clip(true)
            .into()
    }

    fn info_panel<'a>(&'a self, ctx: &'a Context, viewer: &'a Viewer) -> Element<'a, Message> {
        let panel = column![
            section("Details", self.details(ctx, viewer)),
            section("Share", self.share_actions(viewer)),
            section("Manage", self.manage_actions(viewer)),
        ]
        .spacing(space::LG)
        .padding(Padding {
            right: space::MD,
            ..Padding::ZERO
        });
        scrollable(panel)
            .direction(Direction::Vertical(
                Scrollbar::new()
                    .width(theme::SCROLLBAR_WIDTH)
                    .scroller_width(theme::SCROLLBAR_WIDTH)
                    .margin(space::XS / 2.0),
            ))
            .style(theme::scroll)
            .width(layout::SHOT_INFO_WIDTH)
            .height(Length::Fill)
            .into()
    }

    fn details<'a>(&'a self, ctx: &'a Context, viewer: &'a Viewer) -> Element<'a, Message> {
        let resolution = match &viewer.stage {
            Stage::Ready(picture) => format!("{} x {}", picture.width, picture.height),
            Stage::Loading => "Reading...".to_string(),
            Stage::Unreadable => "Unknown".to_string(),
        };
        let mut items = vec![detail("Resolution", resolution)];
        if let Some(shot) = self.shot(&viewer.path) {
            items.push(detail("Size", units::bytes(shot.size)));
            items.push(detail("Captured", when::full(taken(shot))));
            items.push(detail("Channel", self.channel_text(viewer)));
        }
        items.push(
            column![
                hint("Location"),
                label(ctx.display_path(&viewer.path)).wrapping(Wrapping::WordOrGlyph),
            ]
            .spacing(space::XS)
            .into(),
        );
        Column::with_children(items)
            .spacing(space::SM)
            .padding(space::MD)
            .into()
    }

    fn channel_text(&self, viewer: &Viewer) -> String {
        match self
            .shot(&viewer.path)
            .and_then(|shot| shot.channel.clone())
        {
            Some(channel) => channel,
            None => match self
                .index
                .as_ref()
                .and_then(|index| index.origin.get(&viewer.path))
            {
                Some(origin) => format!("{ARCHIVE}, from {origin}"),
                None => ARCHIVE.to_string(),
            },
        }
    }

    fn share_actions<'a>(&'a self, viewer: &'a Viewer) -> Element<'a, Message> {
        let idle = self.busy.is_none();
        let picture = idle && !matches!(viewer.stage, Stage::Unreadable);
        column![
            action(
                glyphs::COPY,
                self.busy_label(Busy::CopyImage, "Copy image", "Copying..."),
                ButtonKind::Primary,
                picture.then_some(Message::CopyImage),
            ),
            action(
                glyphs::SHRINK,
                self.busy_label(Busy::CopySmaller, "Copy smaller copy", "Copying..."),
                ButtonKind::Ghost,
                picture.then_some(Message::CopySmaller),
            ),
            action(
                glyphs::DOWNLOAD,
                self.busy_label(Busy::SaveSmaller, "Save smaller copy", "Saving..."),
                ButtonKind::Ghost,
                picture.then_some(Message::SaveSmaller),
            ),
            action(
                glyphs::LINK,
                "Copy path",
                ButtonKind::Ghost,
                idle.then_some(Message::CopyPath),
            ),
            action(
                crate::icons::FOLDER,
                "Show in folder",
                ButtonKind::Ghost,
                Some(Message::ShowInFolder),
            ),
        ]
        .spacing(space::XS)
        .padding(space::SM)
        .into()
    }

    fn manage_actions<'a>(&'a self, viewer: &'a Viewer) -> Element<'a, Message> {
        let idle = self.busy.is_none();
        let favourite = self.is_favourite(&viewer.path);
        let archivable = self
            .shot(&viewer.path)
            .is_some_and(|shot| shot.channel.is_some());
        let mut list = Column::new().spacing(space::XS).padding(space::SM);
        list = list.push(if favourite {
            action(
                glyphs::STAR_FILLED,
                "Remove favourite",
                ButtonKind::Ghost,
                Some(Message::ToggleFavourite),
            )
        } else {
            action(
                glyphs::STAR,
                "Favourite",
                ButtonKind::Ghost,
                Some(Message::ToggleFavourite),
            )
        });
        list = list.push(match &viewer.rename {
            Some(rename) => self.rename_editor(viewer, rename),
            None => action(
                glyphs::PENCIL,
                "Rename",
                ButtonKind::Ghost,
                idle.then_some(Message::Rename),
            ),
        });
        list = list.push(action(
            crate::icons::BACKUPS,
            self.busy_label(Busy::Archiving, "Archive", "Archiving..."),
            ButtonKind::Ghost,
            (idle && archivable).then_some(Message::Archive),
        ));
        list.push(action(
            glyphs::TRASH,
            self.busy_label(Busy::Deleting, "Delete", "Moving to the trash..."),
            ButtonKind::DangerGhost,
            idle.then_some(Message::Delete),
        ))
        .into()
    }

    fn rename_editor<'a>(&'a self, viewer: &'a Viewer, rename: &'a Rename) -> Element<'a, Message> {
        let saving = self.busy == Some(Busy::Renaming);
        let field = text_field("File name", &rename.stem, Message::RenameInput)
            .id(RENAME_ID)
            .on_submit(Message::RenameSave)
            .width(Length::Fill);
        let mut editor = column![
            row![field, muted(extension(&viewer.path))]
                .spacing(space::XS)
                .align_y(Alignment::Center)
        ]
        .spacing(space::SM)
        .padding(Padding {
            top: space::XS,
            right: space::XS,
            bottom: space::SM,
            left: space::XS,
        });
        if let Some(error) = &rename.error {
            editor = editor.push(
                text(error.as_str())
                    .size(size::CAPTION)
                    .style(theme::toned(Tone::Danger)),
            );
        }
        editor
            .push(
                row![
                    button(
                        if saving { "Saving..." } else { "Save" },
                        ButtonKind::Primary,
                        (!saving).then_some(Message::RenameSave),
                    ),
                    button("Cancel", ButtonKind::Ghost, Some(Message::RenameCancel)),
                ]
                .spacing(space::SM),
            )
            .into()
    }

    fn busy_label(&self, busy: Busy, idle: &'static str, working: &'static str) -> &'static str {
        if self.busy == Some(busy) {
            working
        } else {
            idle
        }
    }
}

fn detail<'a>(name: &'a str, value: String) -> Element<'a, Message> {
    row![
        hint(name).width(layout::SHOT_DETAIL_LABEL_WIDTH),
        label(value).width(Length::Fill),
    ]
    .align_y(Alignment::Center)
    .into()
}

/// A full-width button with a leading icon, for the action lists of the info panel.
fn action<'a>(
    glyph: &'static [u8],
    caption: impl IntoFragment<'a>,
    kind: ButtonKind,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let enabled = on_press.is_some();
    let glyph: Element<'a, Message> = match (kind, enabled) {
        (ButtonKind::Primary, true) => svg(svg::Handle::from_memory(glyph))
            .width(layout::ICON)
            .height(layout::ICON)
            .style(theme::icon_on_accent)
            .into(),
        (ButtonKind::DangerGhost, true) => icon_toned(glyph, layout::ICON, Tone::Danger),
        (_, true) => icon_toned(glyph, layout::ICON, Tone::Neutral),
        (_, false) => icon_toned(glyph, layout::ICON, Tone::Muted),
    };
    widget::button(
        row![glyph, text(caption).size(size::BODY)]
            .spacing(space::SM)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(if kind == ButtonKind::Primary {
        ACTION_PADDING
    } else {
        LIST_ACTION_PADDING
    })
    .style(theme::button_style(kind))
    .on_press_maybe(on_press)
    .into()
}
