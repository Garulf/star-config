use crate::context::{Context, discover_with};
use crate::screen::{Action, Confirm, Screen, Toast, ToastKind, UndoIn};
use crate::screens::{
    backups, channels, comms, config, keybinds, maintenance, overview, screenshots, settings,
    starstrings,
};
use crate::theme::{
    self, ButtonKind, FONT_SEMIBOLD, SIDEBAR_WIDTH, TOPBAR_HEIGHT, Tone, layout, size, space,
};
use crate::widgets::{
    self, MAX_TOASTS, SEARCH_ID, banner, confirm, empty_state, icon_button, nav_item, pick_list,
    segmented, status_pill, toast_stack,
};
use crate::{icons, work};
use iced::keyboard::{self, Key, key};
use iced::widget::{Column, Space, column, container, operation, row, stack, text};
use iced::{Alignment, Element, Length, Padding, Subscription, Task, Theme, time};
use iced::{Event, event, mouse, window};
use sc_core::backup::{Scope, SnapshotId};
use sc_core::install::{Channel, Install};
use sc_core::launcher::{self, LaunchMethod};
use sc_core::process::RunState;
use sc_core::starstrings::Http;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const PROBE_EVERY: Duration = Duration::from_secs(3);
const TOAST_LIFETIME: Duration = Duration::from_secs(6);
const TOAST_SWEEP: Duration = Duration::from_secs(1);

pub const READ_ONLY_RUNNING: &str = "Star Citizen is running. Files are read-only until it closes.";
pub const READ_ONLY_UNKNOWN: &str =
    "star-config cannot tell whether Star Citizen is running. Files are read-only.";
pub const UNLOCK_ACTION: &str = "It is not running, unlock";
pub const UNLOCK_BODY: &str = "If the game is running it may overwrite what star-config writes.";
pub const WINE_WARNING: &str =
    "This script closes every program running in this Wine prefix before it starts the launcher.";
pub const UNKNOWN_WARNING: &str =
    "star-config cannot tell whether the game is running. Launching will close it if it is.";
pub const NO_INSTALL_IN_FOLDER: &str = "That folder has no Star Citizen install";

/// A keyboard shortcut the app handles itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    Screen(Screen),
    Apply,
    Undo,
    Find,
    Escape,
    Enter,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstallChoice {
    index: usize,
    label: String,
}

impl fmt::Display for InstallChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Navigate(Screen),
    PickInstall(InstallChoice),
    PickChannel(String),
    SwitchInstall(usize),
    SwitchChannel(String),
    InstallLoaded(Option<Install>, Vec<Channel>),
    ChooseFolder,
    FolderPicked(Option<PathBuf>),
    FolderChecked(Option<Install>),
    Rescan,
    Rescanned(Vec<Install>),
    Launch,
    ApplyAndLaunch,
    LaunchUnchecked,
    LaunchConfirmed(String),
    Launched(Result<(), String>),
    LaunchDetected(Option<LaunchMethod>),
    ProbeTick,
    Probed(RunState, RunState),
    AskUnlock,
    Unlock,
    DialogAction(usize),
    CloseDialog,
    DismissToast(usize),
    UndoToast(usize),
    Restored(Result<(), String>),
    Recover(Scope, Channel, SnapshotId),
    KeepAsIs(Scope, SnapshotId),
    StopCopyAnd(Box<Message>),
    Kept(Result<(), String>),
    Pruned(Result<usize, String>),
    SweepToasts,
    SettingsSaved(Result<(), String>),
    Apply,
    Discard,
    Shortcut(Shortcut),
    /// A raw key or mouse event for the active screen while it captures input.
    Input(Event),
    Overview(overview::Message),
    Config(config::Message),
    Keybinds(keybinds::Message),
    Comms(comms::Message),
    StarStrings(starstrings::Message),
    Screenshots(screenshots::Message),
    Backups(backups::Message),
    Maintenance(maintenance::Message),
    Channels(channels::Message),
    Settings(settings::Message),
}

struct Screens {
    overview: overview::State,
    config: config::State,
    keybinds: keybinds::State,
    comms: comms::State,
    starstrings: starstrings::State,
    screenshots: screenshots::State,
    backups: backups::State,
    maintenance: maintenance::State,
    channels: channels::State,
    settings: settings::State,
}

/// Runs `$body` with `$state` bound to the screen's state and `$wrap` to its root message
/// constructor. `$($screens)+` is the borrow of the `Screens` value, `&self.screens` or
/// `&mut self.screens`.
macro_rules! on_screen {
    ($screen:expr, [$($screens:tt)+], |$state:ident, $wrap:ident| $body:expr) => {
        match $screen {
            Screen::Overview => { let $state = $($screens)+.overview; let $wrap = Message::Overview; $body }
            Screen::Config => { let $state = $($screens)+.config; let $wrap = Message::Config; $body }
            Screen::Keybinds => { let $state = $($screens)+.keybinds; let $wrap = Message::Keybinds; $body }
            Screen::Comms => { let $state = $($screens)+.comms; let $wrap = Message::Comms; $body }
            Screen::StarStrings => { let $state = $($screens)+.starstrings; let $wrap = Message::StarStrings; $body }
            Screen::Screenshots => { let $state = $($screens)+.screenshots; let $wrap = Message::Screenshots; $body }
            Screen::Backups => { let $state = $($screens)+.backups; let $wrap = Message::Backups; $body }
            Screen::Maintenance => { let $state = $($screens)+.maintenance; let $wrap = Message::Maintenance; $body }
            Screen::Channels => { let $state = $($screens)+.channels; let $wrap = Message::Channels; $body }
            Screen::Settings => { let $state = $($screens)+.settings; let $wrap = Message::Settings; $body }
        }
    };
}

