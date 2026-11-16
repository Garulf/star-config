mod jobs;
mod view;
mod words;

pub use jobs::{ChannelFacts, ResumeFailure, Survey, SwapFailure};
pub use words::{
    CANNOT_TELL, COMPARE_CONTENTS, GAME_CAPTION, IN_PLACE, NOT_COPIED, PENDING_TITLE,
    SETTINGS_CAPTION, STARSTRINGS_NOTE, SWAP_STOPPED, SWAP_TRAVEL, SWAP_UNDONE,
};

use crate::context::Context;
use crate::screen::{Action, Confirm, Toast, ToastKind};
use crate::theme::ButtonKind;
use crate::work;
use iced::Task;
use jobs::{SettingsJob, UnlockedProbe};
use sc_core::channels::{
    self, CopyOptions, CopyOutcome, CopyPlan, Part, Progress, Resume, Step, SwapJournal,
};
use sc_core::dirs::AppDirs;
use sc_core::install::Channel;
use sc_core::process::{GameProbe, RunState};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use words::PARTS;

/// A swap journal found on disk and the file it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingSwap {
    pub file: PathBuf,
    pub journal: SwapJournal,
}

/// Where swap journals are kept, in the app's own data folder.
pub fn journal_dir(dirs: &AppDirs) -> PathBuf {
    dirs.data.join("swaps")
}

/// The question shown at startup when a swap of the selected install did not finish.
pub fn startup_prompt(ctx: &Context) -> Option<Confirm<Message>> {
    let install = ctx.install.as_ref()?;
    let (file, journal) = channels::find_pending_swap(&journal_dir(&ctx.dirs), install)
        .ok()
        .flatten()?;
    Some(resume_confirm(PendingSwap { file, journal }))
}

fn resume_confirm(pending: PendingSwap) -> Confirm<Message> {
    Confirm::new(PENDING_TITLE, words::pending_body(&pending.journal))
        .action(
            "Finish the swap",
            ButtonKind::Primary,
            Message::Resume(Box::new(pending.clone()), Resume::Finish),
        )
        .action(
            "Undo it",
            ButtonKind::Secondary,
            Message::Resume(Box::new(pending), Resume::Undo),
        )
        .cancel("Decide later")
}

#[derive(Debug, Clone)]
pub enum Message {
    Surveyed(Box<Survey>),
    Measured(PathBuf, Vec<(String, u64)>),
    PickFrom(String),
    PickTo(String),
    Reverse,
    Checked(String, String, Result<(), String>),
    PlanCopy,
    CompareContents(bool),
    CopyPlanned(String, String, bool, Result<Box<CopyPlan>, String>),
    InPlace(bool),
    /// Starts the planned copy. Runs are numbered from 1 in the order they start.
    StartCopy,
    CopyProgress(u64, Progress),
    Copied(u64, Result<CopyOutcome, String>),
    CancelCopy,
    CloseCopy,
    AskSwap,
    SwapTravel(bool),
    SwapPlanned(String, String, bool, Result<Box<SwapJournal>, String>),
    CancelSwap,
    Swap,
    Swapped(String, String, Result<(), SwapFailure>),
    Resume(Box<PendingSwap>, Resume),
    Resumed(Resume, Box<PendingSwap>, Result<(), ResumeFailure>),
    DismissNotice,
    TickPart(Part, bool),
    CopySettings,
    CopySettingsConfirmed,
    SwapSettings,
    SwapSettingsConfirmed,
    SettingsDone(Box<Toast>),
}

#[derive(Debug, Clone, PartialEq)]
enum Pair {
    Checking,
    Allowed,
    Refused(String),
}

struct Running {
    run: u64,
    plan: Box<CopyPlan>,
    in_place: bool,
    progress: Option<Progress>,
    cancel: Arc<AtomicBool>,
    stopping: bool,
}

enum CopyStage {
    Idle,
    Planning,
    Planned(Box<CopyPlan>),
    Failed(String),
    Running(Box<Running>),
}

enum SwapStage {
    Idle,
    Planning,
    Confirming {
        journal: Box<SwapJournal>,
        travel: bool,
    },
    Failed(String),
    Swapping,
}

/// A standing message at the top of the screen that stays until dismissed.
struct Notice {
    kind: ToastKind,
    text: String,
}

