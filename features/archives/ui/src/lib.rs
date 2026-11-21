#![forbid(unsafe_code)]

mod naming;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use gpui::{App, Context, Window};
use nova_files_api::{Event as CoreEvent, ExtractTo, Listing, Operation, Packing};
use nova_files_archives_api::{Command, Event};
use nova_files_shell::actions::RenameSelected;
use nova_files_shell::assets::Icon;
use nova_files_shell::{
    FolderRules, Host, Location, MenuAt, MenuGroup, MenuItem, Part, Placed, RenameWhenMade, Scene,
    Slots, Views, command_spec,
};

use naming::{archive_name, opens};

mod actions {
    #![expect(
        clippy::derive_partial_eq_without_eq,
        reason = "GPUI's actions! macro derives PartialEq only"
    )]
    gpui::actions!(nova_files_archives, [ExtractHere, ExtractTo, Compress]);
}

pub use actions::{Compress, ExtractHere, ExtractTo as ExtractToFolder};

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
command_spec!(COMPRESS, Compress, "file.compress", "Compress", []);

const PICK_TITLE: &str = "Extract to";
const PICK_LABEL: &str = "Extract";
const LEVEL: u8 = 6;
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
}

impl ArchivesPart {
    pub const fn packing(&self) -> Packing {
        if self.seven {
            Packing::SevenZip
        } else {
            Packing::ZipDeflate
        }
    }

    fn answered(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Formats { seven } => self.seven = seven,
            Event::ArchiveOf { folder, archive } => {
                if self.folder.as_ref() != Some(&folder) {
                    return;
                }
                match archive {
                    Some(archive) => self.host.rules.claim(OWNER, archive, INSIDE),
                    None => self.host.rules.give_up(OWNER),
                }
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
        let scene = self.host.scene(cx);
        let Some(into) = writable_folder(&scene) else {
            return;
        };
        let packing = self.packing();
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
            level: LEVEL,
            locked: false,
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
    vec![MenuItem::run(Icon::FileArchive, &COMPRESS)]
}

impl Part for ArchivesPart {
    fn open(host: &Host, _: &mut Context<Self>) -> Self {
        host.link.send(Command::Formats).ok();
        Self {
            host: host.clone(),
            seven: false,
            made: RenameWhenMade::default(),
            folder: None,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(Self::answered);
        slots.sends::<Command>();
        slots.command(&EXTRACT_HERE, has_archives, Self::extract_here);
        slots.command(&EXTRACT_TO, has_archives, Self::extract_to);
        slots.command(&COMPRESS, can_compress, Self::compress);
        slots.menu_in(
            MenuAt::Entry,
            MenuGroup::Archive,
            Placed::FIRST,
            extract_items,
        );
        slots.menu_in(MenuAt::Entry, MenuGroup::Edit, AFTER_RENAME, compress_item);
        slots.listed(Self::listed);
        slots.watch(Self::watch);
    }
}