pub struct App {
    ctx: Context,
    screen: Screen,
    screens: Screens,
    toasts: Vec<Toast>,
    toast_born: Vec<Instant>,
    dialog: Option<Confirm<Message>>,
    /// A startup question waiting for the one on screen to be answered.
    queued: Option<Confirm<Message>>,
    launch: Option<LaunchMethod>,
}

impl App {
    /// Builds the app around `ctx` without touching the system beyond launcher detection
    /// for the selected install.
    pub fn with(ctx: Context) -> Self {
        let launch = detect_launch(&ctx);
        let mut prompts = [recovery_prompt(&ctx), swap_prompt(&ctx)]
            .into_iter()
            .flatten();
        let dialog = prompts.next();
        let queued = prompts.next();
        Self {
            ctx,
            screen: Screen::Overview,
            screens: Screens {
                overview: overview::State::new(),
                config: config::State::new(),
                keybinds: keybinds::State::new(),
                comms: comms::State::new(),
                starstrings: starstrings::State::new(),
                screenshots: screenshots::State::new(),
                backups: backups::State::new(),
                maintenance: maintenance::State::new(),
                channels: channels::State::new(),
                settings: settings::State::new(),
            },
            toasts: Vec::new(),
            toast_born: Vec::new(),
            dialog,
            queued,
            launch,
        }
    }

    /// [`App::with`] with the StarStrings screen asking `http` instead of GitHub.
    pub fn with_http(ctx: Context, http: Arc<dyn Http>) -> Self {
        let mut app = Self::with(ctx);
        app.screens.starstrings = starstrings::State::with_http(http);
        app
    }

