use super::backups::when;
use super::units;
use crate::context::Context;
use crate::icons;
use crate::screen::{Action, Confirm, Screen, Toast, ToastKind};
use crate::theme::{self, ButtonKind, Tone, layout, space};
use crate::widgets::{
    banner, button, divider, hint, icon_button, icon_toned, label, muted, page, rows, section,
    setting_row, strong, toggle,
};
use crate::work;
use iced::widget::{Column, Space, column, container, row};
use iced::{Alignment, Element, Length, Padding, Task};
use sc_core::backup::{Scope, SnapshotId};
use sc_core::dirs::AppDirs;
use sc_core::error::Error;
use sc_core::install::Channel;
use sc_core::starstrings::{self, Advice, Http, NextAction, Release, Status, UreqHttp};
use std::fmt;
use std::sync::Arc;

pub const SOURCE_URL: &str = "https://github.com/MrKraken/StarStrings";
const LIVE: &str = "LIVE";
const SUBTITLE: &str = "Community text with clearer item and mission names.";
const ABOUT: &str = "StarStrings is a community replacement for the game's text file. \
    It adds useful detail to item, component and mission names, such as size, grade and type.";
const WRITES_FILE: &str = "data/Localization/english/global.ini";
const SETS_LINE: &str = "g_language = english";
const FOREIGN_NOTICE: &str = "This channel already has a custom global.ini that star-config did not install. Installing replaces it. A snapshot is taken first.";
const READ_ONLY_HINT: &str = "Close the game to install or remove StarStrings.";

/// What went wrong, in the user's words, with the technical text kept for a muted line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub message: String,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Doing {
    Check,
    Install,
    Uninstall,
}

impl Failure {
    fn from_error(err: &Error, doing: Doing) -> Self {
        Self {
            message: plain_message(err, doing),
            detail: err.to_string(),
        }
    }
}

fn plain_message(err: &Error, doing: Doing) -> String {
    match (err, doing) {
        (Error::Network(_), _) => "Could not reach GitHub. Check your connection and try again.".into(),
        (Error::RollbackFailed { .. }, _) => "The change failed and putting the old files back failed too, so some files may have changed. The snapshot on the Backups screen holds the originals.".into(),
        (Error::GameRunning, _) => "Star Citizen is running, so nothing was changed. Close the game and try again.".into(),
        (Error::Invalid(_), Doing::Check) => "GitHub sent an answer star-config did not understand. Try again later.".into(),
        (Error::Invalid(_), Doing::Install) => "The download did not look like a StarStrings release, so nothing was changed.".into(),
        (Error::Invalid(_), Doing::Uninstall) => "StarStrings on this channel was not installed by star-config, so it cannot be removed here.".into(),
        (Error::ChangedOnDisk { .. }, _) => "A game file changed while this ran, so nothing was changed. Try again.".into(),
        (Error::Parse { line, .. }, _) => format!("user.cfg could not be read (line {line}), so nothing was changed. Fix that line and try again."),
        _ => "Could not write the game files. Check that the game folder can be written to, then try again.".into(),
    }
}

/// The downloaded release asset, held between the download and the install step.
#[derive(Clone)]
pub struct Download(Arc<Vec<u8>>);

impl fmt::Debug for Download {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Download({} bytes)", self.0.len())
    }
}

/// The game-file side of the screen, read off the UI thread.
#[derive(Debug, Clone)]
pub struct Survey {
    scope: Scope,
    status: Status,
    build: Option<String>,
    advice: Advice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Work {
    Checking,
    Downloading(u64),
    Installing,
    Removing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    Check,
    Install,
    Update,
    UpToDate,
}

#[derive(Debug, Clone)]
pub enum Message {
    Surveyed(Box<Survey>),
    Check,
    Checked(Result<Option<Release>, Failure>),
    Install,
    InstallConfirmed,
    Downloaded(Result<Download, Failure>),
    Installed(Result<(String, SnapshotId), Failure>),
    Uninstall,
    UninstallConfirmed,
    Uninstalled(Result<SnapshotId, Failure>),
    CheckOnStart(bool),
    OpenSource,
    Opened(Result<(), String>),
}

pub struct State {
    http: Arc<dyn Http>,
    started: bool,
    scope: Option<Scope>,
    survey: Option<Survey>,
    latest: Option<Option<Release>>,
    work: Option<Work>,
    failure: Option<Failure>,
    notices: Vec<String>,
}

impl State {
    pub fn new() -> Self {
        Self::with_http(Arc::new(UreqHttp::new()))
    }