pub struct State {
    install_root: Option<PathBuf>,
    surveyed: bool,
    channels: Vec<Channel>,
    facts: Vec<ChannelFacts>,
    sizes: Option<BTreeMap<String, u64>>,
    from: Option<String>,
    to: Option<String>,
    pair: Pair,
    options: CopyOptions,
    copy: CopyStage,
    swap: SwapStage,
    swap_travel: bool,
    pending: Option<PendingSwap>,
    stuck: bool,
    resuming: bool,
    notice: Option<Notice>,
    ticked: [bool; 5],
    settings_busy: bool,
    next_run: u64,
}

impl State {
    pub fn new() -> Self {
        Self {
            install_root: None,
            surveyed: false,
            channels: Vec::new(),
            facts: Vec::new(),
            sizes: None,
            from: None,
            to: None,
            pair: Pair::Checking,
            options: CopyOptions::default(),
            copy: CopyStage::Idle,
            swap: SwapStage::Idle,
            swap_travel: false,
            pending: None,
            stuck: false,
            resuming: false,
            notice: None,
            ticked: PARTS.map(|part| part != Part::StarStrings),
            settings_busy: false,
            next_run: 1,
        }
    }

    pub fn reload(&mut self, ctx: &Context) -> Task<Message> {
        let Some(install) = ctx.install.clone() else {
            return Task::none();
        };
        if self.install_root.as_ref() != Some(&install.root) {
            *self = Self {
                next_run: self.next_run,
                ..Self::new()
            };
            self.install_root = Some(install.root.clone());
        }
        self.sizes = None;
        let dir = journal_dir(&ctx.dirs);
        work::background(
            move || jobs::survey(&install, &dir),
            |survey| Message::Surveyed(Box::new(survey)),
        )
    }

    /// True while a copy runs. The shell asks before switching install or channel.
    pub fn busy(&self) -> bool {
        matches!(self.copy, CopyStage::Running(_))
    }

    /// Stops a running copy after the current file and forgets it.
    pub fn stop_copy(&mut self) {
        if let CopyStage::Running(running) = &self.copy {
            running.cancel.store(true, Ordering::SeqCst);
            self.copy = CopyStage::Idle;
        }
    }

