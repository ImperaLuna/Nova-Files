#![forbid(unsafe_code)]

mod naming;
mod picker;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use gpui::{App, Context, SharedString, Subscription, Window};
use nova_files_api::{Event as CoreEvent, ExtractTo, Listing, Operation, Packing};
use nova_files_archives_api::{Command, Event};
use nova_files_shell::actions::RenameSelected;
use nova_files_shell::assets::Icon;
use nova_files_shell::{
    FolderRules, Host, Location, MenuAt, MenuGroup, MenuItem, Part, Placed, RenameWhenMade, Scene,
    SelectionMade, Slots, Views, command_spec,
};

use naming::{archive_name, opens};
use picker::{Choice, Picking};

mod actions {
    #![expect(
        clippy::derive_partial_eq_without_eq,
        reason = "GPUI's actions! macro derives PartialEq only"
    )]
    gpui::actions!(
        nova_files_archives,
        [ExtractHere, ExtractTo, ExtractAll, Compress, CompressAs]
    );
}

pub use actions::{Compress, CompressAs, ExtractAll, ExtractHere, ExtractTo as ExtractToFolder};

command_spec!(
    EXTRACT_HERE,
    ExtractHere,
    "file.extract-here",
    "Extract here",
    []
);
command_spec!(
    EXTRACT_TO,
    ExtractToFolder,
    "file.extract-to",
    "Extract to…",
    []
);
command_spec!(
    EXTRACT_ALL,
    ExtractAll,
    "file.extract-all",
    "Extract all",
    []
);
command_spec!(COMPRESS, Compress, "file.compress", "Compress", []);
command_spec!(
    COMPRESS_AS,
    CompressAs,
    "file.compress-as",
    "Compress as…",
    []
);

const PICK_TITLE: &str = "Extract to";
const PICK_LABEL: &str = "Extract";
const AFTER_RENAME: u8 = Placed::SECOND + 1;
const OWNER: &str = "archives";
const INSIDE: FolderRules = FolderRules {
    read_only: true,
    views: Views::Any,
    replaces_menu: false,
    trashed: false,
};

pub struct ArchivesPart {
    host: Host,
    seven: bool,
    made: RenameWhenMade,
    folder: Option<PathBuf>,
    inside: Option<PathBuf>,
    choice: Choice,
    picking: Option<Picking>,
    _selections: Subscription,
}

impl ArchivesPart {
    fn what(&self, cx: &App) -> SharedString {
        let entries = self.host.entries.read(cx);
        let names: Vec<&OsStr> = entries
            .selected()
            .iter()
            .map(|entry| entry.name.as_os_str())
            .collect();
        picker::what(&names)
    }

    fn answered(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Formats { seven } => self.seven = seven,
            Event::ArchiveOf { folder, archive } => {
                if self.folder.as_ref() != Some(&folder) {
                    return;
                }
                match &archive {
                    Some(archive) => self.host.rules.claim(OWNER, archive.clone(), INSIDE),
                    None => self.host.rules.give_up(OWNER),
                }
                self.inside = archive;
            }
        }
        cx.notify();
    }

    fn archives(&self, cx: &App) -> Vec<PathBuf> {
        let entries = self.host.entries.read(cx);
        let (Some(listing), selected) = (entries.listing(), entries.selected()) else {
            return Vec::new();
        };
        if selected.is_empty() || !selected.iter().all(|entry| opens(entry, self.seven)) {
            return Vec::new();
        }
        selected
            .iter()
            .map(|entry| listing.path_of(entry))
            .collect()
    }

    fn extract(&self, archives: Vec<PathBuf>, into: Option<&Path>, cx: &mut App) {
        for archive in archives {
            let Some(into) = into.or_else(|| archive.parent()).map(PathBuf::from) else {
                continue;
            };
            let operation = Operation::Extract {
                archive,
                into,
                to: ExtractTo::Here,
            };
            self.host
                .island
                .update(cx, |island, cx| island.start_job(operation, cx));
        }
    }

    fn extract_here(&mut self, _: &ExtractHere, _: &mut Window, cx: &mut Context<Self>) {
        let archives = self.archives(cx);
        self.extract(archives, None, cx);
    }

    fn extract_all(&mut self, _: &ExtractAll, _: &mut Window, cx: &mut Context<Self>) {
        let archives = self.inside.iter().cloned().collect();
        self.extract(archives, None, cx);
    }

    fn extract_to(&mut self, _: &ExtractToFolder, _: &mut Window, cx: &mut Context<Self>) {
        let archives = self.archives(cx);
        if archives.is_empty() {
            return;
        }
        let part = cx.entity().downgrade();
        self.host.nav.update(cx, |nav, cx| {
            nav.pick_folder(
                PICK_TITLE,
                PICK_LABEL,
                move |folder, cx| {
                    part.update(cx, |part, cx| {
                        part.extract(archives.clone(), Some(&folder), cx);
                    })
                    .ok();
                },
                cx,
            );
        });
    }

    fn compress(&mut self, _: &Compress, window: &mut Window, cx: &mut Context<Self>) {
        let packing = self.choice.packing(self.seven);
        let level = self.choice.level.number();
        self.make((packing, level, false), window, cx);
    }

    fn compress_as(&mut self, _: &CompressAs, _: &mut Window, cx: &mut Context<Self>) {
        self.picking = Some(Picking {
            choice: self.choice,
            open: false,
        });
        cx.notify();
    }

    fn pick(&mut self, change: impl FnOnce(&mut Choice), cx: &mut Context<Self>) {
        if let Some(picking) = self.picking.as_mut() {
            change(&mut picking.choice);
        }
        cx.notify();
    }

    fn compress_picked(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(picked) = self.picking.take() else {
            return;
        };
        self.choice = picked.choice;
        let packing = self.choice.packing(self.seven);
        let level = self.choice.level.number();
        let locked = self.choice.locked(self.seven);
        self.make((packing, level, locked), window, cx);
        cx.notify();
    }

    fn make(
        &mut self,
        (packing, level, locked): (Packing, u8, bool),
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let scene = self.host.scene(cx);
        let Some(into) = writable_folder(&scene) else {
            return;
        };
        let entries = self.host.entries.read(cx);
        let Some(listing) = entries.listing().filter(|listing| listing.path == into) else {
            return;
        };
        let selected = entries.selected();
        let Some(first) = selected.first() else {
            return;
        };
        let sources = selected
            .iter()
            .map(|entry| listing.path_of(entry))
            .collect();
        let Some(name) = archive_name(first, packing, |name| taken(listing, name)) else {
            return;
        };
        self.made.wait(&self.host, into.join(&name), window, cx);
        let operation = Operation::Compress {
            sources,
            into,
            name,
            packing,
            level,
            locked,
        };
        self.host
            .island
            .update(cx, |island, cx| island.start_job(operation, cx));
    }

    fn watch(&mut self, event: &CoreEvent, _: &mut Context<Self>) {
        self.made.watch(event);
    }

    fn listed(&mut self, listing: &Listing, _: bool, cx: &mut Context<Self>) {
        if self.folder.as_ref() != Some(&listing.path) {
            self.folder = Some(listing.path.clone());
            self.picking = None;
            self.inside = None;
            let folder = listing.path.clone();
            self.host.link.send(Command::ArchiveOf { folder }).ok();
        }
        let Some(made) = self.made.listed(listing) else {
            return;
        };
        cx.defer(move |cx| {
            made.update(cx, |_, window, cx| {
                window.dispatch_action(Box::new(RenameSelected), cx);
            })
            .ok();
        });
    }
}