    pub fn with_http(http: Arc<dyn Http>) -> Self {
        Self {
            http,
            started: false,
            scope: None,
            survey: None,
            latest: None,
            work: None,
            failure: None,
            notices: Vec::new(),
        }
    }

    pub fn reload(&mut self, ctx: &Context) -> Task<Message> {
        let first = !self.started;
        self.started = true;
        if ctx.scope() != self.scope {
            self.scope = ctx.scope();
            self.survey = None;
            self.failure = None;
            self.notices.clear();
        }
        let survey = self.survey_task(ctx);
        if first && ctx.settings.starstrings.check_on_start && self.scope.is_some() {
            Task::batch([survey, self.start_check()])
        } else {
            survey
        }
    }

    pub fn update(&mut self, message: Message, ctx: &Context) -> Action<Message> {
        match message {
            Message::Surveyed(survey) => {
                if Some(&survey.scope) == ctx.scope().as_ref() {
                    self.survey = Some(*survey);
                    self.refresh_notices(ctx);
                }
                Action::none()
            }
            Message::Check => {
                if self.work.is_some() {
                    return Action::none();
                }
                Action::task(self.start_check())
            }
            Message::Checked(result) => {
                self.work = None;
                match result {
                    Ok(release) => {
                        self.latest = Some(release);
                        Action::task(self.survey_task(ctx))
                    }
                    Err(failure) => {
                        self.failure = Some(failure);
                        Action::none()
                    }
                }
            }
            Message::Install => self.ask_install(ctx),
            Message::InstallConfirmed => self.start_download(ctx),
            Message::Downloaded(Ok(download)) => self.start_install(download, ctx),
            Message::Downloaded(Err(failure)) | Message::Installed(Err(failure)) => {
                self.finish_with_failure(failure, ctx)
            }
            Message::Installed(Ok((name, snapshot))) => {
                self.work = None;
                Action::task(self.survey_task(ctx)).with_toast(
                    Toast::success(format!("Installed StarStrings {name}")).with_undo(snapshot),
                )
            }
            Message::Uninstall => self.ask_uninstall(ctx),
            Message::UninstallConfirmed => self.start_uninstall(ctx),
            Message::Uninstalled(Ok(snapshot)) => {
                self.work = None;
                Action::task(self.survey_task(ctx))
                    .with_toast(Toast::success("Removed StarStrings").with_undo(snapshot))
            }
            Message::Uninstalled(Err(failure)) => self.finish_with_failure(failure, ctx),
            Message::CheckOnStart(on) => {
                let mut settings = ctx.settings.clone();
                settings.starstrings.check_on_start = on;
                Action::settings(settings)
            }
            Message::OpenSource => Action::task(work::background(
                || open::that_detached(SOURCE_URL).map_err(|err| err.to_string()),
                Message::Opened,
            )),
            Message::Opened(Ok(())) => Action::none(),
            Message::Opened(Err(err)) => Action::toast(Toast::error(format!(
                "Could not open the browser. The address is {SOURCE_URL} ({err})"
            ))),
        }
    }