    /// [`App::with`] plus the first reload of every screen.
    pub fn boot(ctx: Context) -> (Self, Task<Message>) {
        let mut app = Self::with(ctx);
        let task = app.reload_all();
        (app, task)
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn context(&self) -> &Context {
        &self.ctx
    }

    pub fn toasts(&self) -> &[Toast] {
        &self.toasts
    }

    pub fn dialog(&self) -> Option<&Confirm<Message>> {
        self.dialog.as_ref()
    }

    pub fn pending(&self, screen: Screen) -> usize {
        on_screen!(screen, [&self.screens], |state, _wrap| state.pending())
    }

    pub fn title(&self) -> String {
        match &self.ctx.channel {
            Some(channel) => format!("star-config: {}", channel.name),
            None => "star-config".to_string(),
        }
    }

    pub fn theme(&self) -> Theme {
        theme::theme(self.ctx.settings.theme)
    }

    pub fn push_toast(&mut self, toast: Toast) {
        self.toasts.push(toast);
        self.toast_born.push(Instant::now());
        while self.toasts.len() > MAX_TOASTS {
            self.toasts.remove(0);
            self.toast_born.remove(0);
        }
    }

    fn remove_toast(&mut self, index: usize) -> Option<Toast> {
        if index >= self.toasts.len() {
            return None;
        }
        self.toast_born.remove(index);
        Some(self.toasts.remove(index))
    }

    fn total_pending(&self) -> usize {
        Screen::ALL.iter().map(|screen| self.pending(*screen)).sum()
    }

    fn reload(&mut self, screen: Screen) -> Task<Message> {
        let ctx = &self.ctx;
        on_screen!(screen, [&mut self.screens], |state, wrap| state
            .reload(ctx)
            .map(wrap))
    }

    fn reload_all(&mut self) -> Task<Message> {
        Task::batch(Screen::ALL.map(|screen| self.reload(screen)))
    }

    fn discard_all(&mut self) {
        for screen in Screen::ALL {
            on_screen!(screen, [&mut self.screens], |state, _wrap| state.discard());
        }
    }

    fn apply_active(&mut self) -> Task<Message> {
        let ctx = &self.ctx;
        let action = on_screen!(self.screen, [&mut self.screens], |state, wrap| state
            .apply(ctx)
            .map(wrap));
        self.handle(action)
    }

    fn handle(&mut self, action: Action<Message>) -> Task<Message> {
        let mut tasks = vec![action.task];
        if let Some(toast) = action.toast {
            if toast.undo.is_some() {
                tasks.push(self.prune());
            }
            self.push_toast(toast);
        }
        if let Some(confirm) = action.confirm {
            self.dialog = Some(confirm);
        }
        if let Some(settings) = action.settings {
            self.ctx.settings = settings;
            self.launch = detect_launch(&self.ctx);
            tasks.push(self.save_settings());
        }
        if action.rescan {
            tasks.push(self.rescan());
        }
        if let Some(screen) = action.goto {
            tasks.push(self.update(Message::Navigate(screen)));
        }
        Task::batch(tasks)
    }

    /// Deletes the oldest finished snapshots of the selected channel beyond the kept count.
    fn prune(&self) -> Task<Message> {
        let Some(scope) = self.ctx.scope() else {
            return Task::none();
        };
        let store = self.ctx.writer().store().clone();
        let keep = self.ctx.settings.snapshot_keep;
        work::background(
            move || {
                store
                    .prune(&scope, keep)
                    .map_err(|err| backups::words(&err))
            },
            Message::Pruned,
        )
    }

    fn save_settings(&self) -> Task<Message> {
        let settings = self.ctx.settings.clone();
        let path = self.ctx.dirs.settings_file();
        work::background(
            move || settings.save(&path).map_err(|err| err.to_string()),
            Message::SettingsSaved,
        )
    }

    fn rescan(&self) -> Task<Message> {
        let request = self.ctx.discovery_env();
        work::background(move || discover_with(request), Message::Rescanned)
    }

    fn load_install(install: Option<Install>) -> Task<Message> {
        work::background(
            move || {
                let channels = install.as_ref().map(Install::channels).unwrap_or_default();
                (install, channels)
            },
            |(install, channels)| Message::InstallLoaded(install, channels),
        )
    }

    fn confirm_discard(&mut self, then: Message) -> Task<Message> {
        if self.screens.channels.busy() {
            self.dialog = Some(
                Confirm::new(
                    "A copy is running. Stop it and switch?",
                    "The copy stops after the current file. Run Verify on the target in the launcher before you play it.",
                )
                .action(
                    "Stop and switch",
                    ButtonKind::Danger,
                    Message::StopCopyAnd(Box::new(then)),
                )
                .cancel("Keep copying"),
            );
            return Task::none();
        }
        if self.total_pending() == 0 {
            return self.update(then);
        }
        self.dialog = Some(
            Confirm::new(
                "Discard unsaved changes?",
                "Switching loads other files, so your unsaved changes will be lost.",
            )
            .action("Discard and switch", ButtonKind::Danger, then)
            .cancel("Cancel"),
        );
        Task::none()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Navigate(screen) => {
                self.screen = screen;
                if self.pending(screen) == 0 {
                    self.reload(screen)
                } else {
                    Task::none()
                }
            }
            Message::PickInstall(choice) => {
                self.confirm_discard(Message::SwitchInstall(choice.index))
            }
            Message::PickChannel(name) => {
                if self.ctx.channel.as_ref().is_some_and(|c| c.name == name) {
                    return Task::none();
                }
                self.confirm_discard(Message::SwitchChannel(name))
            }
            Message::SwitchInstall(index) => {
                self.discard_all();
                Self::load_install(self.ctx.installs.get(index).cloned())
            }
            Message::SwitchChannel(name) => {
                self.discard_all();
                if !self.ctx.set_channel(&name) {
                    return Task::none();
                }
                Task::batch([self.save_settings(), self.reload_all()])
            }
            Message::InstallLoaded(install, channels) => {
                self.ctx.set_install(install, channels);
                self.launch = detect_launch(&self.ctx);
                Task::batch([self.save_settings(), self.reload_all()])
            }
            Message::ChooseFolder => work::pick_folder(
                "Choose your StarCitizen folder or Wine prefix",
                Message::FolderPicked,
            ),
            Message::FolderPicked(None) => Task::none(),
            Message::FolderPicked(Some(path)) => {
                work::background(move || Install::from_dir(&path), Message::FolderChecked)
            }
            Message::FolderChecked(None) => {
                self.push_toast(Toast::error(NO_INSTALL_IN_FOLDER));
                Task::none()
            }
            Message::FolderChecked(Some(install)) => {
                if !self.ctx.settings.installs.contains(&install.root) {
                    self.ctx.settings.installs.push(install.root.clone());
                }
                if !self.ctx.installs.contains(&install) {
                    self.ctx.installs.push(install.clone());
                }
                self.discard_all();
                Self::load_install(Some(install))
            }
            Message::Rescan => self.rescan(),
            Message::Rescanned(installs) => {
                let keep = self
                    .ctx
                    .install
                    .clone()
                    .filter(|current| installs.contains(current))
                    .or_else(|| installs.first().cloned());
                self.ctx.installs = installs;
                Self::load_install(keep)
            }
            Message::Launch => self.launch_pressed(),
            Message::ApplyAndLaunch => {
                let applied = self.apply_active();
                applied.chain(Task::done(Message::LaunchUnchecked))
            }
            Message::LaunchUnchecked => self.launch_checked(),
            Message::LaunchConfirmed(described) => {
                if let Some(install) = &self.ctx.install {
                    self.ctx
                        .settings
                        .confirmed_launch
                        .insert(install.key(), described);
                }
                Task::batch([self.save_settings(), self.start_launch()])
            }
            Message::Launched(Ok(())) => {
                self.push_toast(Toast::info("Starting the RSI Launcher"));
                Task::none()
            }
            Message::Launched(Err(err)) => {
                self.push_toast(Toast::error(format!("Could not launch: {err}")));
                Task::none()
            }
            Message::LaunchDetected(method) => {
                self.launch = method;
                Task::none()
            }
            Message::ProbeTick => {
                let probe = self.ctx.probe();
                work::background(
                    move || (probe.game(), probe.launcher()),
                    |(game, launcher)| Message::Probed(game, launcher),
                )
            }
            Message::Probed(game, launcher) => {
                self.ctx.game = game;
                self.ctx.launcher = launcher;
                if game == RunState::NotRunning {
                    self.ctx.unlocked = false;
                }
                Task::none()
            }
            Message::AskUnlock => {
                self.dialog = Some(
                    Confirm::new("Unlock files?", UNLOCK_BODY)
                        .action("Unlock", ButtonKind::Danger, Message::Unlock)
                        .cancel("Cancel"),
                );
                Task::none()
            }
            Message::Unlock => {
                self.ctx.unlocked = true;
                Task::none()
            }
            Message::DialogAction(index) => {
                let Some(dialog) = self.dialog.take() else {
                    return Task::none();
                };
                let task = match dialog
                    .actions
                    .into_iter()
                    .nth(index)
                    .and_then(|a| a.message)
                {
                    Some(message) => self.update(message),
                    None => Task::none(),
                };
                self.show_queued();
                task
            }
            Message::CloseDialog => {
                self.dialog = None;
                self.show_queued();
                Task::none()
            }
            Message::StopCopyAnd(then) => {
                self.screens.channels.stop_copy();
                self.confirm_discard(*then)
            }
            Message::DismissToast(index) => {
                self.remove_toast(index);
                Task::none()
            }
            Message::UndoToast(index) => self.undo_toast(index),
            Message::Restored(result) => {
                let pruned = match result {
                    Ok(()) => {
                        self.push_toast(Toast::success("Restored the previous files"));
                        self.prune()
                    }
                    Err(err) => {
                        self.push_toast(Toast::error(err));
                        Task::none()
                    }
                };
                Task::batch([pruned, self.reload(self.screen)])
            }
            Message::Recover(scope, channel, snapshot) => {
                let writer = self.ctx.writer();
                work::background(
                    move || backups::restore(&writer, &scope, &channel, &snapshot).map(|_| ()),
                    Message::Restored,
                )
            }
            Message::KeepAsIs(scope, snapshot) => {
                let store = self.ctx.writer().store().clone();
                work::background(
                    move || {
                        store
                            .finish(&scope, &snapshot)
                            .map_err(|err| backups::words(&err))
                    },
                    Message::Kept,
                )
            }
            Message::Kept(Ok(())) => self.reload(Screen::Backups),
            Message::Kept(Err(err)) => {
                self.push_toast(Toast::error(format!(
                    "Could not mark the change as kept: {err}"
                )));
                Task::none()
            }
            Message::Pruned(Ok(0)) => Task::none(),
            Message::Pruned(Ok(_)) => self.reload(Screen::Backups),
            Message::Pruned(Err(err)) => {
                self.push_toast(Toast::error(format!(
                    "Could not delete old snapshots: {err}"
                )));
                Task::none()
            }
            Message::SweepToasts => {
                let now = Instant::now();
                let mut index = 0;
                while index < self.toasts.len() {
                    let old = now.duration_since(self.toast_born[index]) >= TOAST_LIFETIME;
                    if old && self.toasts[index].expires() {
                        self.remove_toast(index);
                    } else {
                        index += 1;
                    }
                }
                Task::none()
            }
            Message::SettingsSaved(Ok(())) => Task::none(),
            Message::SettingsSaved(Err(err)) => {
                self.push_toast(Toast::error(format!("Could not save settings: {err}")));
                Task::none()
            }
            Message::Apply => self.apply_active(),
            Message::Discard => {
                on_screen!(self.screen, [&mut self.screens], |state, _wrap| state
                    .discard());
                Task::none()
            }
            Message::Shortcut(shortcut) => self.shortcut(shortcut),
            Message::Input(event) => self.input(event),
            Message::Overview(m) => {
                let a = self.screens.overview.update(m, &self.ctx);
                self.handle(a.map(Message::Overview))
            }
            Message::Config(m) => {
                let a = self.screens.config.update(m, &self.ctx);
                self.handle(a.map(Message::Config))
            }
            Message::Keybinds(m) => {
                let a = self.screens.keybinds.update(m, &self.ctx);
                self.handle(a.map(Message::Keybinds))
            }
            Message::Comms(m) => {
                let a = self.screens.comms.update(m, &self.ctx);
                self.handle(a.map(Message::Comms))
            }
            Message::StarStrings(m) => {
                let a = self.screens.starstrings.update(m, &self.ctx);
                let release = self.screens.starstrings.release().cloned();
                self.screens.overview.set_release(release);
                self.handle(a.map(Message::StarStrings))
            }
            Message::Screenshots(m) => {
                let a = self.screens.screenshots.update(m, &self.ctx);
                self.handle(a.map(Message::Screenshots))
            }
            Message::Backups(m) => {
                let a = self.screens.backups.update(m, &self.ctx);
                self.handle(a.map(Message::Backups))
            }
            Message::Maintenance(m) => {
                let a = self.screens.maintenance.update(m, &self.ctx);
                self.handle(a.map(Message::Maintenance))
            }
            Message::Channels(m) => {
                let a = self.screens.channels.update(m, &self.ctx);
                self.handle(a.map(Message::Channels))
            }
            Message::Settings(m) => {
                let a = self.screens.settings.update(m, &self.ctx);
                self.handle(a.map(Message::Settings))
            }
        }
    }