fn taken(listing: &Listing, name: &OsStr) -> bool {
    listing.entries.iter().any(|entry| entry.name == name)
}

fn writable_folder(scene: &Scene) -> Option<PathBuf> {
    if scene.picker || scene.searching || scene.rules.read_only {
        return None;
    }
    scene
        .location
        .as_ref()
        .and_then(Location::folder)
        .map(PathBuf::from)
}

fn has_archives(part: &ArchivesPart, scene: &Scene, cx: &App) -> bool {
    !scene.picker && !scene.rules.read_only && !part.archives(cx).is_empty()
}

const fn in_archive(part: &ArchivesPart, scene: &Scene, _: &App) -> bool {
    !scene.picker && part.inside.is_some()
}

fn extract_all_chip(part: &ArchivesPart, scene: &Scene, cx: &App) -> Vec<MenuItem> {
    if !in_archive(part, scene, cx) {
        return Vec::new();
    }
    vec![MenuItem::run(Icon::FileArchive, &EXTRACT_ALL).with_selector("extract-all")]
}

fn can_compress(_: &ArchivesPart, scene: &Scene, _: &App) -> bool {
    scene.selected > 0 && writable_folder(scene).is_some()
}

fn extract_items(part: &ArchivesPart, scene: &Scene, cx: &App) -> Vec<MenuItem> {
    if scene.rules.replaces_menu || !has_archives(part, scene, cx) {
        return Vec::new();
    }
    vec![
        MenuItem::run(Icon::FileArchive, &EXTRACT_HERE),
        MenuItem::run(Icon::FileArchive, &EXTRACT_TO),
    ]
}

fn compress_item(part: &ArchivesPart, scene: &Scene, cx: &App) -> Vec<MenuItem> {
    if scene.rules.replaces_menu || !can_compress(part, scene, cx) {
        return Vec::new();
    }
    vec![
        MenuItem::run(Icon::FileArchive, &COMPRESS),
        MenuItem::run(Icon::FileArchive, &COMPRESS_AS),
    ]
}

impl Part for ArchivesPart {
    fn open(host: &Host, cx: &mut Context<Self>) -> Self {
        host.link.send(Command::Formats).ok();
        let selections = cx.subscribe(&host.entries, |part, entries, _: &SelectionMade, cx| {
            if entries.read(cx).selected().is_empty() {
                part.picking = None;
            }
        });
        Self {
            host: host.clone(),
            seven: false,
            made: RenameWhenMade::default(),
            folder: None,
            inside: None,
            choice: Choice::default(),
            picking: None,
            _selections: selections,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(Self::answered);
        slots.sends::<Command>();
        slots.command(&EXTRACT_HERE, has_archives, Self::extract_here);
        slots.command(&EXTRACT_TO, has_archives, Self::extract_to);
        slots.command(&EXTRACT_ALL, in_archive, Self::extract_all);
        slots.menu(MenuAt::PathBarEnd, extract_all_chip);
        slots.command(&COMPRESS, can_compress, Self::compress);
        slots.command(&COMPRESS_AS, can_compress, Self::compress_as);
        slots.menu_in(
            MenuAt::Entry,
            MenuGroup::Archive,
            Placed::FIRST,
            extract_items,
        );
        slots.menu_in(MenuAt::Entry, MenuGroup::Edit, AFTER_RENAME, compress_item);
        slots.listed(Self::listed);
        slots.island(picker::picking, picker::island);
        slots.watch(Self::watch);
    }
}