    pub fn view<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let mut body = Column::new().spacing(space::XL);
        if !self.notices.is_empty() {
            body = body.push(
                Column::with_children(
                    self.notices
                        .iter()
                        .map(|notice| banner(ToastKind::Warning, notice, None)),
                )
                .spacing(space::SM),
            );
        }
        body = body
            .push(self.status_card(ctx))
            .push(self.options(ctx))
            .push(details());
        page(
            Screen::StarStrings.label(),
            Some(SUBTITLE),
            Vec::new(),
            body,
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

    fn survey_task(&self, ctx: &Context) -> Task<Message> {
        let (Some(channel), Some(scope)) = (ctx.channel.clone(), ctx.scope()) else {
            return Task::none();
        };
        let dirs = ctx.dirs.clone();
        let latest = self.latest.clone().flatten();
        work::background(
            move || survey(channel, dirs, scope, latest),
            |survey| Message::Surveyed(Box::new(survey)),
        )
    }

    fn start_check(&mut self) -> Task<Message> {
        self.work = Some(Work::Checking);
        self.failure = None;
        let http = self.http.clone();
        work::background(
            move || {
                starstrings::latest(http.as_ref())
                    .map_err(|err| Failure::from_error(&err, Doing::Check))
            },
            Message::Checked,
        )
    }

    /// The latest release, once a check has found one.
    pub fn release(&self) -> Option<&Release> {
        self.latest.as_ref().and_then(Option::as_ref)
    }

    fn can_change(&self, ctx: &Context) -> bool {
        self.work.is_none() && !ctx.read_only() && ctx.scope().is_some()
    }

    fn ask_install(&mut self, ctx: &Context) -> Action<Message> {
        if !self.can_change(ctx) || self.release().is_none() {
            return Action::none();
        }
        let channel = channel_name(ctx);
        if channel.eq_ignore_ascii_case(LIVE) {
            return self.start_download(ctx);
        }
        Action::confirm(
            Confirm::new(
                format!("Install StarStrings on {channel}?"),
                not_live_notice(channel),
            )
            .action(
                "Install anyway",
                ButtonKind::Primary,
                Message::InstallConfirmed,
            )
            .cancel("Cancel"),
        )
    }

    fn start_download(&mut self, ctx: &Context) -> Action<Message> {
        let Some(release) = self.release().cloned() else {
            return Action::none();
        };
        if !self.can_change(ctx) {
            return Action::none();
        }
        self.work = Some(Work::Downloading(release.asset_size));
        self.failure = None;
        let http = self.http.clone();
        Action::task(work::background(
            move || {
                http.get_bytes(&release.asset_url, starstrings::MAX_BYTES)
                    .map(|bytes| Download(Arc::new(bytes)))
                    .map_err(|err| Failure::from_error(&err, Doing::Install))
            },
            Message::Downloaded,
        ))
    }

    fn start_install(&mut self, download: Download, ctx: &Context) -> Action<Message> {
        let (Some(release), Some(channel), Some(scope)) =
            (self.release().cloned(), ctx.channel.clone(), ctx.scope())
        else {
            self.work = None;
            return Action::none();
        };
        self.work = Some(Work::Installing);
        let writer = ctx.writer();
        let dirs = ctx.dirs.clone();
        Action::task(work::background(
            move || {
                let fetched = Prefetched {
                    url: release.asset_url.clone(),
                    bytes: download.0,
                };
                starstrings::install(
                    &fetched,
                    &release,
                    &writer,
                    &scope,
                    &channel,
                    &dirs,
                    starstrings::MIN_LINES,
                )
                .map(|snapshot| (release.name.clone(), snapshot.id))
                .map_err(|err| Failure::from_error(&err, Doing::Install))
            },
            Message::Installed,
        ))
    }

    fn ask_uninstall(&mut self, ctx: &Context) -> Action<Message> {
        if !self.can_change(ctx) || !self.installed() {
            return Action::none();
        }
        Action::confirm(
            Confirm::new(
                format!("Remove StarStrings from {}?", channel_name(ctx)),
                "The game goes back to its own text.",
            )
            .action("Uninstall", ButtonKind::Danger, Message::UninstallConfirmed)
            .cancel("Cancel"),
        )
    }

    fn start_uninstall(&mut self, ctx: &Context) -> Action<Message> {
        let (Some(channel), Some(scope)) = (ctx.channel.clone(), ctx.scope()) else {
            return Action::none();
        };
        if !self.can_change(ctx) {
            return Action::none();
        }
        self.work = Some(Work::Removing);
        self.failure = None;
        let writer = ctx.writer();
        let dirs = ctx.dirs.clone();
        Action::task(work::background(
            move || {
                starstrings::uninstall(&writer, &scope, &channel, &dirs)
                    .map(|snapshot| snapshot.id)
                    .map_err(|err| Failure::from_error(&err, Doing::Uninstall))
            },
            Message::Uninstalled,
        ))
    }

    fn finish_with_failure(&mut self, failure: Failure, ctx: &Context) -> Action<Message> {
        self.work = None;
        self.failure = Some(failure);
        Action::task(self.survey_task(ctx))
    }

    fn record(&self) -> Option<&starstrings::InstallRecord> {
        match self.survey.as_ref().map(|survey| &survey.status) {
            Some(Status::Managed(record) | Status::Modified(record)) => Some(record),
            _ => None,
        }
    }

    fn installed(&self) -> bool {
        self.record().is_some()
    }

    fn next(&self) -> Next {
        let action = self.survey.as_ref().map(|survey| survey.advice.action);
        match (self.release(), action) {
            (Some(_), Some(NextAction::Install)) => Next::Install,
            (Some(_), Some(NextAction::Update)) => Next::Update,
            (Some(_), Some(NextAction::UpToDate)) => Next::UpToDate,
            _ => Next::Check,
        }
    }

    fn refresh_notices(&mut self, ctx: &Context) {
        self.notices = self.notices_for(channel_name(ctx));
    }

    fn notices_for(&self, channel: &str) -> Vec<String> {
        let mut notices = Vec::new();
        let Some(survey) = &self.survey else {
            return notices;
        };
        if survey.advice.build_changed {
            let was = self
                .record()
                .and_then(|record| record.game_build.as_deref())
                .map_or("unknown", short_build);
            let now = survey.build.as_deref().map_or("unknown", short_build);
            notices.push(format!(
                "The game was updated since StarStrings was installed (build {now}, was {was}). Update StarStrings so its text matches the game."
            ));
        }
        if !channel.eq_ignore_ascii_case(LIVE) {
            notices.push(not_live_notice(channel));
        }
        if survey.status == Status::Foreign {
            notices.push(FOREIGN_NOTICE.to_string());
        }
        notices
    }

    fn status_card<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let mut card = column![
            row![
                self.installed_column(),
                self.latest_column(),
                self.build_column(ctx)
            ]
            .spacing(space::XL),
            divider(),
            self.action_row(ctx),
        ]
        .spacing(space::LG);
        if let Some(failure) = &self.failure {
            card = card.push(failure_block(failure));
        }
        container(card)
            .padding(space::LG)
            .width(Length::Fill)
            .style(theme::card)
            .into()
    }

