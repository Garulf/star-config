use super::backups::when;
use super::units;
use crate::context::Context;
use crate::screen::{Action, Confirm, Screen, Toast, ToastKind};
use crate::theme::{self, ButtonKind, Tone, layout, space};
use crate::widgets::{
    banner, button, cell, checkbox, display, empty_state, hint, muted, page, page_fixed, rows,
    section, strong, table_header, table_row, tag,
};
use crate::work;
use iced::widget::{Column, Space, column, container, row};
use iced::{Alignment, Element, Length, Padding, Task};
use sc_core::error::Error;
use sc_core::install::{Install, resolve_ci};
use sc_core::process::{GameProbe, RunState};
use sc_core::shaders::{self, Cache, CacheKind, ShaderBackup, ShaderBackups, Tag};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SUBTITLE: &str = "Shader caches the game rebuilds on its own.";
const SAFE_CAPTION: &str = "Clearing a cache is safe. The game rebuilds it the next time it starts, which makes the first launch slower.";
const UNMATCHED_HINT: &str =
    "No installed channel matches this version. It may be left over from an older build.";
const SAME_DISK_CAPTION: &str = "A backup on the same disk frees no space. Only Clear does.";
const INTERRUPTED: &str = "This backup was interrupted and may be missing files.";
const BLOCKED: &str = "Close the game to change shader caches.";
const CALCULATING: &str = "Calculating...";
const UNVERSIONED: &str = "unversioned";
const STAR_CITIZEN_DIR: &str = "Star Citizen";

const TICK_WIDTH: Length = Length::Fixed(layout::CHECKBOX_SIZE + space::SM);
const CACHE_COLUMNS: [(&str, Length); 6] = [
    ("", TICK_WIDTH),
    ("Game version", Length::FillPortion(5)),
    ("Kind", Length::FillPortion(2)),
    ("Size", Length::FillPortion(2)),
    ("Last modified", Length::FillPortion(2)),
    ("", Length::FillPortion(2)),
];
const LAYER_COLUMNS: [(&str, Length); 6] = [
    ("", TICK_WIDTH),
    ("Channel", Length::FillPortion(5)),
    ("Kind", Length::FillPortion(2)),
    ("Size", Length::FillPortion(2)),
    ("Last modified", Length::FillPortion(2)),
    ("", Length::FillPortion(2)),
];

/// What the screen lists, read off the UI thread.
#[derive(Debug, Clone)]
pub struct Inventory {
    install: PathBuf,
    caches: Vec<Cache>,
    backups: Vec<ShaderBackup>,
}

/// The result of acting on several caches in a row, stopping at the first failure.
#[derive(Debug, Clone)]
pub struct Outcome {
    done: usize,
    asked: usize,
    freed: u64,
    same_disk: bool,
    failure: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Busy {
    Clearing(usize),
    BackingUp(usize),
    Restoring,
    Deleting,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Box<Inventory>),
    Tick(PathBuf, bool),
    BackUpAndClear,
    BackedUp(Outcome),
    Clear,
    ClearConfirmed,
    Cleared(Outcome),
    Restore(Box<ShaderBackup>),
    RestoreConfirmed(Box<ShaderBackup>),
    Restored(Result<String, String>),
    DeleteBackup(Box<ShaderBackup>),
    DeleteConfirmed(Box<ShaderBackup>),
    Deleted(Result<(String, u64), String>),
}

pub struct State {
    local_appdata: Option<PathBuf>,
    loaded: bool,
    loading: bool,
    caches: Vec<Cache>,
    backups: Vec<ShaderBackup>,
    backups_title: String,
    ticked: BTreeSet<PathBuf>,
    busy: Option<Busy>,
}

impl State {
    pub fn new() -> Self {
        Self::with_local_appdata(local_appdata())
    }

    /// Where a native Windows install keeps its caches. Wine prefixes carry their own.
    pub fn with_local_appdata(local_appdata: Option<PathBuf>) -> Self {
        Self {
            local_appdata,
            loaded: false,
            loading: false,
            caches: Vec::new(),
            backups: Vec::new(),
            backups_title: backups_title(&[]),
            ticked: BTreeSet::new(),
            busy: None,
        }
    }

    pub fn reload(&mut self, ctx: &Context) -> Task<Message> {
        let Some(install) = ctx.install.clone() else {
            return Task::none();
        };
        self.loading = true;
        let local = self.local_appdata.clone();
        let store = store_root(ctx);
        work::background(
            move || inventory(&install, local.as_deref(), store),
            |inventory| Message::Loaded(Box::new(inventory)),
        )
    }

