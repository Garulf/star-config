mod facts;

use crate::app::game_status;
use crate::context::Context;
use crate::icons;
use crate::screen::{Action, Screen};
use crate::theme::{self, ButtonKind, Tone, layout, size, space};
use crate::widgets::{button, hint, icon_toned, page, status_pill, strong};
use crate::work;
use facts::{Card, Facts, Input};
use iced::widget::{Column, Row, Space, column, container, grid, image, row, text};
use iced::{Alignment, ContentFit, Element, Length, Task};
use sc_core::starstrings::Release;

const SUBTITLE: &str = "Your install and channel at a glance.";
pub const LOADING: &str = "Reading files";
pub const UNREADABLE: &str = "Could not read this";

/// One card on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    StarStrings,
    Config,
    Keybinds,
    Comms,
    Screenshots,
    Backups,
    Shaders,
    Channels,
}

impl Area {
    pub const ALL: [Area; 8] = [
        Area::StarStrings,
        Area::Config,
        Area::Keybinds,
        Area::Comms,
        Area::Screenshots,
        Area::Backups,
        Area::Shaders,
        Area::Channels,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Area::Shaders => "Shader caches",
            other => other.screen().label(),
        }
    }

    pub fn screen(self) -> Screen {
        match self {
            Area::StarStrings => Screen::StarStrings,
            Area::Config => Screen::Config,
            Area::Keybinds => Screen::Keybinds,
            Area::Comms => Screen::Comms,
            Area::Screenshots => Screen::Screenshots,
            Area::Backups => Screen::Backups,
            Area::Shaders => Screen::Maintenance,
            Area::Channels => Screen::Channels,
        }
    }

    pub fn button(self) -> &'static str {
        match self {
            Area::StarStrings => "Open StarStrings",
            Area::Config => "Edit config",
            Area::Keybinds => "Edit keybinds",
            Area::Comms => "Open settings",
            Area::Screenshots => "Open screenshots",
            Area::Backups => "Open backups",
            Area::Shaders => "Open maintenance",
            Area::Channels => "Open channels",
        }
    }
}

/// What a card shows right now.
#[derive(Debug, Clone)]
pub enum Shown {
    Loading,
    Ready(Card),
    Failed(String),
}

impl Shown {
    fn attention(&self) -> bool {
        matches!(self, Shown::Ready(card) if card.attention)
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(u64, Box<Facts>),
    Open(Screen),
}

pub struct State {
    seq: u64,
    facts: Option<Facts>,
    release: Option<Release>,
    subtitle: Option<String>,
}

impl State {
    pub fn new() -> Self {
        Self {
            seq: 0,
            facts: None,
            release: None,
            subtitle: None,
        }
    }

    pub fn reload(&mut self, ctx: &Context) -> Task<Message> {
        self.seq += 1;
        let (Some(install), Some(channel), Some(scope)) =
            (ctx.install.clone(), ctx.channel.clone(), ctx.scope())
        else {
            self.facts = None;
            self.subtitle = None;
            return Task::none();
        };
        if self.facts.as_ref().map(|facts| &facts.channel) != Some(&channel.dir) {
            self.facts = None;
            self.subtitle = None;
        }
        let input = Input {
            install,
            channels: ctx.channels.clone(),
            channel,
            scope,
            dirs: ctx.dirs.clone(),
            archive: ctx.settings.screenshots.archive_dir.clone(),
            local_appdata: super::maintenance::local_appdata(),
        };
        let seq = self.seq;
        work::background(
            move || facts::load(input),
            move |facts| Message::Loaded(seq, Box::new(facts)),
        )
    }

    /// The latest StarStrings release, as the StarStrings screen last checked it.
    pub fn set_release(&mut self, release: Option<Release>) {
        self.release = release;
    }

    pub fn update(&mut self, message: Message, ctx: &Context) -> Action<Message> {
        match message {
            Message::Loaded(seq, facts) => {
                let current = ctx.channel.as_ref().map(|channel| &channel.dir);
                if seq == self.seq && current == Some(&facts.channel) {
                    self.subtitle = Some(subtitle(ctx, facts.build.as_deref()));
                    self.facts = Some(*facts);
                }
                Action::none()
            }
            Message::Open(screen) => Action::goto(screen),
        }
    }

    /// Every card with what it shows, the ones needing attention first.
    pub fn cards(&self) -> Vec<(Area, Shown)> {
        let mut cards: Vec<(Area, Shown)> = Area::ALL
            .iter()
            .map(|area| (*area, self.shown(*area)))
            .collect();
        cards.sort_by_key(|(_, shown)| !shown.attention());
        cards
    }

