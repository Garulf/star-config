//! The blocking work behind the Channels screen. Every function here runs inside
//! `work::background`, never on the UI thread.

use super::words::{self, PARTS};
use super::{Message, PendingSwap};
use crate::screen::{Toast, UndoIn};
use crate::screens::backups;
use crate::screens::edits::toast_for;
use crate::work;
use iced::Task;
use iced::futures::channel::mpsc;
use sc_core::backup::{Scope, SnapshotInfo};
use sc_core::channels::{self, CopyOptions, CopyPlan, KNOWN, Part, Progress, Resume, SwapJournal};
use sc_core::error::Error;
use sc_core::guard::Writer;
use sc_core::install::{Channel, Install, resolve_ci};
use sc_core::process::{GameProbe, RunState};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// One channel as the list shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelFacts {
    pub name: String,
    pub installed: bool,
    pub has_game_files: bool,
    pub build: Option<String>,
    pub has_settings: bool,
    pub screenshots: usize,
    /// Existing files per part, in [`PARTS`] order.
    pub parts: [usize; 5],
}

/// What a reload finds, before sizes are known.
#[derive(Debug, Clone)]
pub struct Survey {
    pub install_root: PathBuf,
    pub channels: Vec<Channel>,
    pub facts: Vec<ChannelFacts>,
    pub pending: Option<PendingSwap>,
}

pub fn survey(install: &Install, journal_dir: &Path) -> Survey {
    let channels = install.channels();
    let mut facts: Vec<ChannelFacts> = channels.iter().map(installed_facts).collect();
    for known in KNOWN {
        if !channels
            .iter()
            .any(|channel| channel.name.eq_ignore_ascii_case(known))
        {
            facts.push(missing_facts(known));
        }
    }
    let pending = channels::find_pending_swap(journal_dir, install)
        .ok()
        .flatten()
        .map(|(file, journal)| PendingSwap { file, journal });
    Survey {
        install_root: install.root.clone(),
        channels,
        facts,
        pending,
    }
}

fn installed_facts(channel: &Channel) -> ChannelFacts {
    let parts = PARTS.map(|part| existing(&channels::part_files(channel, part)));
    let settings_parts = parts[..4].iter().sum::<usize>();
    ChannelFacts {
        name: channel.name.clone(),
        installed: true,
        has_game_files: resolve_ci(&channel.dir, "Data.p4k").is_some_and(|p4k| p4k.is_file()),
        build: channel.build_id(),
        has_settings: settings_parts > 0,
        screenshots: files_in(&channel.screenshots_dir()),
        parts,
    }
}

fn missing_facts(name: &str) -> ChannelFacts {
    ChannelFacts {
        name: name.to_string(),
        installed: false,
        has_game_files: false,
        build: None,
        has_settings: false,
        screenshots: 0,
        parts: [0; 5],
    }
}

fn existing(paths: &[PathBuf]) -> usize {
    paths.iter().filter(|path| path.is_file()).count()
}

fn files_in(dir: &Path) -> usize {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
                .count()
        })
        .unwrap_or(0)
}

/// Size on disk of every installed channel, by name.
pub fn measure(channels: &[Channel]) -> Vec<(String, u64)> {
    channels
        .iter()
        .map(|channel| (channel.name.clone(), dir_size(&channel.dir)))
        .collect()
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match fs::symlink_metadata(entry.path()) {
            Ok(meta) if meta.is_dir() => dir_size(&entry.path()),
            Ok(meta) if meta.is_file() => meta.len(),
            _ => 0,
        })
        .sum()
}

/// `validate_pair` with its refusal in plain words.
pub fn check_pair(install: &Install, from: &str, to: &str) -> Result<(), String> {
    if from.eq_ignore_ascii_case(to) {
        return Err(words::SAME_CHANNEL.to_string());
    }
    channels::validate_pair(install, from, to)
        .map(|_| ())
        .map_err(|err| refusal(install, from, &err))
}

fn refusal(install: &Install, from: &str, err: &Error) -> String {
    match resolve_ci(&install.root, from) {
        None => format!("{from} is not installed, so there is nothing to copy from."),
        Some(dir) if resolve_ci(&dir, "Data.p4k").is_none() => {
            format!("{from} has no game files to copy. It holds no Data.p4k.")
        }
        Some(_) => format!(
            "star-config cannot use these two channels. {}",
            sentence(err)
        ),
    }
}

/// The core's message as a sentence: capitalised, ending in a full stop.
pub fn sentence(err: &Error) -> String {
    let text = backups::words(err);
    let mut chars = text.chars();
    let mut out: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    };
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

pub fn plan_copy(
    install: &Install,
    from: &str,
    to: &str,
    options: CopyOptions,
) -> Result<Box<CopyPlan>, String> {
    channels::plan_copy(install, from, to, options)
        .map(Box::new)
        .map_err(|err| {
            format!(
                "star-config could not work out the copy. {}",
                sentence(&err)
            )
        })
}

/// Runs the copy and streams its progress, at most ten updates a second.
pub fn copy_task(
    run: u64,
    plan: CopyPlan,
    options: CopyOptions,
    probe: Arc<dyn GameProbe>,
    cancel: Arc<AtomicBool>,
) -> Task<Message> {
    let (sender, receiver) = mpsc::unbounded();
    let target = plan
        .target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let job = work::background(
        move || {
            let mut last: Option<Instant> = None;
            let mut report = |progress: Progress| {
                if last.is_none_or(|at| at.elapsed() >= PROGRESS_EVERY) {
                    last = Some(Instant::now());
                    let _ = sender.unbounded_send(progress);
                }
            };
            channels::run_copy(&plan, options, probe.as_ref(), &cancel, &mut report)
                .map_err(|err| copy_failure(&err, &target))
        },
        move |result| Message::Copied(run, result),
    );
    let progress = Task::run(receiver, move |progress| {
        Message::CopyProgress(run, progress)
    });
    Task::batch([progress, job])
}