    pub fn update(&mut self, message: Message, ctx: &Context) -> Action<Message> {
        match message {
            Message::Loaded(inventory) => {
                if ctx.install.as_ref().map(|install| &install.root) == Some(&inventory.install) {
                    self.take(*inventory);
                }
                Action::none()
            }
            Message::Tick(path, on) => {
                if on {
                    self.ticked.insert(path);
                } else {
                    self.ticked.remove(&path);
                }
                Action::none()
            }
            Message::BackUpAndClear => self.back_up_and_clear(ctx),
            Message::BackedUp(outcome) => self.finish(ctx, backed_up_toast(&outcome)),
            Message::Clear => self.ask_clear(ctx),
            Message::ClearConfirmed => self.clear(ctx),
            Message::Cleared(outcome) => self.finish(ctx, cleared_toast(&outcome)),
            Message::Restore(backup) => self.ask_restore(*backup, ctx),
            Message::RestoreConfirmed(backup) => self.restore(*backup, ctx),
            Message::Restored(result) => self.finish(
                ctx,
                match result {
                    Ok(version) => Toast::success(format!("Restored the {version} shader cache")),
                    Err(err) => Toast::error(format!("Could not restore the backup. {err}")),
                },
            ),
            Message::DeleteBackup(backup) => self.ask_delete(*backup, ctx),
            Message::DeleteConfirmed(backup) => self.delete(*backup, ctx),
            Message::Deleted(result) => self.finish(
                ctx,
                match result {
                    Ok((version, size)) => Toast::success(format!(
                        "Deleted the {version} shader backup, freed {}",
                        units::bytes(size)
                    )),
                    Err(err) => Toast::error(format!("Could not delete the backup. {err}")),
                },
            ),
        }
    }

    pub fn view<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let title = Screen::Maintenance.label();
        if self.loaded && self.caches.is_empty() && self.backups.is_empty() {
            return page_fixed(
                title,
                Some(SUBTITLE),
                Vec::new(),
                empty_state(
                    "No shader caches found",
                    "They appear after the game has run once.",
                    None,
                ),
            );
        }
        let mut body = Column::new().spacing(space::XL);
        if blocked(ctx) {
            body = body.push(banner(ToastKind::Warning, BLOCKED, None));
        }
        body = body.push(self.summary()).push(self.caches_section(ctx));
        if self.layer_caches().next().is_some() {
            body = body.push(self.layers_section());
        }
        body = body.push(self.backups_section(ctx));
        page(title, Some(SUBTITLE), Vec::new(), body)
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

    fn take(&mut self, inventory: Inventory) {
        let mut caches = inventory.caches;
        caches.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
        self.ticked
            .retain(|path| caches.iter().any(|cache| &cache.path == path));
        self.caches = caches;
        self.backups_title = backups_title(&inventory.backups);
        self.backups = inventory.backups;
        self.loaded = true;
        self.loading = false;
    }

    fn can_act(&self, ctx: &Context) -> bool {
        self.busy.is_none() && !blocked(ctx) && self.loaded
    }

    fn selected(&self) -> Vec<Cache> {
        self.caches
            .iter()
            .filter(|cache| self.ticked.contains(&cache.path))
            .cloned()
            .collect()
    }

    fn folder_caches(&self) -> impl Iterator<Item = &Cache> {
        self.caches.iter().filter(|cache| is_folder(cache.kind))
    }

    fn layer_caches(&self) -> impl Iterator<Item = &Cache> {
        self.caches.iter().filter(|cache| !is_folder(cache.kind))
    }

    fn back_up_and_clear(&mut self, ctx: &Context) -> Action<Message> {
        let chosen = self.selected();
        if chosen.is_empty() || !self.can_act(ctx) {
            return Action::none();
        }
        self.busy = Some(Busy::BackingUp(chosen.len()));
        let store = ShaderBackups::new(store_root(ctx));
        let probe = ctx.probe();
        Action::task(work::background(
            move || back_up_all(&chosen, &store, probe),
            Message::BackedUp,
        ))
    }

