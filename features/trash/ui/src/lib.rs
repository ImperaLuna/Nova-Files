#![forbid(unsafe_code)]

mod words;

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use gpui::{App, Context, Modifiers, SharedString, Window};
use nova_files_api::{Entry, Event, JobId, Operation};
use nova_files_shell::actions::TrashSelected;
use nova_files_shell::assets::Icon;
use nova_files_shell::{
    Column, DropOver, FolderName, FolderRules, Host, Location, Look, MenuAt, MenuGroup, MenuItem,
    Navigator, Part, Placed, Scene, SidebarAt, SidebarRow, Slots, Views, command_spec,
};
use nova_files_trash_api::{
    self as trash_api, DeleteFromTrashJob, EmptyTrashJob, IN_TRASH, NO_TRASH, RestoreJob, TrashJob,
    Trashed,
};

use crate::words::{DeleteFromTrashWords, EmptyTrashWords, RestoreWords, TrashWords};

mod actions {
    #![expect(
        clippy::derive_partial_eq_without_eq,
        reason = "GPUI's actions! macro derives PartialEq only"
    )]
    gpui::actions!(nova_files_trash, [RestoreSelected, EmptyTrash]);
}

pub use actions::{EmptyTrash, RestoreSelected};

command_spec!(
    TRASH,
    TrashSelected,
    "file.trash",
    "Move to Trash",
    ["delete"]
);
command_spec!(RESTORE, RestoreSelected, "file.restore", "Restore", []);
command_spec!(
    EMPTY_TRASH,
    EmptyTrash,
    "file.empty-trash",
    "Empty Trash",
    []
);

pub const TRASH_LABEL: &str = "Trash";
const OWNER: &str = "trash";
const ROW: &str = "place-trash";
const WAS_IN_LABEL: &str = "Was in";
const WAS_IN_HEAD: &str = "head-was-in";
const EMPTY_LABEL: &str = "Empty";
const DELETE_LABEL: &str = "Delete";
const EVERYTHING: &str = "everything";
const ITEM: &str = "item";
const ITEMS: &str = "items";
const HOME: &str = "~";
const MOVE_TO: &str = "Move to";
const NO_TRASH_REASON: &str = "its drive has no trash";
const IN_TRASH_REASON: &str = "it is in the Trash already";

pub struct TrashPart {
    host: Host,
    folder: Option<PathBuf>,
    listed: HashMap<OsString, Trashed>,
    count: Option<usize>,
    touching: HashSet<JobId>,
}