    fn shortcut(&mut self, shortcut: Shortcut) -> Task<Message> {
        if self.dialog.is_some() {
            return match shortcut {
                Shortcut::Escape => self.update(Message::CloseDialog),
                Shortcut::Enter => match self.dialog.as_ref().and_then(Confirm::primary) {
                    Some(index) => self.update(Message::DialogAction(index)),
                    None => Task::none(),
                },
                _ => Task::none(),
            };
        }
        match shortcut {
            Shortcut::Screen(screen) => self.update(Message::Navigate(screen)),
            Shortcut::Apply if self.pending(self.screen) > 0 => self.apply_active(),
            Shortcut::Undo => {
                let ctx = &self.ctx;
                let action = on_screen!(self.screen, [&mut self.screens], |state, wrap| state
                    .undo(ctx)
                    .map(wrap));
                self.handle(action)
            }
            Shortcut::Find => operation::focus(SEARCH_ID),
            Shortcut::Apply | Shortcut::Escape | Shortcut::Enter => Task::none(),
        }
    }

    fn input(&mut self, event: Event) -> Task<Message> {
        if !self.wants_input() {
            return Task::none();
        }
        match self.screen {
            Screen::Keybinds => self.update(Message::Keybinds(keybinds::Message::Input(event))),
            Screen::Screenshots => self.viewer_input(event),
            _ => Task::none(),
        }
    }