    fn ask_clear(&mut self, ctx: &Context) -> Action<Message> {
        let chosen = self.selected();
        if chosen.is_empty() || !self.can_act(ctx) {
            return Action::none();
        }
        let size: u64 = chosen.iter().map(|cache| cache.size).sum();
        let paths: Vec<String> = chosen
            .iter()
            .map(|cache| ctx.display_path(&cache.path))
            .collect();
        Action::confirm(
            Confirm::new(
                format!(
                    "Clear {} ({})?",
                    units::count(chosen.len(), "shader cache", "shader caches"),
                    units::bytes(size)
                ),
                format!("{}\n\nThis cannot be undone.", paths.join("\n")),
            )
            .action("Clear", ButtonKind::Danger, Message::ClearConfirmed)
            .cancel("Cancel"),
        )
    }

    fn clear(&mut self, ctx: &Context) -> Action<Message> {
        let chosen = self.selected();
        if chosen.is_empty() || !self.can_act(ctx) {
            return Action::none();
        }
        self.busy = Some(Busy::Clearing(chosen.len()));
        let probe = ctx.probe();
        Action::task(work::background(
            move || clear_all(&chosen, probe),
            Message::Cleared,
        ))
    }

    fn ask_restore(&mut self, backup: ShaderBackup, ctx: &Context) -> Action<Message> {
        if !backup.complete || !self.can_act(ctx) {
            return Action::none();
        }
        let replaces = self
            .caches
            .iter()
            .any(|cache| cache.kind == backup.kind && version_key(cache) == backup.version);
        if !replaces {
            return self.restore(backup, ctx);
        }
        Action::confirm(
            Confirm::new(
                format!("Restore the {} backup?", backup.version),
                "The current cache for this version is replaced.",
            )
            .action(
                "Restore",
                ButtonKind::Primary,
                Message::RestoreConfirmed(Box::new(backup)),
            )
            .cancel("Cancel"),
        )
    }

    fn restore(&mut self, backup: ShaderBackup, ctx: &Context) -> Action<Message> {
        let Some(install) = ctx.install.clone() else {
            return Action::none();
        };
        if !backup.complete || !self.can_act(ctx) {
            return Action::none();
        }
        self.busy = Some(Busy::Restoring);
        let store = ShaderBackups::new(store_root(ctx));
        let local = self.local_appdata.clone();
        let probe = ctx.probe();
        Action::task(work::background(
            move || restore_one(&backup, &store, &install, local.as_deref(), probe),
            Message::Restored,
        ))
    }

    fn ask_delete(&mut self, backup: ShaderBackup, ctx: &Context) -> Action<Message> {
        if !self.can_act(ctx) {
            return Action::none();
        }
        Action::confirm(
            Confirm::new(
                format!("Delete the {} backup?", backup.version),
                format!(
                    "{}, backed up {}, {}. This cannot be undone.",
                    kind_label(backup.kind),
                    when::listed_inline(backup.created_unix),
                    units::bytes(backup.size)
                ),
            )
            .action(
                "Delete",
                ButtonKind::Danger,
                Message::DeleteConfirmed(Box::new(backup)),
            )
            .cancel("Cancel"),
        )
    }

    fn delete(&mut self, backup: ShaderBackup, ctx: &Context) -> Action<Message> {
        if !self.can_act(ctx) {
            return Action::none();
        }
        self.busy = Some(Busy::Deleting);
        let store = ShaderBackups::new(store_root(ctx));
        Action::task(work::background(
            move || {
                store
                    .delete(&backup)
                    .map(|()| (backup.version.clone(), backup.size))
                    .map_err(|err| plain(&err))
            },
            Message::Deleted,
        ))
    }

    fn finish(&mut self, ctx: &Context, toast: Toast) -> Action<Message> {
        self.busy = None;
        self.ticked.clear();
        Action::task(self.reload(ctx)).with_toast(toast)
    }