    pub fn update(&mut self, message: Message, ctx: &Context) -> Action<Message> {
        match message {
            Message::Surveyed(survey) => self.surveyed(*survey, ctx),
            Message::Measured(root, sizes) => {
                if self.install_root.as_ref() == Some(&root) {
                    self.sizes = Some(sizes.into_iter().collect());
                }
                Action::none()
            }
            Message::PickFrom(name) => self.pick(ctx, Some(name), None),
            Message::PickTo(name) => self.pick(ctx, None, Some(name)),
            Message::Reverse => {
                let (from, to) = (self.to.clone(), self.from.clone());
                self.pick(ctx, from, to)
            }
            Message::Checked(from, to, result) => {
                if self.is_pair(&from, &to) {
                    self.pair = match result {
                        Ok(()) => Pair::Allowed,
                        Err(why) => Pair::Refused(why),
                    };
                }
                Action::none()
            }
            Message::PlanCopy => self.plan_copy(ctx),
            Message::CompareContents(on) => {
                self.options.compare_contents = on;
                match self.copy {
                    CopyStage::Planning | CopyStage::Planned(_) | CopyStage::Failed(_) => {
                        self.plan_copy(ctx)
                    }
                    _ => Action::none(),
                }
            }
            Message::CopyPlanned(from, to, compare, result) => {
                let wanted = self.is_pair(&from, &to)
                    && compare == self.options.compare_contents
                    && matches!(self.copy, CopyStage::Planning);
                if wanted {
                    self.copy = match result {
                        Ok(plan) => CopyStage::Planned(plan),
                        Err(why) => CopyStage::Failed(why),
                    };
                }
                Action::none()
            }
            Message::InPlace(on) => {
                self.options.in_place = on;
                Action::none()
            }
            Message::StartCopy => self.start_copy(ctx),
            Message::CopyProgress(run, progress) => {
                if let CopyStage::Running(running) = &mut self.copy
                    && running.run == run
                {
                    running.progress = Some(progress);
                }
                Action::none()
            }
            Message::Copied(run, result) => self.copied(run, result),
            Message::CancelCopy => {
                if let CopyStage::Running(running) = &mut self.copy {
                    running.cancel.store(true, Ordering::SeqCst);
                    running.stopping = true;
                }
                Action::none()
            }
            Message::CloseCopy => {
                if matches!(self.copy, CopyStage::Planned(_) | CopyStage::Failed(_)) {
                    self.copy = CopyStage::Idle;
                }
                Action::none()
            }
            Message::AskSwap => {
                if !self.can_swap(ctx) {
                    return Action::none();
                }
                self.swap_travel = false;
                self.swap = SwapStage::Planning;
                self.plan_swap(ctx)
            }
            Message::SwapTravel(on) => {
                if !matches!(self.swap, SwapStage::Confirming { .. }) {
                    return Action::none();
                }
                self.swap_travel = on;
                self.plan_swap(ctx)
            }
            Message::SwapPlanned(from, to, travel, result) => {
                let wanted = self.is_pair(&from, &to)
                    && travel == self.swap_travel
                    && matches!(
                        self.swap,
                        SwapStage::Planning | SwapStage::Confirming { .. }
                    );
                if wanted {
                    self.swap = match result {
                        Ok(journal) => SwapStage::Confirming { journal, travel },
                        Err(why) => SwapStage::Failed(why),
                    };
                }
                Action::none()
            }
            Message::CancelSwap => {
                if !matches!(self.swap, SwapStage::Swapping) {
                    self.swap = SwapStage::Idle;
                }
                Action::none()
            }
            Message::Swap => self.swap(ctx),
            Message::Swapped(a, b, result) => self.swapped(&a, &b, result),
            Message::Resume(pending, how) => self.resume(ctx, *pending, how),
            Message::Resumed(how, pending, result) => self.resumed(ctx, how, *pending, result),
            Message::DismissNotice => {
                self.notice = None;
                Action::none()
            }
            Message::TickPart(part, on) => {
                if let Some(index) = PARTS.iter().position(|p| *p == part) {
                    self.ticked[index] = on;
                }
                Action::none()
            }
            Message::CopySettings => self.ask_settings(ctx, false),
            Message::SwapSettings => self.ask_settings(ctx, true),
            Message::CopySettingsConfirmed => self.run_settings(ctx, false),
            Message::SwapSettingsConfirmed => self.run_settings(ctx, true),
            Message::SettingsDone(toast) => {
                self.settings_busy = false;
                Action::toast(*toast).with_task(self.reload(ctx))
            }
        }
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

    fn surveyed(&mut self, survey: Survey, ctx: &Context) -> Action<Message> {
        if self.install_root.as_ref() != Some(&survey.install_root) {
            return Action::none();
        }
        if self.pending.as_ref().map(|p| &p.file) != survey.pending.as_ref().map(|p| &p.file) {
            self.stuck = false;
        }
        self.pending = survey.pending;
        self.channels = survey.channels;
        self.facts = survey.facts;
        self.surveyed = true;
        self.choose_defaults(ctx);
        let root = survey.install_root;
        let channels = self.channels.clone();
        let measure = work::background(
            move || jobs::measure(&channels),
            move |sizes| Message::Measured(root, sizes),
        );
        Action::task(Task::batch([measure, self.check(ctx)]))
    }

    fn has_name(&self, name: Option<&str>) -> bool {
        name.is_some_and(|name| self.facts.iter().any(|facts| facts.name == name))
    }

    fn facts_of(&self, name: Option<&str>) -> Option<&ChannelFacts> {
        let name = name?;
        self.facts.iter().find(|facts| facts.name == name)
    }

    fn channel(&self, name: Option<&str>) -> Option<Channel> {
        let name = name?;
        self.channels.iter().find(|c| c.name == name).cloned()
    }

    fn choose_defaults(&mut self, ctx: &Context) {
        if !self.has_name(self.from.as_deref()) {
            let current = ctx
                .channel
                .as_ref()
                .map(|c| c.name.clone())
                .filter(|name| self.channel(Some(name.as_str())).is_some());
            self.from = current.or_else(|| self.channels.first().map(|c| c.name.clone()));
        }
        if !self.has_name(self.to.as_deref()) || self.to == self.from {
            let from = self.from.clone();
            let installed_other = self
                .channels
                .iter()
                .map(|c| c.name.clone())
                .find(|name| Some(name) != from.as_ref());
            self.to = installed_other.or_else(|| {
                self.facts
                    .iter()
                    .map(|facts| facts.name.clone())
                    .find(|name| Some(name) != from.as_ref())
            });
        }
    }

    fn is_pair(&self, from: &str, to: &str) -> bool {
        self.from.as_deref() == Some(from) && self.to.as_deref() == Some(to)
    }

    fn locked(&self) -> bool {
        matches!(self.copy, CopyStage::Running(_)) || matches!(self.swap, SwapStage::Swapping)
    }

    fn pick(&mut self, ctx: &Context, from: Option<String>, to: Option<String>) -> Action<Message> {
        if self.locked() {
            return Action::none();
        }
        let next_from = from.or_else(|| self.from.clone());
        let next_to = to.or_else(|| self.to.clone());
        if next_from == self.from && next_to == self.to {
            return Action::none();
        }
        self.from = next_from;
        self.to = next_to;
        self.copy = CopyStage::Idle;
        self.swap = SwapStage::Idle;
        Action::task(self.check(ctx))
    }

    fn check(&mut self, ctx: &Context) -> Task<Message> {
        let (Some(install), Some(from), Some(to)) =
            (ctx.install.clone(), self.from.clone(), self.to.clone())
        else {
            return Task::none();
        };
        self.pair = Pair::Checking;
        let (f, t) = (from.clone(), to.clone());
        work::background(
            move || jobs::check_pair(&install, &from, &to),
            move |result| Message::Checked(f, t, result),
        )
    }

    /// Why game file actions are off right now, if they are.
    fn game_blocker(&self, ctx: &Context) -> Option<&'static str> {
        if self.pending.is_some() {
            return Some(if self.stuck {
                words::STUCK_BLOCKS
            } else {
                words::PENDING_BLOCKS
            });
        }
        self.running_blocker(ctx)
    }

