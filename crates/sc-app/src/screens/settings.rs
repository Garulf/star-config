use crate::context::Context;
use crate::screen::{Action, Screen, Toast};
use crate::theme::{ButtonKind, Tone, layout, space};
use crate::widgets::{
    button, field_row, hint, label, page, rows, section, segmented, setting_row, slider, tag,
    text_field,
};
use crate::{icons, work};
use iced::widget::{column, row, text};
use iced::{Alignment, Element, Length, Task};
use sc_core::install::Install;
use sc_core::launcher;
use sc_core::settings::{Settings, ThemeChoice};
use std::path::{Path, PathBuf};

const KEEP_MIN: u32 = 5;
const KEEP_MAX: u32 = 200;
const KEEP_STEP: u32 = 5;

pub struct State {
    install_key: Option<String>,
    saved_command: String,
    command: String,
    saved_keep: u32,
    keep: u32,
    detected: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    ThemeChosen(ThemeChoice),
    AddFolder,
    FolderPicked(Option<PathBuf>),
    FolderChecked(Option<Install>),
    RemoveFolder(PathBuf),
    Command(String),
    Keep(u32),
    Detected(Option<String>),
    OpenBackups,
    Opened(Result<(), String>),
}

fn keep_of(settings: &Settings) -> u32 {
    u32::try_from(settings.snapshot_keep)
        .unwrap_or(KEEP_MAX)
        .clamp(KEEP_MIN, KEEP_MAX)
}

impl State {
    pub fn new() -> Self {
        Self {
            install_key: None,
            saved_command: String::new(),
            command: String::new(),
            saved_keep: keep_of(&Settings::default()),
            keep: keep_of(&Settings::default()),
            detected: None,
        }
    }

    pub fn reload(&mut self, ctx: &Context) -> Task<Message> {
        self.install_key = ctx.install.as_ref().map(Install::key);
        self.saved_command = self
            .install_key
            .as_ref()
            .and_then(|key| ctx.settings.launch_commands.get(key))
            .cloned()
            .unwrap_or_default();
        self.command = self.saved_command.clone();
        self.saved_keep = keep_of(&ctx.settings);
        self.keep = self.saved_keep;
        self.detected = None;
        let Some(install) = ctx.install.clone() else {
            return Task::none();
        };
        work::background(
            move || launcher::detect(&install, None).map(|method| method.describe()),
            Message::Detected,
        )
    }

    pub fn update(&mut self, message: Message, ctx: &Context) -> Action<Message> {
        match message {
            Message::ThemeChosen(choice) => {
                let mut settings = ctx.settings.clone();
                settings.theme = choice;
                Action::settings(settings)
            }
            Message::AddFolder => Action::task(work::pick_folder(
                "Choose a StarCitizen folder or Wine prefix",
                Message::FolderPicked,
            )),
            Message::FolderPicked(None) => Action::none(),
            Message::FolderPicked(Some(path)) => Action::task(work::background(
                move || Install::from_dir(&path),
                Message::FolderChecked,
            )),
            Message::FolderChecked(None) => {
                Action::toast(Toast::error("That folder has no Star Citizen install"))
            }
            Message::FolderChecked(Some(install)) => {
                if ctx.settings.installs.contains(&install.root) {
                    return Action::toast(Toast::info("That install is already listed"));
                }
                let mut settings = ctx.settings.clone();
                settings.installs.push(install.root.clone());
                Action::settings(settings)
                    .with_rescan()
                    .with_toast(Toast::success(format!(
                        "Added {}",
                        ctx.install_label(&install)
                    )))
            }
            Message::RemoveFolder(path) => {
                let mut settings = ctx.settings.clone();
                settings.installs.retain(|known| known != &path);
                if settings.selected_install.as_ref() == Some(&path) {
                    settings.selected_install = None;
                }
                Action::settings(settings).with_rescan()
            }
            Message::Command(command) => {
                self.command = command;
                Action::none()
            }
            Message::Keep(keep) => {
                self.keep = keep;
                Action::none()
            }
            Message::Detected(detected) => {
                self.detected = detected;
                Action::none()
            }
            Message::OpenBackups => {
                Action::task(work::open_path(ctx.dirs.backups(), Message::Opened))
            }
            Message::Opened(Ok(())) => Action::none(),
            Message::Opened(Err(err)) => Action::toast(Toast::error(format!(
                "Could not open the backups folder: {err}"
            ))),
        }
    }