    /// The screenshot viewer takes the arrow keys and Escape, and every other shortcut still works.
    fn viewer_input(&mut self, event: Event) -> Task<Message> {
        let shell_shortcut = match &event {
            Event::Keyboard(key_event) => shortcut(key_event),
            _ => None,
        };
        match shell_shortcut {
            Some(Shortcut::Escape | Shortcut::Enter) | None => {
                self.update(Message::Screenshots(screenshots::Message::Input(event)))
            }
            Some(other) => self.shortcut(other),
        }
    }

    fn launch_pressed(&mut self) -> Task<Message> {
        let pending = self.pending(self.screen);
        if pending == 0 {
            return self.launch_checked();
        }
        let body = format!(
            "{} has {}. Apply them before the game reads its files?",
            self.screen.label(),
            unsaved_text(pending)
        );
        self.dialog = Some(
            Confirm::new("Apply changes first?", body)
                .action(
                    "Apply and launch",
                    ButtonKind::Primary,
                    Message::ApplyAndLaunch,
                )
                .action(
                    "Launch without applying",
                    ButtonKind::Secondary,
                    Message::LaunchUnchecked,
                )
                .cancel("Cancel"),
        );
        Task::none()
    }

    fn launch_checked(&mut self) -> Task<Message> {
        let (Some(install), Some(method)) = (&self.ctx.install, &self.launch) else {
            return self.update(Message::Navigate(Screen::Settings));
        };
        let described = method.describe();
        let unknown = self.ctx.game == RunState::Unknown || self.ctx.launcher == RunState::Unknown;
        let closes = method.closes_wine_programs();
        let confirmed = self.ctx.settings.confirmed_launch.get(&install.key()) == Some(&described);
        if confirmed && !(unknown && closes) {
            return self.start_launch();
        }
        let mut body = format!("star-config will run:\n{described}");
        if closes {
            body.push_str("\n\n");
            body.push_str(WINE_WARNING);
        }
        if unknown && closes {
            body.push_str("\n\n");
            body.push_str(UNKNOWN_WARNING);
        }
        self.dialog = Some(
            Confirm::new("Start the launcher?", body)
                .action(
                    "Launch",
                    ButtonKind::Primary,
                    Message::LaunchConfirmed(described),
                )
                .cancel("Cancel"),
        );
        Task::none()
    }

    fn start_launch(&self) -> Task<Message> {
        let Some(method) = self.launch.clone() else {
            return Task::none();
        };
        work::background(
            move || launcher::launch(&method).map_err(|err| err.to_string()),
            Message::Launched,
        )
    }

    fn show_queued(&mut self) {
        if self.dialog.is_none() {
            self.dialog = self.queued.take();
        }
    }

    fn undo_toast(&mut self, index: usize) -> Task<Message> {
        let Some((undo, mut targets)) = self
            .toasts
            .get(index)
            .map(|toast| (toast.undo.clone(), toast.undo_in.clone()))
        else {
            return Task::none();
        };
        if undo.is_none() {
            return Task::none();
        }
        if self.ctx.read_only() {
            self.push_toast(Toast::error(backups::CLOSE_GAME));
            return Task::none();
        }
        if targets.is_empty() {
            let (Some(scope), Some(channel), Some(snapshot)) =
                (self.ctx.scope(), self.ctx.channel.clone(), undo)
            else {
                return Task::none();
            };
            targets.push(UndoIn {
                scope,
                channel,
                snapshot,
            });
        }
        self.remove_toast(index);
        let writer = self.ctx.writer();
        work::background(
            move || {
                targets.iter().rev().try_for_each(|target| {
                    backups::restore(&writer, &target.scope, &target.channel, &target.snapshot)
                        .map(|_| ())
                })
            },
            Message::Restored,
        )
    }

    /// Whether the active screen is taking raw keys and mouse buttons.
    pub fn wants_input(&self) -> bool {
        self.dialog.is_none()
            && match self.screen {
                Screen::Keybinds => self.screens.keybinds.wants_input(),
                Screen::Screenshots => self.screens.screenshots.wants_input(),
                _ => false,
            }
    }

    /// Where the event subscription sends keyboard and mouse events right now.
    pub fn event_handler(&self) -> fn(Event, event::Status, window::Id) -> Option<Message> {
        if self.wants_input() {
            captured_input
        } else {
            shortcut_event
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            time::every(PROBE_EVERY).map(|_| Message::ProbeTick),
            event::listen_with(self.event_handler()),
        ];
        if self.toasts.iter().any(Toast::expires) {
            subscriptions.push(time::every(TOAST_SWEEP).map(|_| Message::SweepToasts));
        }
        Subscription::batch(subscriptions)
    }