    fn shown(&self, area: Area) -> Shown {
        let Some(facts) = &self.facts else {
            return Shown::Loading;
        };
        if area == Area::StarStrings {
            return Shown::Ready(facts::starstrings_card(
                &facts.starstrings,
                self.release.as_ref(),
            ));
        }
        match facts.cards.iter().find(|(card, _)| *card == area) {
            Some((_, Ok(card))) => Shown::Ready(card.clone()),
            Some((_, Err(reason))) => Shown::Failed(reason.clone()),
            None => Shown::Loading,
        }
    }

    pub fn view<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let (status, tone) = game_status(ctx.game, ctx.launcher);
        let cards = grid(
            self.cards()
                .into_iter()
                .map(|(area, shown)| card_view(area, shown)),
        )
        .fluid(layout::OVERVIEW_CARD_MAX_WIDTH)
        .spacing(space::LG)
        .height(Length::Shrink);
        page(
            Screen::Overview.label(),
            Some(self.subtitle.as_deref().unwrap_or(SUBTITLE)),
            vec![status_pill(status, tone)],
            cards,
        )
    }

    pub fn pending(&self) -> usize {
        0
    }

    pub fn apply(&mut self, _ctx: &Context) -> Action<Message> {
        Action::none()
    }

    pub fn discard(&mut self) {}

    pub fn undo(&mut self, _ctx: &Context) -> Action<Message> {
        Action::none()
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

/// `~/Games/star-citizen, LIVE 4.3.2 (9876543)`.
fn subtitle(ctx: &Context, build: Option<&str>) -> String {
    let install = ctx
        .install
        .as_ref()
        .map(|install| ctx.install_label(install))
        .unwrap_or_default();
    let channel = ctx
        .channel
        .as_ref()
        .map(|channel| channel.name.as_str())
        .unwrap_or_default();
    match build {
        Some(build) => format!("{install}, {channel} {build}"),
        None => format!("{install}, {channel}, build unknown"),
    }
}

fn card_view<'a>(area: Area, shown: Shown) -> Element<'a, Message> {
    let attention = shown.attention();
    let mut top = Row::new()
        .spacing(space::SM)
        .align_y(Alignment::Center)
        .push(icon_toned(
            area_icon(area),
            layout::ICON,
            if attention {
                Tone::Warning
            } else {
                Tone::Muted
            },
        ))
        .push(strong(area.title()).width(Length::Fill));
    if attention {
        top = top.push(icon_toned(icons::WARNING, layout::ICON, Tone::Warning));
    }
    let body = column![top, card_body(shown)].spacing(space::MD);
    container(
        column![
            body,
            Space::new().height(Length::Fill),
            button(
                area.button(),
                ButtonKind::Secondary,
                Some(Message::Open(area.screen()))
            ),
        ]
        .height(Length::Fill),
    )
    .padding(space::LG)
    .width(Length::Fill)
    .height(layout::OVERVIEW_CARD_HEIGHT)
    .style(if attention {
        theme::card_attention
    } else {
        theme::card
    })
    .into()
}

fn area_icon(area: Area) -> &'static [u8] {
    area.screen().icon()
}

fn figure<'a>(content: String) -> Element<'a, Message> {
    text(content)
        .size(size::TITLE)
        .font(theme::FONT_SEMIBOLD)
        .into()
}

fn card_body<'a>(shown: Shown) -> Element<'a, Message> {
    match shown {
        Shown::Loading => hint(LOADING).into(),
        Shown::Failed(reason) => column![strong(UNREADABLE), hint(reason)]
            .spacing(space::XS)
            .into(),
        Shown::Ready(card) => {
            let lines = Column::with_children(card.lines.into_iter().map(|line| hint(line).into()))
                .spacing(space::XS / 2.0);
            let mut body = column![figure(card.figure), lines].spacing(space::XS);
            if !card.thumbs.is_empty() {
                body = body
                    .push(Space::new().height(space::XS))
                    .push(thumb_strip(card.thumbs));
            }
            body.into()
        }
    }
}

fn thumb_strip<'a>(thumbs: Vec<iced::widget::image::Handle>) -> Element<'a, Message> {
    row(thumbs.into_iter().map(|handle| {
        container(
            image(handle)
                .width(Length::Fill)
                .height(layout::OVERVIEW_THUMB_HEIGHT)
                .content_fit(ContentFit::Cover),
        )
        .width(Length::FillPortion(1))
        .style(theme::sunken)
        .into()
    }))
    .spacing(space::SM)
    .into()
}