    pub fn view<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let body = column![
            self.appearance(ctx),
            self.installs(ctx),
            self.launch(ctx),
            self.backups(),
        ]
        .spacing(space::XL);
        page(
            Screen::Settings.label(),
            Some("How star-config itself behaves."),
            Vec::new(),
            body,
        )
    }

    fn appearance<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let current = ctx.settings.theme;
        section(
            "Appearance",
            setting_row(
                "Theme",
                None,
                false,
                segmented(vec![
                    (
                        "Dark",
                        current == ThemeChoice::Dark,
                        Message::ThemeChosen(ThemeChoice::Dark),
                    ),
                    (
                        "Light",
                        current == ThemeChoice::Light,
                        Message::ThemeChosen(ThemeChoice::Light),
                    ),
                ]),
            ),
        )
    }

    fn installs<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let mut items: Vec<Element<'a, Message>> = Vec::new();
        for install in &ctx.installs {
            let added = ctx.settings.installs.contains(&install.root);
            let selected = ctx.install.as_ref() == Some(install);
            let label = ctx.install_label(install);
            let root = ctx.display_path(&install.root);
            items.push(install_row(
                (root != label).then_some(root),
                label,
                if selected {
                    Some(("Selected", Tone::Neutral))
                } else if added {
                    None
                } else {
                    Some(("Found automatically", Tone::Muted))
                },
                added.then(|| Message::RemoveFolder(install.root.clone())),
            ));
        }
        for path in &ctx.settings.installs {
            if ctx.installs.iter().all(|install| &install.root != path) {
                items.push(install_row(
                    Some("This folder no longer holds a Star Citizen install".to_string()),
                    ctx.display_path(path),
                    Some(("Missing", Tone::Warning)),
                    Some(Message::RemoveFolder(path.clone())),
                ));
            }
        }
        if items.is_empty() {
            items.push(
                row![muted_row(
                    "No installs yet. Add the folder that holds StarCitizen."
                )]
                .into(),
            );
        }
        items.push(
            row![
                hint("Folders star-config checks besides the usual places.").width(Length::Fill),
                crate::widgets::icon_button(
                    icons::FOLDER,
                    "Add folder",
                    ButtonKind::Secondary,
                    Some(Message::AddFolder),
                ),
            ]
            .align_y(Alignment::Center)
            .padding(row_padding())
            .into(),
        );
        section("Install folders", rows(items))
    }

    fn launch<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        if ctx.install.is_none() {
            return section(
                "Launch",
                muted_row("Choose an install to set how it launches."),
            );
        }
        let detected = self.detected.as_deref().map_or_else(
            || "None found".to_string(),
            |found| ctx.display_path(Path::new(found)),
        );
        let placeholder = self
            .detected
            .as_deref()
            .unwrap_or("lutris lutris:rungame/star-citizen");
        let changed = self.command != self.saved_command;
        section(
            "Launch",
            rows([
                setting_row(
                    "Detected launcher",
                    Some("Used when no launch command is set."),
                    false,
                    text(detected)
                        .size(crate::theme::size::BODY)
                        .style(crate::theme::muted),
                ),
                field_row(
                    "Launch command",
                    Some("Runs instead of the detected launcher for this install."),
                    changed,
                    Some(display_command(&self.saved_command).to_string()),
                    text_field(placeholder, &self.command, Message::Command)
                        .width(layout::WIDE_CONTROL_WIDTH),
                ),
            ]),
        )
    }

    fn backups(&self) -> Element<'_, Message> {
        let keep_label = format!("{}", self.keep);
        let changed = self.keep != self.saved_keep;
        section(
            "Backups",
            rows([
                field_row(
                    "Snapshots to keep",
                    Some("Per channel. Older snapshots are deleted after each change."),
                    changed,
                    Some(self.saved_keep.to_string()),
                    row![
                        slider(KEEP_MIN..=KEEP_MAX, self.keep, Message::Keep).step(KEEP_STEP),
                        label(keep_label)
                            .width(layout::VALUE_WIDTH)
                            .align_x(Alignment::End),
                    ]
                    .spacing(space::MD)
                    .align_y(Alignment::Center),
                ),
                setting_row(
                    "Backups folder",
                    Some("Where snapshots are stored."),
                    false,
                    button("Open", ButtonKind::Secondary, Some(Message::OpenBackups)),
                ),
            ]),
        )
    }

    pub fn pending(&self) -> usize {
        usize::from(self.command != self.saved_command) + usize::from(self.keep != self.saved_keep)
    }

    pub fn apply(&mut self, ctx: &Context) -> Action<Message> {
        if self.pending() == 0 {
            return Action::none();
        }
        let mut settings = ctx.settings.clone();
        if let Some(key) = &self.install_key {
            let command = self.command.trim();
            if command.is_empty() {
                settings.launch_commands.remove(key);
            } else {
                settings
                    .launch_commands
                    .insert(key.clone(), command.to_string());
            }
        }
        settings.snapshot_keep = self.keep as usize;
        self.saved_command = self.command.clone();
        self.saved_keep = self.keep;
        Action::settings(settings).with_toast(Toast::success("Settings saved"))
    }

    pub fn discard(&mut self) {
        self.command = self.saved_command.clone();
        self.keep = self.saved_keep;
    }

    pub fn undo(&mut self, _ctx: &Context) -> Action<Message> {
        Action::none()
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

fn display_command(command: &str) -> &str {
    if command.is_empty() { "empty" } else { command }
}

fn row_padding() -> iced::Padding {
    iced::Padding {
        top: space::MD,
        right: space::LG,
        bottom: space::MD,
        left: space::LG,
    }
}

fn muted_row<'a>(content: &'a str) -> Element<'a, Message> {
    iced::widget::container(crate::widgets::muted(content))
        .padding(row_padding())
        .width(Length::Fill)
        .into()
}

