use crate::icons;
use crate::theme::ButtonKind;
use iced::Task;
use sc_core::backup::{Scope, SnapshotId};
use sc_core::install::Channel;
use sc_core::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Screen {
    Overview,
    Config,
    Keybinds,
    Comms,
    StarStrings,
    Screenshots,
    Backups,
    Maintenance,
    Channels,
    Settings,
}

impl Screen {
    pub const ALL: [Screen; 10] = [
        Screen::Overview,
        Screen::Config,
        Screen::Keybinds,
        Screen::Comms,
        Screen::StarStrings,
        Screen::Screenshots,
        Screen::Backups,
        Screen::Maintenance,
        Screen::Channels,
        Screen::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Screen::Overview => "Overview",
            Screen::Config => "Config",
            Screen::Keybinds => "Keybinds",
            Screen::Comms => "Head tracking and comms",
            Screen::StarStrings => "StarStrings",
            Screen::Screenshots => "Screenshots",
            Screen::Backups => "Backups",
            Screen::Maintenance => "Maintenance",
            Screen::Channels => "Channels",
            Screen::Settings => "Settings",
        }
    }

    pub fn icon(self) -> &'static [u8] {
        match self {
            Screen::Overview => icons::OVERVIEW,
            Screen::Config => icons::CONFIG,
            Screen::Keybinds => icons::KEYBINDS,
            Screen::Comms => icons::COMMS,
            Screen::StarStrings => icons::STARSTRINGS,
            Screen::Screenshots => icons::SCREENSHOTS,
            Screen::Backups => icons::BACKUPS,
            Screen::Maintenance => icons::MAINTENANCE,
            Screen::Channels => icons::CHANNELS,
            Screen::Settings => icons::SETTINGS,
        }
    }

    /// Whether the screen works without a selected install and channel.
    pub fn needs_channel(self) -> bool {
        self != Screen::Settings
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub kind: ToastKind,
    pub text: String,
    pub undo: Option<SnapshotId>,
    /// Set when Undo must restore snapshots of channels other than the selected one.
    pub undo_in: Vec<UndoIn>,
}

/// A snapshot and the channel it belongs to, for an Undo outside the selected channel.
#[derive(Debug, Clone, PartialEq)]
pub struct UndoIn {
    pub scope: Scope,
    pub channel: Channel,
    pub snapshot: SnapshotId,
}

impl Toast {
    pub fn new(kind: ToastKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
            undo: None,
            undo_in: Vec::new(),
        }
    }

    pub fn info(text: impl Into<String>) -> Self {
        Self::new(ToastKind::Info, text)
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self::new(ToastKind::Success, text)
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self::new(ToastKind::Error, text)
    }

    pub fn with_undo(mut self, snapshot: SnapshotId) -> Self {
        self.undo = Some(snapshot);
        self
    }

    /// Undo restores every one of `targets`, last first, each in its own channel.
    pub fn with_undo_in(mut self, targets: Vec<UndoIn>) -> Self {
        self.undo = targets.first().map(|target| target.snapshot.clone());
        self.undo_in = targets;
        self
    }

    /// Info and Success toasts leave on their own; Warning and Error stay until dismissed.
    pub fn expires(&self) -> bool {
        matches!(self.kind, ToastKind::Info | ToastKind::Success)
    }
}

/// One button of a [`Confirm`]. `message: None` only closes the dialog.
#[derive(Debug, Clone)]
pub struct ConfirmAction<M> {
    pub label: String,
    pub kind: ButtonKind,
    pub message: Option<M>,
}

/// A modal question the root shows over the whole window.
#[derive(Debug, Clone)]
pub struct Confirm<M> {
    pub title: String,
    pub body: String,
    pub actions: Vec<ConfirmAction<M>>,
}

impl<M> Confirm<M> {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            actions: Vec::new(),
        }
    }

    pub fn action(mut self, label: impl Into<String>, kind: ButtonKind, message: M) -> Self {
        self.actions.push(ConfirmAction {
            label: label.into(),
            kind,
            message: Some(message),
        });
        self
    }

    pub fn cancel(mut self, label: impl Into<String>) -> Self {
        self.actions.push(ConfirmAction {
            label: label.into(),
            kind: ButtonKind::Secondary,
            message: None,
        });
        self
    }

    /// The action Enter picks: the first Primary or Danger one, else the first.
    pub fn primary(&self) -> Option<usize> {
        self.actions
            .iter()
            .position(|action| matches!(action.kind, ButtonKind::Primary | ButtonKind::Danger))
            .or((!self.actions.is_empty()).then_some(0))
    }

    pub fn map<N>(self, f: impl Fn(M) -> N) -> Confirm<N> {
        Confirm {
            title: self.title,
            body: self.body,
            actions: self
                .actions
                .into_iter()
                .map(|action| ConfirmAction {
                    label: action.label,
                    kind: action.kind,
                    message: action.message.map(&f),
                })
                .collect(),
        }
    }
}

/// What a screen asks the root to do after `update` or `apply`.
pub struct Action<M> {
    pub task: Task<M>,
    pub toast: Option<Toast>,
    /// Some = root saves it and updates Context.
    pub settings: Option<Settings>,
    /// true = root rediscovers installs and channels.
    pub rescan: bool,
    pub confirm: Option<Confirm<M>>,
}

impl<M> Action<M> {
    pub fn none() -> Self {
        Self {
            task: Task::none(),
            toast: None,
            settings: None,
            rescan: false,
            confirm: None,
        }
    }

    pub fn task(task: Task<M>) -> Self {
        Self {
            task,
            ..Self::none()
        }
    }

    pub fn toast(toast: Toast) -> Self {
        Self {
            toast: Some(toast),
            ..Self::none()
        }
    }

    pub fn settings(settings: Settings) -> Self {
        Self {
            settings: Some(settings),
            ..Self::none()
        }
    }

    pub fn rescan() -> Self {
        Self {
            rescan: true,
            ..Self::none()
        }
    }

    pub fn confirm(confirm: Confirm<M>) -> Self {
        Self {
            confirm: Some(confirm),
            ..Self::none()
        }
    }

    pub fn with_task(mut self, task: Task<M>) -> Self {
        self.task = task;
        self
    }

    pub fn with_toast(mut self, toast: Toast) -> Self {
        self.toast = Some(toast);
        self
    }

    pub fn with_settings(mut self, settings: Settings) -> Self {
        self.settings = Some(settings);
        self
    }

    pub fn with_rescan(mut self) -> Self {
        self.rescan = true;
        self
    }

    pub fn map<N>(self, f: impl Fn(M) -> N + Clone + Send + 'static) -> Action<N>
    where
        M: Send + 'static,
        N: Send + 'static,
    {
        Action {
            task: self.task.map(f.clone()),
            toast: self.toast,
            settings: self.settings,
            rescan: self.rescan,
            confirm: self.confirm.map(|confirm| confirm.map(f)),
        }
    }
}

impl<M> Default for Action<M> {
    fn default() -> Self {
        Self::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_follow_sidebar_order() {
        let labels: Vec<_> = Screen::ALL.iter().map(|screen| screen.label()).collect();
        assert_eq!(labels[3], "Head tracking and comms");
        assert_eq!(labels[9], "Settings");
    }

    #[test]
    fn confirm_primary_prefers_primary_or_danger() {
        let confirm =
            Confirm::new("t", "b")
                .cancel("Cancel")
                .action("Delete", ButtonKind::Danger, ());
        assert_eq!(confirm.primary(), Some(1));
        let only_cancel = Confirm::<()>::new("t", "b").cancel("Close");
        assert_eq!(only_cancel.primary(), Some(0));
    }
}