    fn summary(&self) -> Element<'_, Message> {
        let total = if self.loading || !self.loaded {
            CALCULATING.to_string()
        } else {
            units::bytes(self.caches.iter().map(|cache| cache.size).sum())
        };
        container(
            row![
                column![hint("Shader caches in total"), display(total)].spacing(space::XS),
                hint(SAFE_CAPTION).width(Length::Fill),
            ]
            .spacing(space::XXL)
            .align_y(Alignment::Center),
        )
        .padding(space::LG)
        .width(Length::Fill)
        .style(theme::card)
        .into()
    }

    fn caches_section<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let mut body = Column::new().push(self.selection_bar(ctx));
        if !self.loaded {
            body = body.push(padded(muted("Looking for shader caches...")));
            return section("Shader caches", body);
        }
        let folders: Vec<&Cache> = self.folder_caches().collect();
        if folders.is_empty() {
            body = body.push(padded(muted(
                "No shader caches found. They appear after the game has run once.",
            )));
            return section("Shader caches", body);
        }
        let any_unmatched = folders.iter().any(|cache| shows_unmatched(cache));
        body = body.push(table_header(&CACHE_COLUMNS));
        body = body.push(rows(folders.into_iter().map(|cache| self.cache_row(cache))));
        if any_unmatched {
            body = body.push(legend());
        }
        section("Shader caches", body)
    }

    fn layers_section(&self) -> Element<'_, Message> {
        let body = column![
            table_header(&LAYER_COLUMNS),
            rows(self.layer_caches().map(|cache| self.cache_row(cache))),
        ];
        section("Translation layer caches", body)
    }

    fn selection_bar<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let chosen = self.selected();
        let status: Element<'a, Message> = match self.busy {
            Some(busy) => muted(busy_text(busy)).into(),
            None if chosen.is_empty() => muted("Tick the caches to back up or clear.").into(),
            None => strong(format!(
                "{} selected, {}",
                chosen.len(),
                units::bytes(chosen.iter().map(|cache| cache.size).sum())
            ))
            .into(),
        };
        let enabled = !chosen.is_empty() && self.can_act(ctx);
        container(
            row![
                container(status).width(Length::Fill),
                button(
                    "Back up and clear",
                    ButtonKind::Secondary,
                    enabled.then_some(Message::BackUpAndClear),
                ),
                button(
                    "Clear",
                    ButtonKind::Danger,
                    enabled.then_some(Message::Clear)
                ),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        )
        .padding(row_padding())
        .width(Length::Fill)
        .into()
    }

    fn cache_row<'a>(&'a self, cache: &'a Cache) -> Element<'a, Message> {
        let ticked = self.ticked.contains(&cache.path);
        let path = cache.path.clone();
        let size = if self.loading {
            CALCULATING.to_string()
        } else {
            units::bytes(cache.size)
        };
        let status: Element<'a, Message> = match (cache.version.is_some(), cache.tag) {
            (false, _) => Space::new().into(),
            (true, Tag::Current) => tag("Current", Tone::Accent),
            (true, Tag::Unmatched) => tag("Unmatched", Tone::Muted),
        };
        let toggle_path = path.clone();
        table_row(
            vec![
                (
                    checkbox("", ticked, move |on| Message::Tick(toggle_path.clone(), on)).into(),
                    TICK_WIDTH,
                ),
                (cell(version_label(cache)), CACHE_COLUMNS[1].1),
                (cell(kind_label(cache.kind)), CACHE_COLUMNS[2].1),
                (cell(size), CACHE_COLUMNS[3].1),
                (cell(when::listed(cache.modified_unix)), CACHE_COLUMNS[4].1),
                (status, CACHE_COLUMNS[5].1),
            ],
            ticked,
            Some(Message::Tick(path, !ticked)),
        )
    }

    fn backups_section<'a>(&'a self, ctx: &'a Context) -> Element<'a, Message> {
        let mut items: Vec<Element<'a, Message>> = vec![padded(hint(SAME_DISK_CAPTION))];
        if self.backups.is_empty() {
            items.push(padded(muted(
                "No shader backups yet. Back up and clear keeps a copy here.",
            )));
        }
        let can_act = self.can_act(ctx);
        items.extend(
            self.backups
                .iter()
                .map(|backup| backup_row(backup, can_act)),
        );
        section(&self.backups_title, rows(items))
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

fn blocked(ctx: &Context) -> bool {
    ctx.game != RunState::NotRunning
}

fn store_root(ctx: &Context) -> PathBuf {
    ctx.settings
        .shaders
        .backup_dir
        .clone()
        .unwrap_or_else(|| ctx.dirs.shader_backups())
}

/// Where a native install keeps its shader caches.
pub(crate) fn local_appdata() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

/// Every shader cache of `install`, as this screen lists them.
pub(crate) fn caches(install: &Install, local: Option<&Path>) -> Vec<Cache> {
    shaders::discover(install, &shaders::appdata_roots(install, local))
}

fn inventory(install: &Install, local: Option<&Path>, store: PathBuf) -> Inventory {
    Inventory {
        install: install.root.clone(),
        caches: caches(install, local),
        backups: ShaderBackups::new(store).list(),
    }
}

fn clear_all(chosen: &[Cache], probe: Arc<dyn GameProbe>) -> Outcome {
    let mut outcome = Outcome::asked(chosen.len());
    for cache in chosen {
        match shaders::clear(cache, probe.as_ref()) {
            Ok(freed) => outcome.add(freed),
            Err(err) => return outcome.failed(&err),
        }
    }
    outcome
}

fn back_up_all(chosen: &[Cache], store: &ShaderBackups, probe: Arc<dyn GameProbe>) -> Outcome {
    let mut outcome = Outcome::asked(chosen.len());
    outcome.same_disk = true;
    for cache in chosen {
        match store.backup_and_clear(cache, probe.as_ref()) {
            Ok(backup) => {
                if !shaders::same_volume(&backup.path, &cache.root) {
                    outcome.same_disk = false;
                }
                outcome.add(backup.size);
            }
            Err(err) => return outcome.failed(&err),
        }
    }
    outcome
}

fn restore_one(
    backup: &ShaderBackup,
    store: &ShaderBackups,
    install: &Install,
    local: Option<&Path>,
    probe: Arc<dyn GameProbe>,
) -> Result<String, String> {
    let roots = shaders::appdata_roots(install, local);
    let root = cache_root_for(backup, install, &roots).ok_or_else(|| {
        "The folder this cache belongs in was not found. Start the game once, then try again."
            .to_string()
    })?;
    store
        .restore(backup, &root, probe.as_ref())
        .map(|_| backup.version.clone())
        .map_err(|err| plain(&err))
}

fn cache_root_for(backup: &ShaderBackup, install: &Install, roots: &[PathBuf]) -> Option<PathBuf> {
    if is_folder(backup.kind) {
        return roots
            .iter()
            .filter_map(|root| resolve_ci(root, STAR_CITIZEN_DIR))
            .find(|dir| dir.is_dir());
    }
    install
        .channels()
        .into_iter()
        .find(|channel| channel.name == backup.version)
        .map(|channel| channel.dir)
}

impl Outcome {
    fn asked(asked: usize) -> Self {
        Self {
            done: 0,
            asked,
            freed: 0,
            same_disk: false,
            failure: None,
        }
    }

    fn add(&mut self, bytes: u64) {
        self.done += 1;
        self.freed += bytes;
    }

    fn failed(mut self, err: &Error) -> Self {
        self.failure = Some(plain(err));
        self
    }
}

fn plain(err: &Error) -> String {
    match err {
        Error::GameRunning => {
            "Star Citizen is running. Close the game to change shader caches.".into()
        }
        Error::NotFound(_) => "It is no longer there. The list has been refreshed.".into(),
        Error::Io { path, .. } => format!(
            "{} could not be changed. Check that nothing else is using it. ({err})",
            path.display()
        ),
        _ => format!("star-config stopped to keep your files safe: {err}."),
    }
}

fn cleared_toast(outcome: &Outcome) -> Toast {
    let done = format!(
        "Cleared {}, freed {}",
        units::count(outcome.done, "cache", "caches"),
        units::bytes(outcome.freed)
    );
    match &outcome.failure {
        None => Toast::success(done),
        Some(failure) => Toast::error(partial(outcome, "Cleared", failure)),
    }
}

fn backed_up_toast(outcome: &Outcome) -> Toast {
    if let Some(failure) = &outcome.failure {
        return Toast::error(partial(outcome, "Backed up and cleared", failure));
    }
    let caches = units::count(outcome.done, "cache", "caches");
    if outcome.same_disk {
        Toast::success(format!(
            "Backed up and cleared {caches}. The backups are on the same disk, so no space was freed."
        ))
    } else {
        Toast::success(format!(
            "Backed up and cleared {caches}, freed {}",
            units::bytes(outcome.freed)
        ))
    }
}

fn partial(outcome: &Outcome, verb: &str, failure: &str) -> String {
    format!(
        "{verb} {} of {}. The next one failed: {failure}",
        outcome.done,
        units::count(outcome.asked, "cache", "caches")
    )
}

fn busy_text(busy: Busy) -> String {
    match busy {
        Busy::Clearing(count) => format!("Clearing {}...", units::count(count, "cache", "caches")),
        Busy::BackingUp(count) => format!(
            "Backing up and clearing {}...",
            units::count(count, "cache", "caches")
        ),
        Busy::Restoring => "Restoring the backup...".into(),
        Busy::Deleting => "Deleting the backup...".into(),
    }
}

fn is_folder(kind: CacheKind) -> bool {
    matches!(kind, CacheKind::Shaders | CacheKind::VulkanShaderCache)
}

fn kind_label(kind: CacheKind) -> &'static str {
    match kind {
        CacheKind::Shaders => "Shaders",
        CacheKind::VulkanShaderCache => "Vulkan",
        CacheKind::DxvkFile => "DXVK",
        CacheKind::Vkd3dFile => "vkd3d-proton",
    }
}

