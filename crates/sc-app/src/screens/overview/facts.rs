//! What the Overview cards say, read off the UI thread in one pass.

use super::Area;
use crate::screens::backups::when;
use crate::screens::screenshots::library;
use crate::screens::{maintenance, screenshots, units};
use iced::widget::image::Handle;
use sc_core::actionmaps::{ActionMaps, DeviceKind, Slot, parse_input};
use sc_core::attributes::Attributes;
use sc_core::backup::{BackupStore, Scope, SnapshotState};
use sc_core::dirs::AppDirs;
use sc_core::error::Error;
use sc_core::install::{Channel, Install};
use sc_core::shaders::Tag;
use sc_core::staging::{Document, Staged};
use sc_core::starstrings::{self, InstallRecord, Release, Status};
use sc_core::usercfg::UserCfg;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

const THUMBS: usize = 3;
const HEAD_TRACKING: &str = "HeadtrackingToggle";
const FACE_TRACKING: &str = "FoipEnabled";

/// One card's facts: a main figure, up to two supporting lines, and whether it needs the
/// user's attention.
#[derive(Debug, Clone, Default)]
pub struct Card {
    pub figure: String,
    pub lines: Vec<String>,
    pub attention: bool,
    pub thumbs: Vec<Handle>,
}

impl Card {
    fn new(figure: impl Into<String>) -> Self {
        Self {
            figure: figure.into(),
            ..Self::default()
        }
    }

    fn line(mut self, line: impl Into<String>) -> Self {
        self.lines.push(line.into());
        self
    }

    fn attention(mut self, attention: bool) -> Self {
        self.attention = attention;
        self
    }
}

/// The StarStrings install as stored on disk. The card is finished in the view, once the
/// latest release is known from the StarStrings screen.
#[derive(Debug, Clone)]
pub struct StarStrings {
    pub status: Status,
    pub build_changed: bool,
}

/// Everything one load found for the selected channel.
#[derive(Debug, Clone)]
pub struct Facts {
    pub channel: PathBuf,
    pub build: Option<String>,
    pub starstrings: StarStrings,
    pub cards: Vec<(Area, Result<Card, String>)>,
}

/// What the load needs, cloned from the context so it can leave the UI thread.
pub struct Input {
    pub install: Install,
    pub channels: Vec<Channel>,
    pub channel: Channel,
    pub scope: Scope,
    pub dirs: AppDirs,
    pub archive: Option<PathBuf>,
    pub local_appdata: Option<PathBuf>,
}

pub fn load(input: Input) -> Facts {
    let attributes = open::<Attributes>(input.channel.attributes_xml());
    let cards = vec![
        (Area::Config, config(&input.channel, &attributes)),
        (Area::Keybinds, keybinds(&input.channel)),
        (Area::Comms, comms(&attributes)),
        (Area::Screenshots, Ok(screenshots(&input))),
        (Area::Backups, backups(&input.dirs, &input.scope)),
        (Area::Shaders, Ok(shaders(&input))),
        (Area::Channels, Ok(channels(&input.channels))),
    ];
    Facts {
        channel: input.channel.dir.clone(),
        build: input.channel.build_id(),
        starstrings: starstrings_facts(&input),
        cards,
    }
}

/// A parsed file, `None` when it does not exist yet, or why it could not be read.
type Opened<D> = Result<Option<D>, String>;

fn open<D: Document>(path: PathBuf) -> Opened<D> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !path.exists() {
        return Ok(None);
    }
    match Staged::<D>::open(&path) {
        Ok(staged) if staged.loaded().hash.is_none() => Ok(None),
        Ok(staged) => Ok(Some(staged.base().clone())),
        Err(Error::Parse { line, .. }) => Err(format!("{name} has an error on line {line}")),
        Err(_) => Err(format!("{name} could not be opened")),
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{} {many}", units::thousands(count))
    }
}

/// `Today 21:14` as it reads after another word: `today 21:14`.
fn mid_sentence(label: String) -> String {
    match label.strip_prefix("Today") {
        Some(rest) => format!("today{rest}"),
        None => match label.strip_prefix("Yesterday") {
            Some(rest) => format!("yesterday{rest}"),
            None => label,
        },
    }
}

fn when_label(unix: i64) -> String {
    mid_sentence(when::list_label(when::local(unix), when::now()))
}