    fn installed_column(&self) -> Element<'_, Message> {
        let (value, note) = match self.survey.as_ref().map(|survey| &survey.status) {
            None => ("Reading...".to_string(), None),
            Some(Status::Managed(record)) => (
                record.release_name.clone(),
                Some(format!(
                    "Installed {}",
                    when::listed_inline(record.installed_unix)
                )),
            ),
            Some(Status::Modified(record)) => (
                record.release_name.clone(),
                Some(format!(
                    "Installed {}, edited since",
                    when::listed_inline(record.installed_unix)
                )),
            ),
            Some(Status::Foreign) => (
                "Not installed".to_string(),
                Some("A custom global.ini is in place".to_string()),
            ),
            Some(Status::NotInstalled) => (
                "Not installed".to_string(),
                Some("The game uses its own text".to_string()),
            ),
        };
        figure("Installed", value, note)
    }

    fn latest_column(&self) -> Element<'_, Message> {
        let (value, note) = match (&self.latest, self.work) {
            (_, Some(Work::Checking)) => {
                ("Checking...".to_string(), Some("Asking GitHub".to_string()))
            }
            (None, _) => (
                "Not checked yet".to_string(),
                Some("Releases come from GitHub".to_string()),
            ),
            (Some(None), _) => (
                "No release found".to_string(),
                Some("GitHub lists no StarStrings download right now".to_string()),
            ),
            (Some(Some(release)), _) => (
                release.name.clone(),
                Some(format!(
                    "Published {}, {}",
                    when::iso_day(&release.published_at, when::now()),
                    units::bytes(release.asset_size)
                )),
            ),
        };
        figure("Latest", value, note)
    }

    fn build_column<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let value = self
            .survey
            .as_ref()
            .and_then(|survey| survey.build.clone())
            .unwrap_or_else(|| "Unknown".to_string());
        figure(
            "Game build",
            value,
            Some(format!("{} channel", channel_name(ctx))),
        )
    }

    fn action_row<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        if let Some(work) = self.work {
            return row![
                icon_toned(icons::INFO, layout::ICON, Tone::Muted),
                muted(work_text(work)),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center)
            .height(layout::CONTROL_HEIGHT)
            .into();
        }
        let can_change = self.can_change(ctx);
        let mut line = row![self.primary_button(can_change)]
            .spacing(space::SM)
            .align_y(Alignment::Center);
        if self.latest.is_some() && self.next() != Next::Check {
            line = line.push(button(
                "Check again",
                ButtonKind::Secondary,
                Some(Message::Check),
            ));
        }
        if ctx.read_only() {
            line = line.push(container(hint(READ_ONLY_HINT)).padding(Padding {
                left: space::SM,
                ..Padding::ZERO
            }));
        }
        line = line.push(Space::new().width(Length::Fill));
        if self.installed() {
            line = line.push(button(
                "Uninstall",
                ButtonKind::DangerGhost,
                can_change.then_some(Message::Uninstall),
            ));
        }
        line.into()
    }

    fn primary_button(&self, can_change: bool) -> Element<'_, Message> {
        match self.next() {
            Next::Check => {
                let label = if self.latest.is_some() {
                    "Check again"
                } else {
                    "Check for updates"
                };
                button(label, ButtonKind::Primary, Some(Message::Check))
            }
            Next::Install => button(
                "Install",
                ButtonKind::Primary,
                can_change.then_some(Message::Install),
            ),
            Next::Update => button(
                "Update",
                ButtonKind::Primary,
                can_change.then_some(Message::Install),
            ),
            Next::UpToDate => icon_button(icons::CHECK, "Up to date", ButtonKind::Primary, None),
        }
    }

    fn options<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        section(
            "Options",
            setting_row(
                "Check when star-config starts",
                Some("Looks for a new release each time star-config opens."),
                false,
                toggle(
                    ctx.settings.starstrings.check_on_start,
                    Message::CheckOnStart,
                ),
            ),
        )
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