fn copy_failure(err: &Error, target: &str) -> String {
    match err {
        Error::GameRunning => "Close the game and the launcher, then start the copy again.".into(),
        _ => format!(
            "The copy stopped. {} Run Verify on {target} in the launcher before you play.",
            sentence(err)
        ),
    }
}

pub fn plan_swap(
    install: &Install,
    a: &str,
    b: &str,
    travel: bool,
) -> Result<Box<SwapJournal>, String> {
    channels::plan_swap(install, a, b, travel)
        .map(Box::new)
        .map_err(|err| format!("star-config cannot swap these channels. {}", sentence(&err)))
}

/// Why a swap did not happen, and whether the core put everything back.
#[derive(Debug, Clone, PartialEq)]
pub enum SwapFailure {
    /// Nothing was renamed, or every rename was undone.
    Undone(String),
    /// The undo failed too; the journal is still on disk.
    PartWay(String),
}

pub fn run_swap(
    journal: SwapJournal,
    journal_dir: &Path,
    install: &Install,
    probe: &dyn GameProbe,
    root_shown: &str,
) -> Result<(), SwapFailure> {
    let file = channels::journal_path(journal_dir, install);
    let result = fs::create_dir_all(journal_dir)
        .map_err(|err| Error::io(journal_dir, err))
        .and_then(|()| channels::run_swap(journal, &file, probe));
    let Err(err) = result else {
        return Ok(());
    };
    let reason = swap_reason(&err, root_shown);
    if file.exists() {
        Err(SwapFailure::PartWay(reason))
    } else {
        Err(SwapFailure::Undone(reason))
    }
}

fn swap_reason(err: &Error, root_shown: &str) -> String {
    match err {
        Error::GameRunning => "Close the game and the launcher, then try again.".into(),
        Error::Io { source, .. }
            if matches!(
                source.kind(),
                ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem
            ) =>
        {
            words::cannot_rename(root_shown)
        }
        other => sentence(other),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResumeFailure {
    /// The folders match no point of the swap. The journal stays where it is.
    CannotTell,
    Other(String),
}

pub fn resume(
    pending: PendingSwap,
    how: Resume,
    probe: &dyn GameProbe,
) -> Result<(), ResumeFailure> {
    channels::resume_swap(pending.journal, &pending.file, how, probe).map_err(|err| match err {
        Error::Invalid(_) => ResumeFailure::CannotTell,
        Error::GameRunning => ResumeFailure::Other(
            "Close the game and the launcher first, then finish or undo the swap on the Channels screen."
                .into(),
        ),
        other => ResumeFailure::Other(format!(
            "The swap is still unfinished. {} Nothing else was changed.",
            sentence(&other)
        )),
    })
}

/// Treats an unknown launcher or game as closed once the user has unlocked the shell.
pub struct UnlockedProbe(pub Arc<dyn GameProbe>);

impl GameProbe for UnlockedProbe {
    fn game(&self) -> RunState {
        known_or_closed(self.0.game())
    }

    fn launcher(&self) -> RunState {
        known_or_closed(self.0.launcher())
    }
}

fn known_or_closed(state: RunState) -> RunState {
    match state {
        RunState::Unknown => RunState::NotRunning,
        other => other,
    }
}

/// Everything a settings copy or swap needs, cloned out of the screen.
pub struct SettingsJob {
    pub writer: Writer,
    pub install: Install,
    pub from: Channel,
    pub to: Channel,
    pub parts: Vec<Part>,
}

impl SettingsJob {
    fn scope(&self, channel: &Channel) -> Scope {
        Scope {
            install_key: self.install.key(),
            channel: channel.name.clone(),
        }
    }

    fn undo_in(&self, channel: &Channel, info: &SnapshotInfo) -> UndoIn {
        UndoIn {
            scope: self.scope(channel),
            channel: channel.clone(),
            snapshot: info.id.clone(),
        }
    }

    pub fn copy(self) -> Toast {
        let scope = self.scope(&self.to);
        match channels::copy_settings(&self.from, &self.to, &self.parts, &self.writer, &scope) {
            Ok(info) => Toast::success(words::copied_settings(
                self.parts.len(),
                &self.from.name,
                &self.to.name,
            ))
            .with_undo_in(vec![self.undo_in(&self.to, &info)]),
            Err(err) => settings_failure(err),
        }
    }

    pub fn swap(self) -> Toast {
        let (scope_a, scope_b) = (self.scope(&self.from), self.scope(&self.to));
        match channels::swap_settings(
            &self.from,
            &self.to,
            &self.parts,
            &self.writer,
            &scope_a,
            &scope_b,
        ) {
            Ok((first, second)) => Toast::success(words::swapped_settings(
                self.parts.len(),
                &self.from.name,
                &self.to.name,
            ))
            .with_undo_in(vec![
                self.undo_in(&self.from, &first),
                self.undo_in(&self.to, &second),
            ]),
            Err(err) => settings_failure(err),
        }
    }
}

fn settings_failure(err: Error) -> Toast {
    match err {
        Error::GameRunning => Toast::error(
            "Star Citizen is running, so no settings were copied. Close it and try again.",
        ),
        Error::RollbackFailed { .. } => {
            toast_for(&Err(err), 0).unwrap_or_else(|| Toast::error("Copying settings failed."))
        }
        other => Toast::error(format!(
            "Could not copy the settings. {} Nothing was changed.",
            sentence(&other)
        )),
    }
}
