#![forbid(unsafe_code)]

mod words;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{App, Context, Modifiers, SharedString, Window};
use nova_files_shell::assets::Icon;
use nova_files_shell::{
    Allow, DropOver, Host, Location, MenuAt, MenuGroup, MenuItem, Part, Placed, Scene, Slots,
    command_spec,
};
use nova_files_transfer_api::{
    Command, CopyJob, DropAskId, DropKind, Event, LinkJob, MoveJob, copy, link, move_into,
};

use crate::words::{CopyWords, LinkWords, MoveWords};

pub use nova_files_shell::actions::{Duplicate, MakeLink};

command_spec!(
    DUPLICATE,
    Duplicate,
    "file.duplicate",
    "Duplicate",
    ["ctrl-d"],
    needs[TakeOut, Change]
);

command_spec!(
    MAKE_LINK,
    MakeLink,
    "file.make-link",
    "Make link",
    ["ctrl-m"],
    needs[Change]
);

pub const MOVE_TO: &str = "Move to";
pub const LINK_IN: &str = "Link in";
pub const COPY_TO: &str = "Copy to";
const ITEMS: &str = "items";
const ROOT: &str = "/";

pub struct TransferPart {
    host: Host,
    drives: RefCell<Drives>,
    waiting: Vec<(Vec<PathBuf>, PathBuf)>,
}

#[derive(Debug, Default)]
struct Drives {
    sources: Vec<PathBuf>,
    kinds: HashMap<PathBuf, Option<DropKind>>,
    asked: HashMap<DropAskId, (Vec<PathBuf>, PathBuf)>,
}

impl Drives {
    fn kind(&mut self, sources: &[PathBuf], into: &Path) -> Lookup {
        if self.sources != sources {
            self.sources = sources.to_vec();
            self.kinds.clear();
        }
        match self.kinds.get(into) {
            Some(Some(kind)) => Lookup::Known(*kind),
            Some(None) => Lookup::Asked,
            None => {
                self.kinds.insert(into.to_path_buf(), None);
                Lookup::Unasked
            }
        }
    }