fn version_label(cache: &Cache) -> String {
    cache
        .version
        .clone()
        .unwrap_or_else(|| "Shared by all versions".to_string())
}

fn version_key(cache: &Cache) -> &str {
    cache.version.as_deref().unwrap_or(UNVERSIONED)
}

fn shows_unmatched(cache: &Cache) -> bool {
    cache.version.is_some() && cache.tag == Tag::Unmatched
}

fn sort_key(cache: &Cache) -> (bool, bool, Option<String>, &'static str) {
    (
        !is_folder(cache.kind),
        cache.tag != Tag::Current,
        cache.version.clone(),
        kind_label(cache.kind),
    )
}

fn backups_title(backups: &[ShaderBackup]) -> String {
    let total: u64 = backups.iter().map(|backup| backup.size).sum();
    if backups.is_empty() {
        "Backups".to_string()
    } else {
        format!("Backups ({})", units::bytes(total))
    }
}

fn backup_row(backup: &ShaderBackup, can_act: bool) -> Element<'_, Message> {
    let mut name = row![strong(backup.version.as_str())]
        .spacing(space::SM)
        .align_y(Alignment::Center);
    if !backup.complete {
        name = name.push(tag("Incomplete", Tone::Warning));
    }
    let facts = format!(
        "{}, backed up {}, {}",
        kind_label(backup.kind),
        when::listed_inline(backup.created_unix),
        units::bytes(backup.size)
    );
    let mut text = column![name, hint(facts)].spacing(space::XS);
    if !backup.complete {
        text = text.push(hint(INTERRUPTED));
    }
    let restore = (can_act && backup.complete).then(|| Message::Restore(Box::new(backup.clone())));
    let delete = can_act.then(|| Message::DeleteBackup(Box::new(backup.clone())));
    padded(
        row![
            text.width(Length::Fill),
            button("Restore", ButtonKind::Secondary, restore),
            button("Delete", ButtonKind::DangerGhost, delete),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center),
    )
}

