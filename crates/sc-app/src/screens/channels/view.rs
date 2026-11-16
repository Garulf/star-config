use super::jobs::ChannelFacts;
use super::words::{self, PARTS};
use super::{CopyStage, Message, Pair, Running, State, SwapStage};
use crate::context::Context;
use crate::icons;
use crate::screen::Screen;
use crate::screens::units;
use crate::theme::{self, ButtonKind, Tone, layout, space};
use crate::widgets::{
    banner, button, checkbox, heading, hint, icon_only, icon_toned, label, muted, page, pick_list,
    progress, rows, section, strong, tag,
};
use iced::widget::{Column, Row, Space, column, container, row};
use iced::{Alignment, Element, Length, Padding};
use sc_core::channels::{CopyPlan, Part, SwapJournal};

const ARROWS: &[u8] = include_bytes!("../../../assets/icons/arrow-left-right.svg");
const CARDS_PER_ROW: usize = 3;
const ROW_PADDING: Padding = Padding {
    top: space::MD,
    right: space::LG,
    bottom: space::MD,
    left: space::LG,
};
const NOTE_INDENT: f32 = layout::CHECKBOX_SIZE + space::SM;

impl State {
    pub fn view<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let mut body = Column::new().spacing(space::XL);
        if let Some(pending) = self.pending_banner(ctx) {
            body = body.push(pending);
        }
        if let Some(notice) = &self.notice {
            body = body.push(banner(
                notice.kind,
                &notice.text,
                Some(("Dismiss", Message::DismissNotice)),
            ));
        }
        body = body
            .push(self.channel_cards())
            .push(self.pair_picker())
            .push(section("Game files", self.game_files(ctx)))
            .push(section("Settings", self.settings_rows(ctx)));
        page(
            Screen::Channels.label(),
            Some(words::SUBTITLE),
            Vec::new(),
            body,
        )
    }

    fn pending_banner(&self, ctx: &Context) -> Option<Element<'_, Message>> {
        let pending = self.pending.as_ref()?;
        if self.stuck {
            return None;
        }
        let blocked = self.running_blocker(ctx).is_some();
        let enabled = !blocked && !self.resuming;
        let on = |how| enabled.then(|| Message::Resume(Box::new(pending.clone()), how));
        let mut text = column![
            strong(words::PENDING_TITLE),
            label(words::pending_body(&pending.journal)),
        ]
        .spacing(space::XS)
        .width(Length::Fill);
        if blocked {
            text = text.push(hint(words::RESUME_BLOCKED));
        } else if self.resuming {
            text = text.push(hint(words::RESUMING));
        }
        let content = row![
            icon_toned(icons::WARNING, layout::ICON, Tone::Warning),
            text,
            button(
                "Finish the swap",
                ButtonKind::Primary,
                on(sc_core::channels::Resume::Finish)
            ),
            button(
                "Undo it",
                ButtonKind::Secondary,
                on(sc_core::channels::Resume::Undo)
            ),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center);
        Some(
            container(content)
                .padding(Padding {
                    top: space::MD,
                    right: space::MD,
                    bottom: space::MD,
                    left: space::LG,
                })
                .width(Length::Fill)
                .style(theme::banner(Tone::Warning))
                .into(),
        )
    }

    fn channel_cards(&self) -> Element<'_, Message> {
        let installed: Vec<Element<'_, Message>> = self
            .facts
            .iter()
            .filter(|facts| facts.installed)
            .map(|facts| self.channel_card(facts))
            .collect();
        let missing: Vec<&str> = self
            .facts
            .iter()
            .filter(|facts| !facts.installed)
            .map(|facts| facts.name.as_str())
            .collect();
        let mut grid = Column::new().spacing(space::LG);
        let mut current = Row::new().spacing(space::LG);
        let count = installed.len();
        for (index, card) in installed.into_iter().enumerate() {
            if index > 0 && index.is_multiple_of(CARDS_PER_ROW) {
                grid = grid.push(current);
                current = Row::new().spacing(space::LG);
            }
            current = current.push(card);
        }
        if !count.is_multiple_of(CARDS_PER_ROW) {
            for _ in count % CARDS_PER_ROW..CARDS_PER_ROW {
                current = current.push(Space::new().width(Length::Fill));
            }
        }
        if count > 0 {
            grid = grid.push(current);
        }
        if !missing.is_empty() {
            grid = grid.push(missing_line(&missing));
        }
        if !self.surveyed {
            grid = grid.push(muted("Looking at the channels in this install..."));
        }
        column![
            container(heading("CHANNELS IN THIS INSTALL")).padding(Padding {
                left: space::XS,
                ..Padding::ZERO
            }),
            grid,
        ]
        .spacing(space::SM)
        .into()
    }

    fn channel_card<'a>(&'a self, facts: &'a ChannelFacts) -> Element<'a, Message> {
        let title = row![
            strong(facts.name.as_str()).width(Length::Fill),
            tag("Installed", Tone::Success)
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);
        let size_and_build: Element<'a, Message> = {
            let size = match self.sizes.as_ref().and_then(|sizes| sizes.get(&facts.name)) {
                Some(bytes) => units::bytes(*bytes),
                None => words::CALCULATING.to_string(),
            };
            let build = facts
                .build
                .clone()
                .unwrap_or_else(|| "Build unknown".to_string());
            row![label(size).width(Length::Fill), hint(build)]
                .spacing(space::SM)
                .align_y(Alignment::Center)
                .into()
        };
        let user_files = hint(words::user_files_line(
            facts.has_settings,
            facts.screenshots,
        ));
        container(column![title, size_and_build, user_files].spacing(space::SM))
            .padding(space::LG)
            .width(Length::Fill)
            .style(theme::card)
            .into()
    }

    fn pair_picker(&self) -> Element<'_, Message> {
        let names: Vec<String> = self.facts.iter().map(|facts| facts.name.clone()).collect();
        let locked = self.locked();
        let picker = |selected: Option<String>, on: fn(String) -> Message| {
            pick_list(names.clone(), selected, on)
                .placeholder("Choose a channel")
                .width(layout::CONTROL_WIDTH)
        };
        let arrow = icon_only(ARROWS, (!locked).then_some(Message::Reverse));
        let pickers = row![
            label("From"),
            picker(self.from.clone(), Message::PickFrom),
            arrow,
            label("To"),
            picker(self.to.clone(), Message::PickTo),
            hint(self.pair_hint()).width(Length::Fill),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center);
        let mut body = column![pickers].spacing(space::MD);
        if let Pair::Refused(why) = &self.pair {
            body = body.push(label(why.as_str()).style(theme::toned(Tone::Danger)));
        }
        container(body)
            .padding(space::LG)
            .width(Length::Fill)
            .style(theme::card)
            .into()
    }

    fn pair_hint(&self) -> String {
        match self.facts_of(self.to.as_deref()) {
            Some(facts) if !facts.installed => {
                format!("{} is created when you copy game files to it.", facts.name)
            }
            _ => String::new(),
        }
    }

    fn game_files<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let mut items: Vec<Element<'a, Message>> = vec![padded(hint(words::GAME_CAPTION))];
        let body: Element<'a, Message> = match (&self.copy, &self.swap) {
            (CopyStage::Running(running), _) => self.copying(running),
            (_, SwapStage::Swapping) => padded(muted(words::SWAPPING)),
            (_, SwapStage::Planning) => padded(muted("Working out the swap...")),
            (_, SwapStage::Confirming { journal, travel }) => {
                self.swap_confirm(ctx, journal, *travel)
            }
            (_, SwapStage::Failed(why)) => failure(why, Message::CancelSwap),
            (CopyStage::Planning, _) => padded(muted(words::PLANNING)),
            (CopyStage::Planned(plan), _) => self.copy_summary(ctx, plan),
            (CopyStage::Failed(why), _) => failure(why, Message::CloseCopy),
            (CopyStage::Idle, SwapStage::Idle) => self.game_actions(ctx),
        };
        items.push(body);
        rows(items)
    }

    fn game_actions<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let reason: Element<'a, Message> = match self.game_blocker(ctx) {
            Some(why) => warning(why),
            None if self.pair == Pair::Allowed && !self.swap_target_ready() => {
                hint(words::SWAP_NEEDS_BOTH).into()
            }
            None => Space::new().into(),
        };
        padded(
            row![
                container(reason).width(Length::Fill),
                button(
                    "Swap game files",
                    ButtonKind::Secondary,
                    self.can_swap(ctx).then_some(Message::AskSwap),
                ),
                button(
                    "Copy game files",
                    ButtonKind::Primary,
                    self.can_copy(ctx).then_some(Message::PlanCopy),
                ),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        )
    }

    fn copy_summary<'a>(&'a self, ctx: &'a Context, plan: &'a CopyPlan) -> Element<'a, Message> {
        let mut body = column![
            strong(format!(
                "Copy {} to {}",
                name_of(&plan.source),
                name_of(&plan.target)
            )),
            column![label(words::plan_summary(plan)), hint(words::NOT_COPIED)].spacing(space::XS),
            checkbox(
                words::COMPARE_CONTENTS,
                self.options.compare_contents,
                Message::CompareContents
            ),
        ]
        .spacing(space::MD);
        if plan.needs_in_place {
            body = body.push(checkbox(
                words::IN_PLACE,
                self.options.in_place,
                Message::InPlace,
            ));
        }
        let has_work = plan.files.iter().any(|file| file.copy);
        let room = !plan.needs_in_place || self.options.in_place;
        let start = (has_work && room && self.can_copy(ctx)).then_some(Message::StartCopy);
        let reason: Element<'a, Message> = match self.game_blocker(ctx) {
            Some(why) => warning(why),
            None => Space::new().into(),
        };
        body = body.push(
            row![
                container(reason).width(Length::Fill),
                button("Cancel", ButtonKind::Secondary, Some(Message::CloseCopy)),
                button("Start copy", ButtonKind::Primary, start),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        );
        padded(body)
    }

    fn copying<'a>(&'a self, running: &'a Running) -> Element<'a, Message> {
        let plan = &running.plan;
        let total = running
            .progress
            .map_or(plan.bytes_to_copy, |p| p.bytes_total);
        let done = running.progress.map_or(0, |p| p.bytes_done);
        let fraction = if total == 0 {
            0.0
        } else {
            (done as f64 / total as f64) as f32
        };
        let index = running.progress.map_or(0, |p| p.files_done);
        let copying: Vec<_> = plan.files.iter().filter(|file| file.copy).collect();
        let current = copying
            .get(index)
            .or(copying.last())
            .map(|file| file.rel.display().to_string())
            .unwrap_or_default();
        let status = if running.stopping {
            words::STOPPING.to_string()
        } else {
            format!("Copying {current}")
        };
        padded(
            column![
                strong(format!(
                    "Copying {} to {}",
                    name_of(&plan.source),
                    name_of(&plan.target)
                )),
                progress(fraction, words::progress_caption(done, total)),
                row![
                    hint(status).width(Length::Fill),
                    button(
                        "Cancel",
                        ButtonKind::Secondary,
                        (!running.stopping).then_some(Message::CancelCopy),
                    ),
                ]
                .spacing(space::SM)
                .align_y(Alignment::Center),
            ]
            .spacing(space::MD),
        )
    }

    fn swap_confirm<'a>(
        &'a self,
        ctx: &'a Context,
        journal: &'a SwapJournal,
        travel: bool,
    ) -> Element<'a, Message> {
        let steps = words::swap_steps(journal, travel)
            .into_iter()
            .enumerate()
            .fold(Column::new().spacing(space::XS), |list, (index, step)| {
                list.push(
                    row![hint(format!("{}.", index + 1)), label(step)]
                        .spacing(space::SM)
                        .align_y(Alignment::Center),
                )
            });
        let ready = travel == self.swap_travel && self.can_swap(ctx);
        padded(
            column![
                strong(words::swap_title(&journal.a, &journal.b)),
                steps,
                label(words::swap_summary(
                    &journal.a,
                    &journal.b,
                    self.swap_travel
                )),
                checkbox(words::SWAP_TRAVEL, self.swap_travel, Message::SwapTravel),
                row![
                    Space::new().width(Length::Fill),
                    button("Cancel", ButtonKind::Secondary, Some(Message::CancelSwap)),
                    button("Swap", ButtonKind::Primary, ready.then_some(Message::Swap)),
                ]
                .spacing(space::SM)
                .align_y(Alignment::Center),
            ]
            .spacing(space::MD),
        )
    }

    fn settings_rows<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let mut items: Vec<Element<'a, Message>> = vec![padded(hint(words::SETTINGS_CAPTION))];
        let from = self.facts_of(self.from.as_deref());
        for (index, part) in PARTS.iter().enumerate() {
            let files = from.map_or(0, |facts| facts.parts[index]);
            items.push(self.part_row(*part, self.ticked[index], files));
        }
        let enabled = self.can_settings(ctx);
        let reason: Element<'a, Message> = match (self.settings_busy, self.settings_blocker(ctx)) {
            (true, _) => muted("Working...").into(),
            (false, Some(why)) => hint(why).into(),
            (false, None) => Space::new().into(),
        };
        items.push(padded(
            row![
                container(reason).width(Length::Fill),
                button(
                    "Swap settings",
                    ButtonKind::Secondary,
                    enabled.then_some(Message::SwapSettings),
                ),
                button(
                    "Copy settings",
                    ButtonKind::Secondary,
                    enabled.then_some(Message::CopySettings),
                ),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        ));
        rows(items)
    }

    fn part_row<'a>(&'a self, part: Part, ticked: bool, files: usize) -> Element<'a, Message> {
        let channel = self.from.as_deref().unwrap_or_default();
        let mut body = column![
            row![
                checkbox(
                    words::part_label(part),
                    ticked,
                    move |on| Message::TickPart(part, on)
                ),
                Space::new().width(Length::Fill),
                hint(words::part_count(files, channel)),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(space::XS);
        if part == Part::StarStrings {
            body = body.push(container(hint(words::STARSTRINGS_NOTE)).padding(Padding {
                left: NOTE_INDENT,
                ..Padding::ZERO
            }));
        }
        padded(body)
    }
}

/// The channels this install could have but does not, on one quiet line.
fn missing_line<'a>(names: &[&str]) -> Element<'a, Message> {
    container(
        row![tag("Not installed", Tone::Muted), hint(names.join(", "))]
            .spacing(space::SM)
            .align_y(Alignment::Center),
    )
    .padding(Padding {
        left: space::XS,
        ..Padding::ZERO
    })
    .into()
}

fn padded<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .padding(ROW_PADDING)
        .width(Length::Fill)
        .into()
}

fn warning<'a>(text: &'a str) -> Element<'a, Message> {
    row![
        icon_toned(icons::WARNING, layout::ICON, Tone::Warning),
        label(text)
    ]
    .spacing(space::SM)
    .align_y(Alignment::Center)
    .into()
}

fn failure<'a>(why: &'a str, close: Message) -> Element<'a, Message> {
    padded(
        row![
            label(why)
                .style(theme::toned(Tone::Danger))
                .width(Length::Fill),
            button("Close", ButtonKind::Secondary, Some(close)),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center),
    )
}

fn name_of(path: &std::path::Path) -> String {
    super::folder_name(path)
}