impl TrashPart {
    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }

    pub const fn count(&self) -> Option<usize> {
        self.count
    }

    pub fn open(&self, cx: &mut App) {
        if let Some(folder) = self.folder.clone() {
            self.host
                .nav
                .update(cx, |nav, cx| nav.go(Location::Folder(folder), cx));
        }
    }

    pub fn open_in_new_tab(&self, cx: &mut App) {
        if let Some(folder) = self.folder.clone() {
            self.host.nav.update(cx, |nav, cx| {
                nav.open_in_new_tab(Location::Folder(folder), cx);
            });
        }
    }

    fn shown(&self, scene: &Scene) -> bool {
        self.folder
            .as_deref()
            .is_some_and(|folder| scene.in_folder(folder))
    }

    fn full(&self) -> bool {
        self.count.is_some_and(|count| count > 0)
    }

    fn ask_list(&self) {
        self.host.link.send(trash_api::Command::List).ok();
    }

    fn listed(&mut self, event: trash_api::Event, cx: &mut Context<Self>) {
        let trash_api::Event::Listed {
            folder,
            items,
            unrecorded,
        } = event;
        if folder != self.folder {
            self.claim(folder.as_deref());
            self.folder = folder;
        }
        self.count = Some(items.len() + unrecorded);
        let files = self.folder.clone();
        self.listed = items
            .into_iter()
            .filter(|item| item.item.parent() == files.as_deref())
            .filter_map(|item| Some((item.item.file_name()?.to_os_string(), item)))
            .collect();
        cx.notify();
    }

    fn claim(&self, folder: Option<&Path>) {
        let Some(folder) = folder else {
            self.host.rules.give_up(OWNER);
            return;
        };
        let rules = FolderRules {
            read_only: true,
            views: Views::DetailsOnly,
            replaces_menu: true,
            trashed: true,
        };
        self.host.rules.claim(OWNER, folder.to_path_buf(), rules);
        let name = FolderName {
            label: TRASH_LABEL.into(),
            icon: Icon::Trash,
            empty_when_missing: true,
        };
        self.host.rules.name(OWNER, folder.to_path_buf(), name);
    }

    fn watch(&mut self, event: &Event, cx: &mut Context<Self>) {
        match event {
            Event::JobStarted { job, operation, .. } if touches_trash(operation) => {
                self.touching.insert(*job);
            }
            Event::JobFinished { job, tally, .. } => {
                if self.touching.remove(job) || tally.replaced > 0 {
                    self.trash_changed(cx);
                }
            }
            Event::FolderListed(listing) if self.folder.as_deref() == Some(&listing.path) => {
                let scene = self.host.scene(cx);
                if !self.shown(&scene) {
                    self.ask_list();
                }
            }
            _ => {}
        }
    }

    fn trash_changed(&self, cx: &mut Context<Self>) {
        self.ask_list();
        let scene = self.host.scene(cx);
        if self.shown(&scene) {
            self.host.nav.update(cx, Navigator::reload);
        }
    }

    pub fn was_in(&self, entry: &Entry) -> Option<SharedString> {
        let folder = self.listed.get(&entry.name)?.original.parent()?;
        Some(short_path(folder, &self.host.home).into())
    }

    fn restore(&mut self, _: &RestoreSelected, _: &mut Window, cx: &mut Context<Self>) {
        let items = self.host.entries.read(cx).selected_paths();
        if !items.is_empty() {
            self.host.island.update(cx, |island, cx| {
                island.start_job(trash_api::restore(items), cx);
            });
        }
    }

    fn trash_selected(&mut self, _: &TrashSelected, _: &mut Window, cx: &mut Context<Self>) {
        let paths = self.host.entries.read(cx).selected_paths();
        if !paths.is_empty() {
            self.start_trash(paths, cx);
        }
    }

    fn start_trash(&self, paths: Vec<PathBuf>, cx: &mut App) {
        self.host.island.update(cx, |island, cx| {
            island.start_job(trash_api::trash(paths), cx);
        });
    }

    fn is_trash(&self, folder: &Path) -> bool {
        self.host.rules.for_folder(folder).trashed && self.host.rules.named(folder).is_some()
    }

    fn aim(&self, over: &DropOver<'_>, _: &App) -> Option<SharedString> {
        let Location::Folder(folder) = over.at else {
            return None;
        };
        self.is_trash(folder)
            .then(|| format!("{MOVE_TO} {}", over.name).into())
    }

    fn take(&mut self, at: Location, paths: Vec<PathBuf>, _: Modifiers, cx: &mut Context<Self>) {
        if let Location::Folder(folder) = at
            && self.is_trash(&folder)
        {
            self.start_trash(paths, cx);
        }
    }

    fn ask_empty(&mut self, _: &EmptyTrash, _: &mut Window, cx: &mut Context<Self>) {
        let what = self.count.map_or_else(|| EVERYTHING.to_owned(), count_of);
        let island = self.host.island.downgrade();
        let on_yes = Box::new(move |cx: &mut App| {
            island
                .update(cx, |island, cx| {
                    island.start_job(trash_api::empty_trash(), cx);
                })
                .ok();
        });
        self.host.island.update(cx, |island, cx| {
            island.ask(
                format!("Delete {what} in Trash for good?"),
                DELETE_LABEL,
                on_yes,
                cx,
            );
        });
    }
}

fn touches_trash(operation: &Operation) -> bool {
    matches!(operation, Operation::Undo { .. })
        || operation.get::<TrashJob>().is_some()
        || operation.get::<RestoreJob>().is_some()
        || operation.get::<DeleteFromTrashJob>().is_some()
        || operation.get::<EmptyTrashJob>().is_some()
}

fn count_of(number: usize) -> String {
    if number == 1 {
        format!("1 {ITEM}")
    } else {
        format!("{number} {ITEMS}")
    }
}

