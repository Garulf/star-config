mod files;
mod when;

pub use files::{CLOSE_GAME, DiffView, FileRow, FileState, restore, words};

use crate::context::Context;
use crate::screen::{Action, Confirm, Screen, Toast, ToastKind};
use crate::theme::{self, ButtonKind, FONT_MONO, FONT_SEMIBOLD, Tone, layout, size, space};
use crate::widgets::{
    VirtualListState, banner, button, cell, empty_state, heading, hint, icon_button, page_fixed,
    split, strong, table_header, table_row, tag, virtual_list,
};
use crate::{icons, work};
use chrono::NaiveDateTime;
use iced::widget::{column, container, row, text};
use iced::{Alignment, Element, Length, Padding, Task};
use sc_core::backup::{Scope, SnapshotId, SnapshotInfo, SnapshotState};
use sc_core::diff::DiffLine;

pub const CAPTION: &str = "star-config takes a snapshot of your files before every change it makes. Restore one to go back.";
pub const DIFF_CAPTION: &str =
    "Red lines are in your file now. Green lines come back if you restore.";
pub const NOT_TEXT: &str = "This file is too large or not text, so only its status is shown.";
pub const MANUAL_LABEL: &str = "Manual snapshot";
const NO_CHANGE: &str = "Restoring would not change this file.";
const PICK_FILE: &str = "Choose a file to see what restoring would change.";
const INTERRUPTED: &str = "star-config did not finish this change. Restore it to put these files back to how they were before it started.";
const FILE_COLUMNS: [(&str, Length); 2] = [
    ("File", Length::Fill),
    ("Compared with now", Length::Shrink),
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Diff {
    Loading,
    Ready(DiffView),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Files {
    Loading,
    Ready(Vec<FileRow>),
    Failed(String),
}

pub struct State {
    scope: Option<Scope>,
    loaded: bool,
    load_error: Option<String>,
    snapshots: Vec<SnapshotInfo>,
    count_label: String,
    selected: Option<SnapshotId>,
    files: Files,
    open_file: Option<String>,
    diff: Option<Diff>,
    list: VirtualListState,
    diff_list: VirtualListState,
    busy: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Scope, Result<Vec<SnapshotInfo>, String>),
    Scrolled(f32),
    DiffScrolled(f32),
    Select(SnapshotId),
    FilesLoaded(SnapshotId, Result<Vec<FileRow>, String>),
    OpenFile(String),
    DiffLoaded(SnapshotId, String, Result<DiffView, String>),
    SnapshotNow,
    SnapshotTaken(Result<SnapshotId, String>),
    AskRestore(SnapshotId),
    Restore(SnapshotId),
    Restored(Result<(SnapshotId, String), String>),
    AskDelete(SnapshotId),
    Delete(SnapshotId),
    Deleted(Result<(), String>),
}

impl State {
    pub fn new() -> Self {
        Self {
            scope: None,
            loaded: false,
            load_error: None,
            snapshots: Vec::new(),
            count_label: String::new(),
            selected: None,
            files: Files::Loading,
            open_file: None,
            diff: None,
            list: VirtualListState::default(),
            diff_list: VirtualListState::default(),
            busy: false,
        }
    }

    pub fn reload(&mut self, ctx: &Context) -> Task<Message> {
        let Some(scope) = ctx.scope() else {
            *self = Self::new();
            return Task::none();
        };
        if self.scope.as_ref() != Some(&scope) {
            *self = Self::new();
            self.scope = Some(scope.clone());
        }
        let store = ctx.writer().store().clone();
        work::background(
            move || {
                let listed = store
                    .list(&scope)
                    .map_err(|err| format!("Could not read the snapshots: {}", files::words(&err)));
                (scope, listed)
            },
            |(scope, listed)| Message::Loaded(scope, listed),
        )
    }

    pub fn update(&mut self, message: Message, ctx: &Context) -> Action<Message> {
        match message {
            Message::Loaded(scope, listed) => self.loaded(scope, listed, ctx),
            Message::Scrolled(offset) => {
                self.list.offset = offset;
                Action::none()
            }
            Message::DiffScrolled(offset) => {
                self.diff_list.offset = offset;
                Action::none()
            }
            Message::Select(id) => self.select(id, ctx),
            Message::FilesLoaded(id, rows) => self.files_loaded(id, rows, ctx),
            Message::OpenFile(rel) => self.open(rel, ctx),
            Message::DiffLoaded(id, rel, diff) => {
                if self.selected.as_ref() == Some(&id) && self.open_file.as_ref() == Some(&rel) {
                    self.diff = Some(match diff {
                        Ok(view) => Diff::Ready(view),
                        Err(err) => Diff::Failed(err),
                    });
                }
                Action::none()
            }
            Message::SnapshotNow => self.snapshot_now(ctx),
            Message::SnapshotTaken(Ok(id)) => {
                self.busy = false;
                self.selected = Some(id);
                Action::task(self.reload(ctx)).with_toast(Toast::success("Snapshot taken"))
            }
            Message::SnapshotTaken(Err(err)) => {
                self.busy = false;
                Action::task(self.reload(ctx)).with_toast(Toast::error(err))
            }
            Message::AskRestore(id) => self.ask_restore(id),
            Message::Restore(id) => self.restore(id, ctx),
            Message::Restored(result) => {
                self.busy = false;
                let toast = match result {
                    Ok((before, done)) => Toast::success(done).with_undo(before),
                    Err(err) => Toast::error(err),
                };
                Action::task(self.reload(ctx)).with_toast(toast)
            }
            Message::AskDelete(id) => self.ask_delete(id),
            Message::Delete(id) => self.delete(id, ctx),
            Message::Deleted(result) => {
                self.busy = false;
                let action = Action::task(self.reload(ctx));
                match result {
                    Ok(()) => action,
                    Err(err) => action.with_toast(Toast::error(err)),
                }
            }
        }
    }

    fn loaded(
        &mut self,
        scope: Scope,
        listed: Result<Vec<SnapshotInfo>, String>,
        ctx: &Context,
    ) -> Action<Message> {
        if ctx.scope().as_ref() != Some(&scope) {
            return Action::none();
        }
        self.loaded = true;
        match listed {
            Ok(snapshots) => {
                self.load_error = None;
                self.snapshots = snapshots;
            }
            Err(err) => {
                self.load_error = Some(err);
                self.snapshots.clear();
            }
        }
        self.count_label = match self.snapshots.len() {
            1 => "1 snapshot".to_string(),
            count => format!("{count} snapshots"),
        };
        let keep = self
            .selected
            .clone()
            .filter(|id| self.snapshots.iter().any(|info| &info.id == id));
        match keep.or_else(|| self.snapshots.first().map(|info| info.id.clone())) {
            Some(id) => self.select(id, ctx),
            None => {
                self.selected = None;
                self.files = Files::Loading;
                self.open_file = None;
                self.diff = None;
                Action::none()
            }
        }
    }

    fn snapshot(&self, id: &SnapshotId) -> Option<&SnapshotInfo> {
        self.snapshots.iter().find(|info| &info.id == id)
    }

    fn selected_info(&self) -> Option<&SnapshotInfo> {
        self.selected.as_ref().and_then(|id| self.snapshot(id))
    }

    fn select(&mut self, id: SnapshotId, ctx: &Context) -> Action<Message> {
        let (Some(scope), Some(channel), Some(info)) = (
            ctx.scope(),
            ctx.channel.clone(),
            self.snapshot(&id).cloned(),
        ) else {
            return Action::none();
        };
        if self.selected.as_ref() != Some(&id) {
            self.open_file = None;
            self.diff = None;
            self.files = Files::Loading;
        }
        self.selected = Some(id.clone());
        let store = ctx.writer().store().clone();
        Action::task(work::background(
            move || files::rows(&store, &scope, &channel, &info),
            move |rows| Message::FilesLoaded(id, rows),
        ))
    }

    fn files_loaded(
        &mut self,
        id: SnapshotId,
        rows: Result<Vec<FileRow>, String>,
        ctx: &Context,
    ) -> Action<Message> {
        if self.selected.as_ref() != Some(&id) {
            return Action::none();
        }
        let first_changed = rows.as_ref().ok().and_then(|rows| {
            rows.iter()
                .find(|row| row.state != FileState::Same)
                .map(|row| row.rel.clone())
        });
        self.files = match rows {
            Ok(rows) => Files::Ready(rows),
            Err(err) => Files::Failed(err),
        };
        let still_listed = self.open_file.as_ref().filter(|rel| {
            matches!(&self.files, Files::Ready(rows) if rows.iter().any(|row| &row.rel == *rel))
        });
        match still_listed.cloned().or(first_changed) {
            Some(rel) => self.open(rel, ctx),
            None => {
                self.open_file = None;
                self.diff = None;
                Action::none()
            }
        }
    }

    fn open(&mut self, rel: String, ctx: &Context) -> Action<Message> {
        let (Some(scope), Some(channel), Some(id)) =
            (ctx.scope(), ctx.channel.clone(), self.selected.clone())
        else {
            return Action::none();
        };
        if self.open_file.as_ref() != Some(&rel) {
            self.diff_list = VirtualListState::default();
        }
        self.open_file = Some(rel.clone());
        self.diff = Some(Diff::Loading);
        let store = ctx.writer().store().clone();
        Action::task(work::background(
            move || {
                let view = files::diff_view(&store, &scope, &channel, &id, &rel);
                (id, rel, view)
            },
            |(id, rel, view)| Message::DiffLoaded(id, rel, view),
        ))
    }

    fn snapshot_now(&mut self, ctx: &Context) -> Action<Message> {
        let (Some(scope), Some(channel)) = (ctx.scope(), ctx.channel.clone()) else {
            return Action::none();
        };
        if self.busy {
            return Action::none();
        }
        self.busy = true;
        let store = ctx.writer().store().clone();
        let keep = ctx.settings.snapshot_keep;
        Action::task(work::background(
            move || {
                let taken = store
                    .snapshot(&scope, &channel, &channel.managed_files(), MANUAL_LABEL)
                    .map_err(|err| format!("Could not take a snapshot: {}", files::words(&err)))?;
                store.prune(&scope, keep).map_err(|err| {
                    format!("Could not delete old snapshots: {}", files::words(&err))
                })?;
                Ok(taken.id)
            },
            Message::SnapshotTaken,
        ))
    }

    fn ask_restore(&self, id: SnapshotId) -> Action<Message> {
        let Some(info) = self.snapshot(&id) else {
            return Action::none();
        };
        let at = when::short(when::local(info.created_unix), when::now());
        Action::confirm(
            Confirm::new(
                format!(
                    "Restore {} to how {} on {at}?",
                    when::files(info.files.len()),
                    they_were(info.files.len())
                ),
                "Your current files are snapshotted first.",
            )
            .action("Restore", ButtonKind::Primary, Message::Restore(id))
            .cancel("Cancel"),
        )
    }

    fn restore(&mut self, id: SnapshotId, ctx: &Context) -> Action<Message> {
        let (Some(scope), Some(channel)) = (ctx.scope(), ctx.channel.clone()) else {
            return Action::none();
        };
        if self.busy {
            return Action::none();
        }
        self.busy = true;
        let done = match self.snapshot(&id) {
            Some(info) => format!(
                "Restored the files from {}",
                when::short(when::local(info.created_unix), when::now())
            ),
            None => "Restored the files".to_string(),
        };
        let writer = ctx.writer();
        Action::task(work::background(
            move || restore(&writer, &scope, &channel, &id).map(|before| (before, done)),
            Message::Restored,
        ))
    }

    fn ask_delete(&self, id: SnapshotId) -> Action<Message> {
        let Some(info) = self.snapshot(&id) else {
            return Action::none();
        };
        let now = when::now();
        let body = format!(
            "{} from {}. This cannot be undone.",
            when::label(&info.label, now),
            when::short(when::local(info.created_unix), now)
        );
        Action::confirm(
            Confirm::new("Delete this snapshot?", body)
                .action("Delete", ButtonKind::Danger, Message::Delete(id))
                .cancel("Cancel"),
        )
    }

    fn delete(&mut self, id: SnapshotId, ctx: &Context) -> Action<Message> {
        let Some(scope) = ctx.scope() else {
            return Action::none();
        };
        self.busy = true;
        let store = ctx.writer().store().clone();
        Action::task(work::background(
            move || {
                store
                    .delete(&scope, &id)
                    .map_err(|err| format!("Could not delete the snapshot: {}", files::words(&err)))
            },
            Message::Deleted,
        ))
    }

    pub fn view<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let title = Screen::Backups.label();
        if self.loaded && self.snapshots.is_empty() && self.load_error.is_none() {
            return page_fixed(
                title,
                Some(CAPTION),
                Vec::new(),
                empty_state(
                    "No snapshots yet",
                    "star-config takes one before every change it makes.",
                    (!self.busy).then_some(("Snapshot now", Message::SnapshotNow)),
                ),
            );
        }
        let snapshot_now = icon_button(
            icons::BACKUPS,
            "Snapshot now",
            ButtonKind::Secondary,
            (!self.busy).then_some(Message::SnapshotNow),
        );
        let body: Element<'a, Message> = match &self.load_error {
            Some(err) => column![banner(ToastKind::Error, err, None)].into(),
            None => split(self.snapshot_list(), layout::SPLIT_LEFT, self.detail(ctx)),
        };
        page_fixed(title, Some(CAPTION), vec![snapshot_now], body)
    }

    fn snapshot_list(&self) -> Element<'_, Message> {
        let now = when::now();
        column![
            table_header(&[(self.count_label.as_str(), Length::Fill)]),
            virtual_list(
                &self.list,
                self.snapshots.len(),
                layout::ROW_MIN_HEIGHT,
                Message::Scrolled,
                move |index| self.snapshot_row(index, now),
            ),
        ]
        .into()
    }

    fn snapshot_row(&self, index: usize, now: NaiveDateTime) -> Element<'_, Message> {
        let info = &self.snapshots[index];
        let selected = self.selected.as_ref() == Some(&info.id);
        let mut first =
            row![strong(when::list_label(when::local(info.created_unix), now)).width(Length::Fill)]
                .spacing(space::SM)
                .align_y(Alignment::Center);
        if info.state == SnapshotState::Applying {
            first = first.push(tag("Interrupted", Tone::Danger));
        }
        let second = row![
            hint(when::label(&info.label, now))
                .width(Length::Fill)
                .wrapping(text::Wrapping::None),
            hint(when::files(info.files.len())),
        ]
        .spacing(space::SM);
        iced::widget::button(
            column![first, second]
                .spacing(space::XS)
                .height(Length::Fill)
                .align_x(Alignment::Start),
        )
        .padding(ROW_PADDING)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::row_button(selected))
        .on_press(Message::Select(info.id.clone()))
        .into()
    }

    fn detail<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let Some(info) = self.selected_info() else {
            return container(hint("Choose a snapshot to see its files."))
                .center(Length::Fill)
                .into();
        };
        let mut content = column![self.detail_header(info, ctx)].spacing(space::LG);
        if info.state == SnapshotState::Applying {
            content = content.push(banner(ToastKind::Warning, INTERRUPTED, None));
        }
        content = content.push(self.file_table());
        content = content.push(self.diff_area());
        content.height(Length::Fill).into()
    }

    fn detail_header<'a>(&'a self, info: &'a SnapshotInfo, ctx: &Context) -> Element<'a, Message> {
        let now = when::now();
        let summary = format!(
            "{} · {}",
            when::full(when::local(info.created_unix)),
            when::files(info.files.len())
        );
        let can_restore = !ctx.read_only() && !self.busy;
        row![
            column![
                text(when::label(&info.label, now))
                    .size(size::LABEL)
                    .font(FONT_SEMIBOLD),
                hint(summary),
            ]
            .spacing(space::XS)
            .width(Length::Fill),
            button(
                "Delete",
                ButtonKind::DangerGhost,
                (!self.busy).then(|| Message::AskDelete(info.id.clone())),
            ),
            button(
                "Restore",
                ButtonKind::Primary,
                can_restore.then(|| Message::AskRestore(info.id.clone())),
            ),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center)
        .into()
    }

    fn file_table(&self) -> Element<'_, Message> {
        let rows: Vec<Element<'_, Message>> = match &self.files {
            Files::Loading => vec![padded(hint("Comparing with your files..."))],
            Files::Failed(err) => vec![padded(banner(ToastKind::Error, err, None))],
            Files::Ready(rows) if rows.is_empty() => {
                vec![padded(hint("This snapshot holds no files."))]
            }
            Files::Ready(rows) => rows.iter().map(|file| self.file_row(file)).collect(),
        };
        container(column![table_header(&FILE_COLUMNS), column(rows)])
            .style(theme::card)
            .into()
    }

    fn file_row<'a>(&'a self, file: &'a FileRow) -> Element<'a, Message> {
        let open = self.open_file.as_ref() == Some(&file.rel);
        table_row(
            vec![
                (cell(file.rel.as_str()), FILE_COLUMNS[0].1),
                (tag(file.state.text(), file.state.tone()), FILE_COLUMNS[1].1),
            ],
            open,
            Some(Message::OpenFile(file.rel.clone())),
        )
    }

    fn diff_area(&self) -> Element<'_, Message> {
        let Some(rel) = &self.open_file else {
            return container(hint(PICK_FILE)).center(Length::Fill).into();
        };
        let body: Element<'_, Message> = match &self.diff {
            None | Some(Diff::Loading) => centered(hint("Reading the file...")),
            Some(Diff::Failed(err)) => banner(ToastKind::Error, err, None),
            Some(Diff::Ready(DiffView::NotText)) => centered(hint(NOT_TEXT)),
            Some(Diff::Ready(DiffView::Lines(lines))) if lines.is_empty() => {
                centered(hint(NO_CHANGE))
            }
            Some(Diff::Ready(DiffView::Lines(lines))) => container(virtual_list(
                &self.diff_list,
                lines.len(),
                layout::DIFF_ROW_HEIGHT,
                Message::DiffScrolled,
                move |index| diff_row(&lines[index]),
            ))
            .padding(space::XS)
            .style(theme::sunken)
            .height(Length::Fill)
            .into(),
        };
        column![
            column![
                heading(format!("What restoring {rel} would change")),
                hint(DIFF_CAPTION)
            ]
            .spacing(space::XS),
            body,
        ]
        .spacing(space::SM)
        .height(Length::Fill)
        .into()
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