    pub fn view(&self) -> Element<'_, Message> {
        let shell = column![
            self.top_bar(),
            row![self.sidebar(), self.content()].height(Length::Fill),
        ];
        let base = container(shell)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::window);
        match &self.dialog {
            Some(dialog) => {
                let actions = dialog
                    .actions
                    .iter()
                    .enumerate()
                    .map(|(index, action)| {
                        (
                            action.label.as_str(),
                            action.kind,
                            Message::DialogAction(index),
                        )
                    })
                    .collect();
                stack![base, confirm(&dialog.title, &dialog.body, actions)].into()
            }
            None => base.into(),
        }
    }

    fn top_bar(&self) -> Element<'_, Message> {
        let brand = container(text("star-config").size(size::LABEL).font(FONT_SEMIBOLD))
            .padding(Padding {
                left: space::LG,
                ..Padding::ZERO
            })
            .width(SIDEBAR_WIDTH);
        let choices: Vec<InstallChoice> = self
            .ctx
            .installs
            .iter()
            .enumerate()
            .map(|(index, install)| InstallChoice {
                index,
                label: self.ctx.install_label(install),
            })
            .collect();
        let selected = self
            .ctx
            .install
            .as_ref()
            .and_then(|current| {
                choices
                    .iter()
                    .position(|c| &self.ctx.installs[c.index] == current)
            })
            .map(|position| choices[position].clone());
        let installs = pick_list(choices, selected, Message::PickInstall)
            .placeholder("No install found")
            .width(layout::INSTALL_PICK_WIDTH);
        let mut bar = row![brand, installs]
            .spacing(space::MD)
            .align_y(Alignment::Center);
        if !self.ctx.channels.is_empty() {
            let current = self.ctx.channel.as_ref().map(|c| c.name.as_str());
            bar = bar.push(segmented(
                self.ctx
                    .channels
                    .iter()
                    .map(|channel| {
                        (
                            channel.name.as_str(),
                            Some(channel.name.as_str()) == current,
                            Message::PickChannel(channel.name.clone()),
                        )
                    })
                    .collect(),
            ));
        }
        let (status, tone) = game_status(self.ctx.game, self.ctx.launcher);
        bar = bar
            .push(Space::new().width(Length::Fill))
            .push(status_pill(status, tone))
            .push(self.launch_button());
        container(bar)
            .height(TOPBAR_HEIGHT)
            .padding(Padding {
                right: space::LG,
                ..Padding::ZERO
            })
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .style(theme::topbar)
            .into()
    }

    fn launch_button(&self) -> Element<'_, Message> {
        let running = self.ctx.game == RunState::Running || self.ctx.launcher == RunState::Running;
        if running {
            icon_button(icons::LAUNCH, "Running", ButtonKind::Primary, None)
        } else if self.ctx.install.is_some() && self.launch.is_none() {
            widgets::button(
                "Set up launch",
                ButtonKind::Secondary,
                Some(Message::Navigate(Screen::Settings)),
            )
        } else {
            icon_button(
                icons::LAUNCH,
                "Launch",
                ButtonKind::Primary,
                self.ctx.install.as_ref().map(|_| Message::Launch),
            )
        }
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let item = |screen: Screen| {
            nav_item(
                screen.icon(),
                screen.label(),
                screen == self.screen,
                self.pending(screen),
                Message::Navigate(screen),
            )
        };
        let mut items = Column::new().spacing(space::XS / 2.0);
        for screen in &Screen::ALL[..Screen::ALL.len() - 1] {
            items = items.push(item(*screen));
        }
        container(
            column![
                items,
                Space::new().height(Length::Fill),
                item(Screen::Settings)
            ]
            .height(Length::Fill),
        )
        .padding(Padding {
            top: space::XS,
            right: space::SM,
            bottom: space::MD,
            left: space::SM,
        })
        .width(SIDEBAR_WIDTH)
        .height(Length::Fill)
        .style(theme::sidebar)
        .into()
    }

    fn content(&self) -> Element<'_, Message> {
        let mut column = Column::new().width(Length::Fill).height(Length::Fill);
        if self.ctx.read_only() {
            let text = if self.ctx.game == RunState::Unknown {
                READ_ONLY_UNKNOWN
            } else {
                READ_ONLY_RUNNING
            };
            column = column.push(
                container(banner(
                    ToastKind::Warning,
                    text,
                    Some((UNLOCK_ACTION, Message::AskUnlock)),
                ))
                .padding(Padding {
                    top: space::LG,
                    right: space::XL,
                    bottom: 0.0,
                    left: space::XL,
                }),
            );
        }
        let toasts = container(toast_stack(
            &self.toasts,
            Message::DismissToast,
            Message::UndoToast,
        ))
        .align_right(Length::Fill)
        .align_bottom(Length::Fill)
        .padding(space::LG);
        column = column.push(stack![self.screen_view(), toasts].height(Length::Fill));
        let pending = self.pending(self.screen);
        if pending > 0 {
            column = column.push(bottom_bar(pending));
        }
        container(column)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::panel)
            .into()
    }

    fn screen_view(&self) -> Element<'_, Message> {
        if self.screen.needs_channel() {
            if self.ctx.install.is_none() {
                return widgets::empty_state_icon(
                    icons::FOLDER,
                    "No Star Citizen install found",
                    "Point star-config at your StarCitizen folder or Wine prefix.",
                    Some(("Choose folder", Message::ChooseFolder)),
                );
            }
            if self.ctx.channel.is_none() {
                return empty_state(
                    "No channels in this install",
                    "The StarCitizen folder has no LIVE, PTU or other channel yet. Install one with the RSI Launcher, then rescan.",
                    Some(("Rescan", Message::Rescan)),
                );
            }
        }
        let ctx = &self.ctx;
        on_screen!(self.screen, [&self.screens], |state, wrap| state
            .view(ctx)
            .map(wrap))
    }
}

fn bottom_bar<'a>(pending: usize) -> Element<'a, Message> {
    container(
        row![
            widgets::change_marker(true),
            text(unsaved_text(pending))
                .size(size::BODY)
                .width(Length::Fill),
            widgets::button("Discard", ButtonKind::Secondary, Some(Message::Discard)),
            widgets::button("Apply", ButtonKind::Primary, Some(Message::Apply)),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center),
    )
    .height(layout::BOTTOMBAR_HEIGHT)
    .padding(Padding {
        top: 0.0,
        right: space::XL,
        bottom: 0.0,
        left: space::XL,
    })
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .style(theme::bottombar)
    .into()
}

pub fn unsaved_text(count: usize) -> String {
    if count == 1 {
        "1 unsaved change".to_string()
    } else {
        format!("{count} unsaved changes")
    }
}