fn short_path(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => HOME.to_owned(),
        Ok(rest) => format!("{HOME}/{}", rest.to_string_lossy()),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

const fn may_trash(_: &TrashPart, scene: &Scene, _: &App) -> bool {
    !scene.picker && !scene.rules.read_only
}

fn trash_items(_: &TrashPart, scene: &Scene, _: &App) -> Vec<MenuItem> {
    if scene.rules.read_only {
        return Vec::new();
    }
    vec![MenuItem::run(Icon::Trash, &TRASH).with_look(Look::Danger)]
}

fn trash_button(_: &TrashPart, scene: &Scene, _: &App) -> Vec<MenuItem> {
    if scene.rules.read_only {
        return Vec::new();
    }
    vec![
        MenuItem::run(Icon::Trash, &TRASH)
            .with_selector("island-trash")
            .icon_only(),
    ]
}

fn in_trash(part: &TrashPart, scene: &Scene, _: &App) -> bool {
    part.shown(scene)
}

fn restore_items(part: &TrashPart, scene: &Scene, _: &App) -> Vec<MenuItem> {
    if !part.shown(scene) {
        return Vec::new();
    }
    vec![MenuItem::run(Icon::Undo, &RESTORE)]
}

fn restore_button(part: &TrashPart, scene: &Scene, _: &App) -> Vec<MenuItem> {
    if !part.shown(scene) {
        return Vec::new();
    }
    vec![
        MenuItem::run(Icon::Undo, &RESTORE)
            .with_look(Look::On)
            .with_selector("island-restore"),
    ]
}

fn empty_items(part: &TrashPart, scene: &Scene, _: &App) -> Vec<MenuItem> {
    if !part.shown(scene) || !part.full() {
        return Vec::new();
    }
    vec![MenuItem::run(Icon::Trash, &EMPTY_TRASH).with_look(Look::Danger)]
}

fn empty_chip(part: &TrashPart, scene: &Scene, _: &App) -> Vec<MenuItem> {
    if !part.shown(scene) || !part.full() {
        return Vec::new();
    }
    vec![MenuItem::run(Icon::Trash, &EMPTY_TRASH).with_selector("empty-trash")]
}

impl Part for TrashPart {
    fn open(host: &Host, _: &mut Context<Self>) -> Self {
        let part = Self {
            host: host.clone(),
            folder: None,
            listed: HashMap::new(),
            count: None,
            touching: HashSet::new(),
        };
        part.ask_list();
        part
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(Self::listed);
        slots.sends::<trash_api::Command>();
        slots.watch(Self::watch);
        slots.command(&TRASH, may_trash, Self::trash_selected);
        slots.command(&RESTORE, in_trash, Self::restore);
        slots.command(&EMPTY_TRASH, in_trash, Self::ask_empty);
        slots.menu_in(MenuAt::Entry, MenuGroup::Edit, Placed::FIRST, restore_items);
        slots.menu(MenuAt::Sheet, empty_items);
        slots.menu(MenuAt::Island, restore_button);
        slots.menu_in(MenuAt::Entry, MenuGroup::Danger, Placed::FIRST, trash_items);
        slots.menu_in(
            MenuAt::Island,
            MenuGroup::Danger,
            Placed::FIRST,
            trash_button,
        );
        slots.drop_target(Self::aim, Self::take);
        slots.job_words::<TrashJob>(TrashWords);
        slots.job_words::<RestoreJob>(RestoreWords);
        slots.job_words::<DeleteFromTrashJob>(DeleteFromTrashWords);
        slots.job_words::<EmptyTrashJob>(EmptyTrashWords);
        slots.question_reason(&NO_TRASH, NO_TRASH_REASON);
        slots.question_reason(&IN_TRASH, IN_TRASH_REASON);
        slots.menu(MenuAt::PathBarEnd, empty_chip);
        slots.column(|part, scene, _| {
            part.shown(scene).then_some(Column {
                head: WAS_IN_LABEL.into(),
                selector: WAS_IN_HEAD,
                cell: |part, entry, _| part.was_in(entry),
            })
        });
        slots.empty_note(|part, scene, _| {
            part.shown(scene)
                .then(|| (Icon::Trash, SharedString::new_static(EMPTY_LABEL)))
        });
        slots.sidebar(SidebarAt::Places, |part, scene, _| {
            let Some(folder) = part.folder.clone() else {
                return Vec::new();
            };
            let number = part
                .count
                .filter(|count| *count > 0)
                .map(|count| count.to_string().into());
            let here = scene.in_folder(&folder);
            vec![
                SidebarRow::new(ROW, Icon::Trash, TRASH_LABEL, Location::Folder(folder))
                    .number(number)
                    .here(here),
            ]
        });
    }
}