/// Serves the asset that was already downloaded, so the install step does no network work.
struct Prefetched {
    url: String,
    bytes: Arc<Vec<u8>>,
}

impl Http for Prefetched {
    fn get_text(&self, url: &str) -> sc_core::error::Result<Option<String>> {
        Err(Error::Network(format!("unexpected request for {url}")))
    }

    fn get_bytes(&self, url: &str, _max_bytes: u64) -> sc_core::error::Result<Vec<u8>> {
        if url == self.url {
            Ok(self.bytes.as_ref().clone())
        } else {
            Err(Error::Network(format!("unexpected request for {url}")))
        }
    }
}

fn survey(channel: Channel, dirs: AppDirs, scope: Scope, latest: Option<Release>) -> Survey {
    let status = starstrings::status(&channel, &dirs, &scope);
    let advice = starstrings::advise(&status, latest.as_ref(), &channel);
    Survey {
        build: channel.build_id(),
        scope,
        status,
        advice,
    }
}

fn channel_name(ctx: &Context) -> &str {
    ctx.channel
        .as_ref()
        .map_or(LIVE, |channel| channel.name.as_str())
}

fn not_live_notice(channel: &str) -> String {
    format!("StarStrings is built for LIVE. On {channel} some text can be wrong or missing.")
}

/// The change number of a build id such as `4.3.2 (9876543)`, or the id itself.
fn short_build(build: &str) -> &str {
    build
        .rsplit_once(" (")
        .and_then(|(_, rest)| rest.strip_suffix(')'))
        .unwrap_or(build)
}

fn work_text(work: Work) -> String {
    match work {
        Work::Checking => "Checking the latest release...".to_string(),
        Work::Downloading(size) => format!("Downloading ({})...", units::bytes(size)),
        Work::Installing => "Installing...".to_string(),
        Work::Removing => "Removing StarStrings...".to_string(),
    }
}

fn figure<'a>(caption: &'a str, value: String, note: Option<String>) -> Element<'a, Message> {
    column![hint(caption), strong(value), hint(note.unwrap_or_default())]
        .spacing(space::XS)
        .width(Length::Fill)
        .into()
}

fn failure_block(failure: &Failure) -> Element<'_, Message> {
    column![
        banner(ToastKind::Error, &failure.message, None),
        container(hint(failure.detail.as_str())).padding(Padding {
            left: space::LG,
            ..Padding::ZERO
        }),
    ]
    .spacing(space::XS)
    .into()
}

fn details<'a>() -> Element<'a, Message> {
    section(
        "About StarStrings",
        rows([
            container(label(ABOUT))
                .padding(Padding {
                    top: space::MD,
                    right: space::LG,
                    bottom: space::MD,
                    left: space::LG,
                })
                .width(Length::Fill)
                .into(),
            setting_row(
                "Source",
                Some(SOURCE_URL),
                false,
                button(
                    "Open in browser",
                    ButtonKind::Secondary,
                    Some(Message::OpenSource),
                ),
            ),
            setting_row("File it writes", Some(WRITES_FILE), false, Space::new()),
            setting_row(
                "Line it sets in user.cfg",
                Some(SETS_LINE),
                false,
                Space::new(),
            ),
        ]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_build_takes_the_change_number() {
        assert_eq!(short_build("4.3.2 (9876543)"), "9876543");
        assert_eq!(short_build("9812345"), "9812345");
    }

    #[test]
    fn plain_messages_never_show_the_technical_text() {
        let network = Error::Network("dns error: failed to lookup".into());
        let failure = Failure::from_error(&network, Doing::Check);
        assert_eq!(
            failure.message,
            "Could not reach GitHub. Check your connection and try again."
        );
        assert!(failure.detail.contains("dns error"));
        let invalid = Error::Invalid("not a valid zip".into());
        assert_eq!(
            Failure::from_error(&invalid, Doing::Install).message,
            "The download did not look like a StarStrings release, so nothing was changed."
        );
    }
}