pub fn game_status(game: RunState, launcher: RunState) -> (&'static str, Tone) {
    match (game, launcher) {
        (RunState::Running, _) => ("Game running", Tone::Warning),
        (RunState::Unknown, _) => ("Game status unknown", Tone::Muted),
        (RunState::NotRunning, RunState::Running) => ("Launcher running", Tone::Neutral),
        (RunState::NotRunning, _) => ("Game closed", Tone::Success),
    }
}

/// The question for the first interrupted change whose install and channel are still here.
fn recovery_prompt(ctx: &Context) -> Option<Confirm<Message>> {
    let interrupted = ctx.writer().store().interrupted().ok()?;
    interrupted.into_iter().find_map(|(scope, info)| {
        let channel = channel_of(ctx, &scope)?;
        let restore = Message::Recover(scope.clone(), channel, info.id.clone());
        let keep = Message::KeepAsIs(scope, info.id.clone());
        Some(backups::recovery_confirm(&info, restore, keep))
    })
}

/// The question for an unfinished channel swap of the selected install.
fn swap_prompt(ctx: &Context) -> Option<Confirm<Message>> {
    channels::startup_prompt(ctx).map(|confirm| confirm.map(Message::Channels))
}

fn channel_of(ctx: &Context, scope: &Scope) -> Option<Channel> {
    let install = ctx
        .installs
        .iter()
        .find(|install| install.key() == scope.install_key)?;
    let channels = if ctx.install.as_ref() == Some(install) {
        ctx.channels.clone()
    } else {
        install.channels()
    };
    channels
        .into_iter()
        .find(|channel| channel.name == scope.channel)
}

fn detect_launch(ctx: &Context) -> Option<LaunchMethod> {
    let install = ctx.install.as_ref()?;
    let saved = ctx.settings.launch_commands.get(&install.key());
    launcher::detect(install, saved.map(String::as_str))
}

fn shortcut_message(event: keyboard::Event) -> Option<Message> {
    shortcut(&event).map(Message::Shortcut)
}

fn shortcut_event(event: Event, status: event::Status, _window: window::Id) -> Option<Message> {
    match event {
        Event::Keyboard(key_event) if status == event::Status::Ignored => {
            shortcut_message(key_event)
        }
        _ => None,
    }
}

fn captured_input(event: Event, status: event::Status, _window: window::Id) -> Option<Message> {
    let wanted = match &event {
        Event::Keyboard(_) => status == event::Status::Ignored,
        Event::Mouse(mouse::Event::ButtonPressed(_) | mouse::Event::WheelScrolled { .. }) => true,
        _ => false,
    };
    wanted.then_some(Message::Input(event))
}

fn digit_of(key: &Key, physical: &key::Physical) -> Option<usize> {
    use key::Code;
    let from_code = match physical {
        key::Physical::Code(code) => match code {
            Code::Digit1 => Some(1),
            Code::Digit2 => Some(2),
            Code::Digit3 => Some(3),
            Code::Digit4 => Some(4),
            Code::Digit5 => Some(5),
            Code::Digit6 => Some(6),
            Code::Digit7 => Some(7),
            Code::Digit8 => Some(8),
            Code::Digit9 => Some(9),
            Code::Digit0 => Some(0),
            _ => None,
        },
        key::Physical::Unidentified(_) => None,
    };
    from_code.or_else(|| match key.as_ref() {
        Key::Character(c) => c.parse::<usize>().ok().filter(|digit| *digit < 10),
        _ => None,
    })
}