const ROW_PADDING: Padding = Padding {
    top: space::SM,
    right: space::LG,
    bottom: space::SM,
    left: space::LG,
};

fn padded<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .padding(ROW_PADDING)
        .width(Length::Fill)
        .into()
}

fn centered<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .center(Length::Fill)
        .style(theme::sunken)
        .into()
}

fn mono<'a>(content: &'a str, tone: Tone) -> Element<'a, Message> {
    text(content)
        .font(FONT_MONO)
        .size(size::CAPTION)
        .wrapping(text::Wrapping::None)
        .style(theme::toned(tone))
        .into()
}

fn diff_row(line: &DiffLine) -> Element<'_, Message> {
    let (sign, content, tone) = match line {
        DiffLine::Removed(content) => ("-", content.as_str(), Tone::Danger),
        DiffLine::Added(content) => ("+", content.as_str(), Tone::Success),
        DiffLine::Context(content) => ("", content.as_str(), Tone::Neutral),
        DiffLine::Gap => ("", "...", Tone::Muted),
    };
    let text_tone = if tone == Tone::Muted {
        Tone::Muted
    } else {
        Tone::Neutral
    };
    container(
        row![
            container(mono(sign, tone)).width(layout::DIFF_SIGN_WIDTH),
            mono(content, text_tone),
        ]
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 0.0,
        right: space::SM,
        bottom: 0.0,
        left: space::SM,
    })
    .height(Length::Fill)
    .width(Length::Fill)
    .align_y(Alignment::Center)
    .style(theme::diff_line(tone))
    .into()
}