fn config(channel: &Channel, attributes: &Opened<Attributes>) -> Result<Card, String> {
    let cfg = open::<UserCfg>(channel.user_cfg())?;
    let card = match cfg {
        None => Card::new("No user.cfg yet").line("Apply a change on Config to create it"),
        Some(cfg) => {
            let keys: BTreeSet<String> = cfg
                .entries()
                .into_iter()
                .map(|entry| entry.key.to_lowercase())
                .collect();
            Card::new(format!(
                "{} in user.cfg",
                plural(keys.len(), "setting", "settings")
            ))
        }
    };
    Ok(match attributes {
        Ok(Some(doc)) => card.line(format!(
            "{} in the game's settings file",
            plural(doc.entries().len(), "value", "values")
        )),
        _ => card,
    })
}

fn is_bound(input: &str) -> bool {
    !input.trim().is_empty() && parse_input(input).is_none_or(|(_, rest)| !rest.trim().is_empty())
}

fn keybinds(channel: &Channel) -> Result<Card, String> {
    let Some(doc) = open::<ActionMaps>(channel.actionmaps_xml())? else {
        return Ok(Card::new("No custom binds yet").line("The game uses its default binds"));
    };
    let binds = doc
        .bindings()
        .iter()
        .filter(|binding| is_bound(&binding.input))
        .count();
    let mut slots: BTreeSet<Slot> = doc.devices().iter().map(|device| device.slot).collect();
    if slots.is_empty() {
        slots = doc.bindings().iter().filter_map(|b| b.slot).collect();
    }
    let conflicts = doc.conflicts().len();
    let mut card = Card::new(plural(binds, "custom bind", "custom binds"));
    if !slots.is_empty() {
        card = card.line(devices_line(&slots));
    }
    if conflicts > 0 {
        card = card
            .line(plural(conflicts, "conflict", "conflicts"))
            .attention(true);
    }
    Ok(card)
}

/// `Keyboard, mouse, 2 joysticks`.
pub fn devices_line(slots: &BTreeSet<Slot>) -> String {
    let mut counts: BTreeMap<DeviceKind, usize> = BTreeMap::new();
    for slot in slots {
        *counts.entry(slot.kind).or_default() += 1;
    }
    let words: Vec<String> = counts
        .into_iter()
        .map(|(kind, count)| {
            let (one, many) = match kind {
                DeviceKind::Keyboard => ("keyboard", "keyboards"),
                DeviceKind::Mouse => ("mouse", "mice"),
                DeviceKind::Joystick => ("joystick", "joysticks"),
                DeviceKind::Gamepad => ("gamepad", "gamepads"),
            };
            if count == 1 {
                one.to_string()
            } else {
                format!("{count} {many}")
            }
        })
        .collect();
    let line = words.join(", ");
    let mut chars = line.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => line,
    }
}

fn on_off(value: &str) -> &'static str {
    if value.trim() == "0" { "off" } else { "on" }
}

fn comms(attributes: &Opened<Attributes>) -> Result<Card, String> {
    let Some(doc) = attributes.clone()? else {
        return Ok(Card::new("Not set up").line("The game has not saved its settings yet"));
    };
    let mut card = match doc.get(HEAD_TRACKING) {
        Some(value) => Card::new(format!("Head tracking {}", on_off(value))),
        None => Card::new("Not set up"),
    };
    if let Some(value) = doc.get(FACE_TRACKING) {
        card = card.line(format!("Face tracking {}", on_off(value)));
    }
    Ok(card)
}

fn screenshots(input: &Input) -> Card {
    let shots = sc_core::screenshots::scan(&input.channels, input.archive.as_deref());
    let Some(newest) = shots.first() else {
        return Card::new("No screenshots yet").line("Captures from every channel show here");
    };
    let newest_images: Vec<PathBuf> = shots
        .iter()
        .filter(|shot| shot.kind == sc_core::screenshots::ShotKind::Image)
        .take(THUMBS)
        .map(|shot| shot.path.clone())
        .collect();
    let thumbs = screenshots::make_thumbs(newest_images, &input.dirs.thumbnails())
        .into_iter()
        .filter_map(|(_, handle)| handle)
        .collect();
    Card {
        thumbs,
        ..Card::new(library::screenshots(shots.len()))
            .line(format!("Newest {}", when_label(newest.modified_unix)))
    }
}