fn legend<'a>() -> Element<'a, Message> {
    container(
        row![tag("Unmatched", Tone::Muted), hint(UNMATCHED_HINT)]
            .spacing(space::SM)
            .align_y(Alignment::Center),
    )
    .padding(row_padding())
    .width(Length::Fill)
    .into()
}

fn row_padding() -> Padding {
    Padding {
        top: space::MD,
        right: space::LG,
        bottom: space::MD,
        left: space::LG,
    }
}

fn padded<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .padding(row_padding())
        .width(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache(version: Option<&str>, kind: CacheKind, tag: Tag) -> Cache {
        Cache {
            path: PathBuf::from("/x"),
            root: PathBuf::from("/"),
            version: version.map(str::to_string),
            kind,
            size: 1,
            modified_unix: 0,
            tag,
        }
    }

    #[test]
    fn current_folders_sort_before_unmatched_and_layer_files_last() {
        let mut caches = [
            cache(Some("LIVE"), CacheKind::DxvkFile, Tag::Current),
            cache(Some("sc-alpha-4.0"), CacheKind::Shaders, Tag::Unmatched),
            cache(Some("sc-alpha-4.1"), CacheKind::Shaders, Tag::Current),
        ];
        caches.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
        let order: Vec<_> = caches.iter().map(|c| c.version.clone().unwrap()).collect();
        assert_eq!(order, ["sc-alpha-4.1", "sc-alpha-4.0", "LIVE"]);
    }

    #[test]
    fn counts_read_naturally() {
        assert_eq!(units::count(1, "cache", "caches"), "1 cache");
        assert_eq!(units::count(2, "cache", "caches"), "2 caches");
    }

    #[test]
    fn unversioned_caches_never_show_unmatched() {
        assert!(!shows_unmatched(&cache(
            None,
            CacheKind::VulkanShaderCache,
            Tag::Unmatched
        )));
        assert!(shows_unmatched(&cache(
            Some("sc-alpha-4.0"),
            CacheKind::Shaders,
            Tag::Unmatched
        )));
    }
}