fn they_were(count: usize) -> &'static str {
    if count == 1 { "it was" } else { "they were" }
}

/// The startup question for a change that did not finish. `restore` and `keep` are the
/// messages for "Restore" and "Keep as is"; "Decide later" only closes it.
pub fn recovery_confirm<M>(info: &SnapshotInfo, restore: M, keep: M) -> Confirm<M> {
    let at = when::short(when::local(info.created_unix), when::now());
    Confirm::new(
        "A change was interrupted",
        format!(
            "star-config was writing {} on {at} and did not finish. Restore {} to how {}?",
            when::files(info.files.len()),
            if info.files.len() == 1 { "it" } else { "them" },
            they_were(info.files.len())
        ),
    )
    .action("Restore", ButtonKind::Primary, restore)
    .action("Keep as is", ButtonKind::Secondary, keep)
    .cancel("Decide later")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::backup::SnapshotFile;

    fn info(files: usize) -> SnapshotInfo {
        SnapshotInfo {
            id: SnapshotId("20261114T190200Z".into()),
            created_unix: 1_763_146_920,
            label: "Config changes".into(),
            state: SnapshotState::Applying,
            files: (0..files)
                .map(|n| SnapshotFile {
                    rel: format!("f{n}"),
                    existed: true,
                    size: 1,
                })
                .collect(),
        }
    }

    #[test]
    fn recovery_prompt_names_the_file_count_and_offers_three_answers() {
        let confirm = recovery_confirm(&info(3), 1, 2);
        assert_eq!(confirm.title, "A change was interrupted");
        assert!(
            confirm
                .body
                .starts_with("star-config was writing 3 files on ")
        );
        assert!(
            confirm
                .body
                .ends_with(" and did not finish. Restore them to how they were?")
        );
        let labels: Vec<&str> = confirm.actions.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(labels, ["Restore", "Keep as is", "Decide later"]);
        assert_eq!(confirm.actions[2].message, None);
    }

    #[test]
    fn nothing_is_ever_staged() {
        let mut state = State::new();
        state.discard();
        assert_eq!(state.pending(), 0);
    }
}