    fn answered(&mut self, id: DropAskId, kind: DropKind) -> Option<(Vec<PathBuf>, PathBuf)> {
        let (sources, into) = self.asked.remove(&id)?;
        if self.sources == sources {
            self.kinds.insert(into.clone(), Some(kind));
        }
        Some((sources, into))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lookup {
    Known(DropKind),
    Asked,
    Unasked,
}

impl TransferPart {
    fn by_drive(&self, sources: &[PathBuf], into: &Path) -> Option<DropKind> {
        let mut drives = self.drives.borrow_mut();
        match drives.kind(sources, into) {
            Lookup::Known(kind) => Some(kind),
            Lookup::Asked => None,
            Lookup::Unasked => {
                let id = DropAskId(self.host.asks.next());
                let ask = Command::DropWould {
                    id,
                    sources: sources.to_vec(),
                    into: into.to_path_buf(),
                };
                if self.host.link.send(ask).is_ok() {
                    drives
                        .asked
                        .insert(id, (sources.to_vec(), into.to_path_buf()));
                }
                None
            }
        }
    }

    fn kind(&self, sources: &[PathBuf], into: &Path, modifiers: Modifiers) -> Option<DropKind> {
        picked(modifiers).or_else(|| self.by_drive(sources, into))
    }

    fn takes_into(&self, into: &Path) -> bool {
        self.host.rules.for_folder(into).allows(Allow::Change)
    }

    fn aim(&self, over: &DropOver<'_>, _: &App) -> Option<SharedString> {
        let Location::Folder(into) = over.at else {
            return None;
        };
        if !self.takes_into(into) {
            return None;
        }
        let label = self
            .kind(over.sources, into, over.modifiers)
            .map_or_else(|| what(over.sources), |kind| label(kind, over.name));
        Some(label.into())
    }

    fn take(
        &mut self,
        at: Location,
        sources: Vec<PathBuf>,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let Location::Folder(into) = at else {
            return;
        };
        if !self.takes_into(&into) {
            return;
        }
        match self.kind(&sources, &into, modifiers) {
            Some(kind) => self.start(kind, sources, into, cx),
            None => self.waiting.push((sources, into)),
        }
    }

    fn start(&self, kind: DropKind, sources: Vec<PathBuf>, into: PathBuf, cx: &mut App) {
        let operation = match kind {
            DropKind::Copy => copy(sources, into),
            DropKind::Move => move_into(sources, into),
            DropKind::Link => link(sources, into),
        };
        self.host
            .island
            .update(cx, |island, cx| island.start_job(operation, cx));
    }

    fn duplicate(&mut self, _: &Duplicate, _: &mut Window, cx: &mut Context<Self>) {
        let paths = self.host.entries.read(cx).selected_paths();
        for (into, sources) in by_folder(paths) {
            if self.takes_into(&into) {
                self.start(DropKind::Copy, sources, into, cx);
            }
        }
    }

    fn make_link(&mut self, _: &MakeLink, _: &mut Window, cx: &mut Context<Self>) {
        let paths = self.host.entries.read(cx).selected_paths();
        for (into, sources) in by_folder(paths) {
            if self.takes_into(&into) {
                self.start(DropKind::Link, sources, into, cx);
            }
        }
    }

    fn answered(&mut self, Event::DropWould { id, kind }: Event, cx: &mut Context<Self>) {
        let Some(asked) = self.drives.get_mut().answered(id, kind) else {
            return;
        };
        if let Some(at) = self.waiting.iter().position(|drop| *drop == asked) {
            let (sources, into) = self.waiting.swap_remove(at);
            self.start(kind, sources, into, cx);
        }
        cx.notify();
        cx.refresh_windows();
    }
}

fn by_folder(paths: Vec<PathBuf>) -> Vec<(PathBuf, Vec<PathBuf>)> {
    let mut groups: Vec<(PathBuf, Vec<PathBuf>)> = Vec::new();
    for path in paths {
        let Some(folder) = path.parent().map(Path::to_path_buf) else {
            continue;
        };
        match groups.iter_mut().find(|(into, _)| *into == folder) {
            Some((_, sources)) => sources.push(path),
            None => groups.push((folder, vec![path])),
        }
    }
    groups
}

const fn not_picking(_: &TransferPart, scene: &Scene, _: &App) -> bool {
    !scene.picker
}

fn duplicate_item(_: &TransferPart, scene: &Scene, _: &App) -> Vec<MenuItem> {
    if scene.picker {
        return Vec::new();
    }
    vec![
        MenuItem::run(Icon::Duplicate, &DUPLICATE),
        MenuItem::run(Icon::Link, &MAKE_LINK),
    ]
}

const fn picked(modifiers: Modifiers) -> Option<DropKind> {
    if modifiers.control && modifiers.shift {
        Some(DropKind::Link)
    } else if modifiers.control {
        Some(DropKind::Copy)
    } else if modifiers.shift {
        Some(DropKind::Move)
    } else {
        None
    }
}

fn label(kind: DropKind, name: &str) -> String {
    let verb = match kind {
        DropKind::Move => MOVE_TO,
        DropKind::Copy => COPY_TO,
        DropKind::Link => LINK_IN,
    };
    format!("{verb} {name}")
}

fn what(paths: &[PathBuf]) -> String {
    match paths {
        [one] => one.file_name().map_or_else(
            || ROOT.to_owned(),
            |name| name.to_string_lossy().into_owned(),
        ),
        many => format!("{} {ITEMS}", many.len()),
    }
}

impl Part for TransferPart {
    fn open(host: &Host, _: &mut Context<Self>) -> Self {
        Self {
            host: host.clone(),
            drives: RefCell::default(),
            waiting: Vec::new(),
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.drop_target(Self::aim, Self::take);
        slots.command(&DUPLICATE, not_picking, Self::duplicate);
        slots.command(&MAKE_LINK, not_picking, Self::make_link);
        slots.job_words::<LinkJob>(LinkWords);
        slots.menu_in(
            MenuAt::Entry,
            MenuGroup::Edit,
            Placed::SECOND,
            duplicate_item,
        );
        slots.on_event(Self::answered);
        slots.sends::<Command>();
        slots.job_words::<CopyJob>(CopyWords);
        slots.job_words::<MoveJob>(MoveWords);
    }
}

#[cfg(test)]
mod tests;