/// Maps a key press to the shortcut it triggers, if any.
pub fn shortcut(event: &keyboard::Event) -> Option<Shortcut> {
    let keyboard::Event::KeyPressed {
        key,
        physical_key,
        modifiers,
        ..
    } = event
    else {
        return None;
    };
    match key.as_ref() {
        Key::Named(key::Named::Escape) => return Some(Shortcut::Escape),
        Key::Named(key::Named::Enter) if modifiers.is_empty() => return Some(Shortcut::Enter),
        _ => {}
    }
    if !modifiers.command() || modifiers.alt() {
        return None;
    }
    if let Some(digit) = digit_of(key, physical_key) {
        let index = if digit == 0 { 9 } else { digit - 1 };
        return Some(Shortcut::Screen(Screen::ALL[index]));
    }
    match key.as_ref() {
        Key::Character(c) if c.eq_ignore_ascii_case("s") => Some(Shortcut::Apply),
        Key::Character(c) if c.eq_ignore_ascii_case("z") && !modifiers.shift() => {
            Some(Shortcut::Undo)
        }
        Key::Character(c) if c.eq_ignore_ascii_case("f") => Some(Shortcut::Find),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{self, ctrl};
    use sc_core::backup::SnapshotId;

    #[test]
    fn changing_channel_saves_selection_to_settings() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = testkit::context_with_channels(temp.path(), &["LIVE", "PTU"]);
        let mut app = App::with(ctx);
        let task = app.update(Message::PickChannel("PTU".into()));
        testkit::settle(&mut app, task);
        assert_eq!(app.context().channel.as_ref().unwrap().name, "PTU");
        let saved = std::fs::read_to_string(app.context().dirs.settings_file()).unwrap();
        assert!(saved.contains("selected_channel = \"PTU\""), "{saved}");
    }

    #[test]
    fn picking_invalid_folder_adds_error_toast() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = testkit::context_with_channels(temp.path(), &["LIVE"]);
        let empty = tempfile::tempdir().unwrap();
        let mut app = App::with(ctx);
        let task = app.update(Message::FolderPicked(Some(empty.path().to_path_buf())));
        testkit::settle(&mut app, task);
        assert_eq!(app.toasts().last().unwrap().text, NO_INSTALL_IN_FOLDER);
        assert_eq!(app.toasts().last().unwrap().kind, ToastKind::Error);
    }

    #[test]
    fn picking_valid_folder_adds_and_selects_it() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = testkit::context_without_install(temp.path());
        let other = tempfile::tempdir().unwrap();
        let root = testkit::make_install(other.path(), &["LIVE"]);
        let mut app = App::with(ctx);
        let task = app.update(Message::FolderPicked(Some(root.clone())));
        testkit::settle(&mut app, task);
        assert_eq!(app.context().install.as_ref().unwrap().root, root);
        assert!(app.context().settings.installs.contains(&root));
        assert_eq!(app.context().channel.as_ref().unwrap().name, "LIVE");
    }

    #[test]
    fn unlocked_resets_when_game_closes() {
        let temp = tempfile::tempdir().unwrap();
        let mut ctx = testkit::context_with_channels(temp.path(), &["LIVE"]);
        ctx.game = RunState::Running;
        let mut app = App::with(ctx);
        let _ = app.update(Message::Unlock);
        assert!(app.context().unlocked);
        let _ = app.update(Message::Probed(RunState::Running, RunState::NotRunning));
        assert!(app.context().unlocked);
        let _ = app.update(Message::Probed(RunState::NotRunning, RunState::NotRunning));
        assert!(!app.context().unlocked);
        assert!(!app.context().read_only());
    }

    #[test]
    fn at_most_three_toasts_are_kept() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::with(testkit::context_without_install(temp.path()));
        for index in 0..4 {
            app.push_toast(Toast::info(format!("toast {index}")));
        }
        let texts: Vec<_> = app.toasts().iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["toast 1", "toast 2", "toast 3"]);
    }

    #[test]
    fn errors_outlive_the_sweep_and_info_does_not() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::with(testkit::context_without_install(temp.path()));
        app.push_toast(Toast::error("bad"));
        app.push_toast(Toast::success("good").with_undo(SnapshotId("x".into())));
        for born in &mut app.toast_born {
            *born -= TOAST_LIFETIME;
        }
        let _ = app.update(Message::SweepToasts);
        assert_eq!(app.toasts().len(), 1);
        assert_eq!(app.toasts()[0].text, "bad");
    }

    #[test]
    fn shortcuts_map_to_screens_and_actions() {
        assert_eq!(
            shortcut(&ctrl("3")),
            Some(Shortcut::Screen(Screen::Keybinds))
        );
        assert_eq!(
            shortcut(&ctrl("0")),
            Some(Shortcut::Screen(Screen::Settings))
        );
        assert_eq!(shortcut(&ctrl("s")), Some(Shortcut::Apply));
        assert_eq!(shortcut(&ctrl("z")), Some(Shortcut::Undo));
        assert_eq!(shortcut(&ctrl("f")), Some(Shortcut::Find));
    }

    #[test]
    fn escape_closes_and_enter_picks_primary() {
        let temp = tempfile::tempdir().unwrap();
        let mut ctx = testkit::context_with_channels(temp.path(), &["LIVE"]);
        ctx.game = RunState::Running;
        let mut app = App::with(ctx);
        let _ = app.update(Message::AskUnlock);
        let _ = app.update(Message::Shortcut(Shortcut::Escape));
        assert!(app.dialog().is_none());
        let _ = app.update(Message::AskUnlock);
        let _ = app.update(Message::Shortcut(Shortcut::Enter));
        assert!(app.context().unlocked);
    }

    #[test]
    fn switching_channel_with_pending_edits_asks_first() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = testkit::context_with_channels(temp.path(), &["LIVE", "PTU"]);
        let mut app = App::with(ctx);
        let task = app.update(Message::Navigate(Screen::Settings));
        testkit::settle(&mut app, task);
        let _ = app.update(Message::Settings(settings::Message::Keep(80)));
        assert_eq!(app.pending(Screen::Settings), 1);
        let _ = app.update(Message::PickChannel("PTU".into()));
        let dialog = app.dialog().expect("confirm shown");
        assert_eq!(dialog.actions[0].label, "Discard and switch");
        assert_eq!(app.context().channel.as_ref().unwrap().name, "LIVE");
        let task = app.update(Message::DialogAction(0));
        testkit::settle(&mut app, task);
        assert_eq!(app.context().channel.as_ref().unwrap().name, "PTU");
        assert_eq!(app.pending(Screen::Settings), 0);
    }

    #[test]
    fn launch_with_pending_edits_offers_apply_first() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = testkit::context_with_channels(temp.path(), &["LIVE"]);
        let mut app = App::with(ctx);
        app.screen = Screen::Settings;
        let _ = app.update(Message::Settings(settings::Message::Keep(80)));
        let _ = app.update(Message::Launch);
        let labels: Vec<_> = app
            .dialog()
            .unwrap()
            .actions
            .iter()
            .map(|a| a.label.as_str())
            .collect();
        assert_eq!(
            labels,
            ["Apply and launch", "Launch without applying", "Cancel"]
        );
    }

    #[test]
    fn unknown_state_confirms_a_script_every_time() {
        let temp = tempfile::tempdir().unwrap();
        let mut ctx = testkit::context_with_wine_script(temp.path());
        let described = detect_launch(&ctx).unwrap().describe();
        let key = ctx.install.as_ref().unwrap().key();
        ctx.settings.confirmed_launch.insert(key, described);
        ctx.launcher = RunState::Unknown;
        let mut app = App::with(ctx);
        let _ = app.update(Message::Launch);
        let body = &app.dialog().expect("confirm shown").body;
        assert!(body.contains(UNKNOWN_WARNING), "{body}");
    }
}