fn install_row<'a>(
    detail: Option<String>,
    name: String,
    status: Option<(&'a str, Tone)>,
    remove: Option<Message>,
) -> Element<'a, Message> {
    let mut title = row![label(name)]
        .spacing(space::SM)
        .align_y(Alignment::Center);
    if let Some((text, tone)) = status {
        title = title.push(tag(text, tone));
    }
    let mut line = row![
        column![title]
            .push(detail.map(hint))
            .spacing(space::XS)
            .width(Length::Fill),
    ]
    .spacing(space::LG)
    .align_y(Alignment::Center);
    if let Some(remove) = remove {
        line = line.push(button("Remove", ButtonKind::Ghost, Some(remove)));
    }
    iced::widget::container(line)
        .padding(row_padding())
        .width(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::dirs::AppDirs;

    fn context() -> (tempfile::TempDir, Context) {
        let temp = tempfile::tempdir().unwrap();
        let ctx = Context::new(AppDirs::under(temp.path()), Settings::default(), Vec::new());
        (temp, ctx)
    }

    #[test]
    fn editing_keep_counts_as_pending_and_discard_clears_it() {
        let (_temp, ctx) = context();
        let mut state = State::new();
        let _ = state.reload(&ctx);
        let _ = state.update(Message::Keep(80), &ctx);
        assert_eq!(state.pending(), 1);
        state.discard();
        assert_eq!(state.pending(), 0);
    }

    #[test]
    fn apply_returns_new_settings() {
        let (_temp, ctx) = context();
        let mut state = State::new();
        let _ = state.reload(&ctx);
        let _ = state.update(Message::Keep(80), &ctx);
        let action = state.apply(&ctx);
        assert_eq!(action.settings.unwrap().snapshot_keep, 80);
        assert_eq!(state.pending(), 0);
    }

    #[test]
    fn theme_choice_is_saved_at_once() {
        let (_temp, ctx) = context();
        let mut state = State::new();
        let action = state.update(Message::ThemeChosen(ThemeChoice::Light), &ctx);
        assert_eq!(action.settings.unwrap().theme, ThemeChoice::Light);
    }
}