    /// Which of the game and the launcher must close first. Unknown counts as running
    /// unless the shell is unlocked.
    fn running_blocker(&self, ctx: &Context) -> Option<&'static str> {
        let running = |state: RunState| {
            state == RunState::Running || (state == RunState::Unknown && !ctx.unlocked)
        };
        words::blocked_text(running(ctx.game), running(ctx.launcher))
    }

    fn can_copy(&self, ctx: &Context) -> bool {
        self.surveyed
            && self.pair == Pair::Allowed
            && self.game_blocker(ctx).is_none()
            && !self.locked()
    }

    fn swap_target_ready(&self) -> bool {
        self.facts_of(self.to.as_deref())
            .is_some_and(|facts| facts.has_game_files)
    }

    fn can_swap(&self, ctx: &Context) -> bool {
        self.can_copy(ctx) && self.swap_target_ready() && matches!(self.copy, CopyStage::Idle)
    }

    fn plan_copy(&mut self, ctx: &Context) -> Action<Message> {
        let (Some(install), Some(from), Some(to)) =
            (ctx.install.clone(), self.from.clone(), self.to.clone())
        else {
            return Action::none();
        };
        if !self.can_copy(ctx) {
            return Action::none();
        }
        self.swap = SwapStage::Idle;
        self.copy = CopyStage::Planning;
        let options = self.options;
        let (f, t) = (from.clone(), to.clone());
        Action::task(work::background(
            move || jobs::plan_copy(&install, &from, &to, options),
            move |result| Message::CopyPlanned(f, t, options.compare_contents, result),
        ))
    }

    fn start_copy(&mut self, ctx: &Context) -> Action<Message> {
        let CopyStage::Planned(plan) = &self.copy else {
            return Action::none();
        };
        let has_work = plan.files.iter().any(|file| file.copy);
        let room = !plan.needs_in_place || self.options.in_place;
        if !has_work || !room || !self.can_copy(ctx) {
            return Action::none();
        }
        let plan = plan.clone();
        let run = self.next_run;
        self.next_run += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        let options = CopyOptions {
            in_place: plan.needs_in_place && self.options.in_place,
            ..self.options
        };
        let task = jobs::copy_task(
            run,
            (*plan).clone(),
            options,
            probe_for(ctx),
            cancel.clone(),
        );
        self.copy = CopyStage::Running(Box::new(Running {
            run,
            plan,
            in_place: options.in_place,
            progress: None,
            cancel,
            stopping: false,
        }));
        Action::task(task)
    }

    fn copied(&mut self, run: u64, result: Result<CopyOutcome, String>) -> Action<Message> {
        let CopyStage::Running(running) = &self.copy else {
            return Action::none();
        };
        if running.run != run {
            return Action::none();
        }
        let target = folder_name(&running.plan.target);
        let total = running.plan.files.iter().filter(|file| file.copy).count();
        self.notice = Some(match result {
            Ok(outcome) if outcome.cancelled => Notice {
                kind: ToastKind::Warning,
                text: words::copy_stopped(&target, &outcome, total, running.in_place),
            },
            Ok(_) => Notice {
                kind: ToastKind::Success,
                text: words::copy_finished(&target),
            },
            Err(why) => Notice {
                kind: ToastKind::Error,
                text: why,
            },
        });
        self.copy = CopyStage::Idle;
        Action::rescan()
    }

    fn plan_swap(&mut self, ctx: &Context) -> Action<Message> {
        let (Some(install), Some(from), Some(to)) =
            (ctx.install.clone(), self.from.clone(), self.to.clone())
        else {
            return Action::none();
        };
        let travel = self.swap_travel;
        let (f, t) = (from.clone(), to.clone());
        Action::task(work::background(
            move || jobs::plan_swap(&install, &from, &to, travel),
            move |result| Message::SwapPlanned(f, t, travel, result),
        ))
    }

    fn swap(&mut self, ctx: &Context) -> Action<Message> {
        let SwapStage::Confirming { journal, travel } = &self.swap else {
            return Action::none();
        };
        let Some(install) = ctx.install.clone() else {
            return Action::none();
        };
        if *travel != self.swap_travel || !self.can_swap(ctx) {
            return Action::none();
        }
        let journal = (**journal).clone();
        let (a, b) = (journal.a.clone(), journal.b.clone());
        let dir = journal_dir(&ctx.dirs);
        let probe = probe_for(ctx);
        let shown = ctx.display_path(&install.root);
        self.swap = SwapStage::Swapping;
        Action::task(work::background(
            move || jobs::run_swap(journal, &dir, &install, probe.as_ref(), &shown),
            move |result| Message::Swapped(a, b, result),
        ))
    }

    fn swapped(&mut self, a: &str, b: &str, result: Result<(), SwapFailure>) -> Action<Message> {
        self.swap = SwapStage::Idle;
        self.notice = Some(match result {
            Ok(()) => Notice {
                kind: ToastKind::Success,
                text: words::swap_finished(a, b),
            },
            Err(SwapFailure::Undone(why)) => Notice {
                kind: ToastKind::Error,
                text: format!("{SWAP_UNDONE} {why}"),
            },
            Err(SwapFailure::PartWay(why)) => Notice {
                kind: ToastKind::Error,
                text: format!("{SWAP_STOPPED} {why}"),
            },
        });
        Action::rescan()
    }

    fn resume(&mut self, ctx: &Context, pending: PendingSwap, how: Resume) -> Action<Message> {
        if self.resuming || self.stuck {
            return Action::none();
        }
        self.resuming = true;
        let probe = probe_for(ctx);
        let job = pending.clone();
        Action::task(work::background(
            move || jobs::resume(job, how, probe.as_ref()),
            move |result| Message::Resumed(how, Box::new(pending), result),
        ))
    }

    fn resumed(
        &mut self,
        ctx: &Context,
        how: Resume,
        pending: PendingSwap,
        result: Result<(), ResumeFailure>,
    ) -> Action<Message> {
        self.resuming = false;
        let (a, b) = (pending.journal.a.as_str(), pending.journal.b.as_str());
        match result {
            Ok(()) => {
                let text = match how {
                    Resume::Finish => words::swap_finished(a, b),
                    Resume::Undo => words::swap_undone(a, b),
                };
                self.pending = None;
                self.notice = Some(Notice {
                    kind: ToastKind::Success,
                    text: text.clone(),
                });
                Action::rescan().with_toast(Toast::success(text))
            }
            Err(ResumeFailure::CannotTell) => {
                self.stuck = true;
                self.notice = Some(Notice {
                    kind: ToastKind::Error,
                    text: cannot_tell_text(ctx, &pending.journal),
                });
                self.pending = Some(pending);
                Action::toast(Toast::error(CANNOT_TELL))
            }
            Err(ResumeFailure::Other(why)) => {
                self.notice = Some(Notice {
                    kind: ToastKind::Error,
                    text: why.clone(),
                });
                Action::toast(Toast::error(why)).with_task(self.reload(ctx))
            }
        }
    }

    fn ticked_parts(&self) -> Vec<Part> {
        PARTS
            .iter()
            .zip(self.ticked)
            .filter(|(_, on)| *on)
            .map(|(part, _)| *part)
            .collect()
    }

    /// Why the settings buttons are off, if they are.
    fn settings_blocker(&self, ctx: &Context) -> Option<String> {
        let (from, to) = (self.from.as_deref()?, self.to.as_deref()?);
        if ctx.read_only() {
            return Some(words::SETTINGS_BLOCKED.to_string());
        }
        if from.eq_ignore_ascii_case(to) {
            return Some(words::SAME_CHANNEL.to_string());
        }
        for name in [from, to] {
            if self.channel(Some(name)).is_none() {
                return Some(format!(
                    "{name} is not installed, so it has no settings yet."
                ));
            }
        }
        if self.ticked_parts().is_empty() {
            return Some(words::NO_PARTS.to_string());
        }
        None
    }

    fn can_settings(&self, ctx: &Context) -> bool {
        self.surveyed && !self.settings_busy && self.settings_blocker(ctx).is_none()
    }

    fn ask_settings(&mut self, ctx: &Context, swap: bool) -> Action<Message> {
        if !self.can_settings(ctx) {
            return Action::none();
        }
        let (Some(from), Some(to)) = (self.from.clone(), self.to.clone()) else {
            return Action::none();
        };
        let parts = self.ticked_parts();
        let list: Vec<&str> = parts.iter().map(|part| words::part_label(*part)).collect();
        let n = words::parts_text(parts.len());
        let confirm = if swap {
            Confirm::new(
                format!("Swap {n} between {from} and {to}?"),
                format!(
                    "{}\n\n{from} gets {to}'s files and {to} gets {from}'s. Both channels' current files are saved in a snapshot first, so you can undo this.",
                    list.join(", ")
                ),
            )
            .action("Swap", ButtonKind::Primary, Message::SwapSettingsConfirmed)
        } else {
            Confirm::new(
                format!("Copy {n} from {from} to {to}?"),
                format!(
                    "{}\n\n{to}'s files are replaced with {from}'s. They are saved in a snapshot first, so you can undo this.",
                    list.join(", ")
                ),
            )
            .action("Copy", ButtonKind::Primary, Message::CopySettingsConfirmed)
        };
        Action::confirm(confirm.cancel("Cancel"))
    }

    fn run_settings(&mut self, ctx: &Context, swap: bool) -> Action<Message> {
        if !self.can_settings(ctx) {
            return Action::none();
        }
        let (Some(install), Some(from), Some(to)) = (
            ctx.install.clone(),
            self.channel(self.from.as_deref()),
            self.channel(self.to.as_deref()),
        ) else {
            return Action::none();
        };
        let job = SettingsJob {
            writer: ctx.writer(),
            install,
            from,
            to,
            parts: self.ticked_parts(),
        };
        self.settings_busy = true;
        Action::task(work::background(
            move || if swap { job.swap() } else { job.copy() },
            |toast| Message::SettingsDone(Box::new(toast)),
        ))
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

fn probe_for(ctx: &Context) -> Arc<dyn GameProbe> {
    if ctx.unlocked {
        Arc::new(UnlockedProbe(ctx.probe()))
    } else {
        ctx.probe()
    }
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The standing error for a swap whose folders match none of its steps, naming those folders.
fn cannot_tell_text(ctx: &Context, journal: &SwapJournal) -> String {
    let mut folders: Vec<&PathBuf> = Vec::new();
    for Step::Rename { from, to } in &journal.steps {
        for path in [from, to] {
            if path.parent() == Some(journal.install_root.as_path()) && !folders.contains(&path) {
                folders.push(path);
            }
        }
    }
    let shown: Vec<String> = folders.iter().map(|path| ctx.display_path(path)).collect();
    format!("{CANNOT_TELL}\n{}", shown.join("\n"))
}