fn backups(dirs: &AppDirs, scope: &Scope) -> Result<Card, String> {
    let list = BackupStore::new(dirs.backups())
        .list(scope)
        .map_err(|_| "The backups folder could not be read".to_string())?;
    let Some(last) = list.iter().max_by_key(|info| info.created_unix) else {
        return Ok(Card::new("No snapshots yet").line("One is taken before every change"));
    };
    let interrupted = list
        .iter()
        .filter(|info| info.state == SnapshotState::Applying)
        .count();
    let mut card = Card::new(format!("Last snapshot {}", when_label(last.created_unix)))
        .line(plural(list.len(), "snapshot", "snapshots"));
    if interrupted > 0 {
        card = card
            .line(format!(
                "{} interrupted",
                plural(interrupted, "change was", "changes were")
            ))
            .attention(true);
    }
    Ok(card)
}

fn shaders(input: &Input) -> Card {
    let caches = maintenance::caches(&input.install, input.local_appdata.as_deref());
    if caches.is_empty() {
        return Card::new("No shader caches").line("The game builds them when it starts");
    }
    let total: u64 = caches.iter().map(|cache| cache.size).sum();
    let card = Card::new(units::bytes(total)).line(plural(caches.len(), "cache", "caches"));
    if caches.iter().any(|cache| cache.tag == Tag::Current) {
        card
    } else {
        card.line("The game was updated since these were built")
            .attention(true)
    }
}

fn channels(channels: &[Channel]) -> Card {
    let lines = channels
        .iter()
        .map(|channel| match channel.build_id() {
            Some(build) => format!("{} {build}", channel.name),
            None => channel.name.clone(),
        })
        .collect();
    Card {
        lines,
        ..Card::new(plural(channels.len(), "channel", "channels"))
    }
}

fn starstrings_facts(input: &Input) -> StarStrings {
    let status = starstrings::status(&input.channel, &input.dirs, &input.scope);
    let advice = starstrings::advise(&status, None, &input.channel);
    StarStrings {
        status,
        build_changed: advice.build_changed,
    }
}

fn record(status: &Status) -> Option<&InstallRecord> {
    match status {
        Status::Managed(record) | Status::Modified(record) => Some(record),
        Status::NotInstalled | Status::Foreign => None,
    }
}

/// The StarStrings card from the stored install record, the build id and, when the
/// StarStrings screen has checked, the latest release. Never asks the network.
pub fn starstrings_card(facts: &StarStrings, latest: Option<&Release>) -> Card {
    let Some(record) = record(&facts.status) else {
        return match facts.status {
            Status::Foreign => {
                Card::new("Custom text installed").line("Not installed by star-config")
            }
            _ => Card::new("Not installed").line("Clearer item and mission names"),
        };
    };
    let installed = format!("Installed {}", when_label(record.installed_unix));
    if facts.build_changed {
        return Card::new("Game updated since install")
            .line(installed)
            .attention(true);
    }
    match latest {
        Some(release)
            if release.name == record.release_name
                && release.published_at == record.published_at =>
        {
            Card::new("Up to date").line(installed)
        }
        Some(release) => Card::new("Update available")
            .line(format!(
                "Published {}",
                units::date_of_iso(&release.published_at)
            ))
            .attention(true),
        None => Card::new("Installed").line(installed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::actionmaps::DeviceKind::{Gamepad, Joystick, Keyboard, Mouse};

    fn slots(list: &[(DeviceKind, u32)]) -> BTreeSet<Slot> {
        list.iter()
            .map(|(kind, instance)| Slot {
                kind: *kind,
                instance: *instance,
            })
            .collect()
    }

    #[test]
    fn devices_read_as_a_short_list() {
        assert_eq!(
            devices_line(&slots(&[
                (Keyboard, 1),
                (Mouse, 1),
                (Joystick, 1),
                (Joystick, 2)
            ])),
            "Keyboard, mouse, 2 joysticks"
        );
        assert_eq!(devices_line(&slots(&[(Gamepad, 1)])), "Gamepad");
    }

    #[test]
    fn times_read_mid_sentence() {
        assert_eq!(mid_sentence("Today 21:14".into()), "today 21:14");
        assert_eq!(mid_sentence("Yesterday 08:30".into()), "yesterday 08:30");
        assert_eq!(mid_sentence("14 Nov 19:02".into()), "14 Nov 19:02");
    }

    #[test]
    fn plurals_group_thousands() {
        assert_eq!(plural(1, "bind", "binds"), "1 bind");
        assert_eq!(plural(1204, "bind", "binds"), "1,204 binds");
    }
}
