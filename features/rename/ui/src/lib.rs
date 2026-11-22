#![forbid(unsafe_code)]

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use gpui::{App, AppContext as _, Context, Entity, Focusable as _, Subscription, Window};
use nova_files_api::numbered::stem;
use nova_files_api::{Entry, EntryKind, Event, Listing, Operation};
use nova_files_shell::assets::Icon;
use nova_files_shell::text::{TextField, TextFieldEvent};
use nova_files_shell::{
    Host, Location, MenuAt, MenuGroup, MenuItem, Part, Placed, RenameWhenMade, Scene, Slots,
    command_spec,
};

mod name;

use name::{free_name, renamed};

pub use nova_files_shell::actions::{NewFolder, RenameSelected};

command_spec!(RENAME, RenameSelected, "file.rename", "Rename", ["f2"]);
command_spec!(
    NEW_FOLDER,
    NewFolder,
    "file.new-folder",
    "New folder",
    ["ctrl-shift-n"]
);

pub struct RenamePart {
    host: Host,
    renaming: Option<Renaming>,
    made: RenameWhenMade,
    listing: Option<Listing>,
}

struct Renaming {
    folder: PathBuf,
    name: OsString,
    field: Entity<TextField>,
    _events: Subscription,
    _blur: Subscription,
}

impl Renaming {
    fn path(&self) -> PathBuf {
        self.folder.join(&self.name)
    }
}

impl RenamePart {
    fn in_folder(&self, scene: &Scene) -> Option<PathBuf> {
        if self.renaming.is_some() || scene.picker || scene.rules.read_only {
            return None;
        }
        scene
            .location
            .as_ref()
            .and_then(Location::folder)
            .map(PathBuf::from)
    }

    fn folder(&self, scene: &Scene) -> Option<PathBuf> {
        self.in_folder(scene).filter(|_| !scene.searching)
    }

    fn path_of(&self, entry: &Entry, cx: &App) -> Option<PathBuf> {
        Some(self.host.entries.read(cx).listing()?.path_of(entry))
    }

    fn shows(&self, renaming: &Renaming, cx: &App) -> bool {
        let entries = self.host.entries.read(cx);
        let listed = entries.listing().is_some_and(|listing| {
            listing
                .entries
                .iter()
                .any(|entry| listing.path_of(entry) == renaming.path())
        });
        listed && entries.drawn(&renaming.name)
    }

    fn rename_selected(&mut self, _: &RenameSelected, window: &mut Window, cx: &mut Context<Self>) {
        self.start(window, cx);
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let scene = self.host.scene(cx);
        if self.in_folder(&scene).is_none() {
            return;
        }
        let (path, name, is_folder) = match self.host.entries.read(cx).selected()[..] {
            [entry] => (
                self.path_of(entry, cx),
                entry.name.clone(),
                entry.kind == EntryKind::Folder,
            ),
            _ => return,
        };
        let Some(folder) = path.as_deref().and_then(Path::parent).map(PathBuf::from) else {
            return;
        };
        let text = name.to_string_lossy().into_owned();
        let field = cx.new(TextField::new);
        field.update(cx, |field, cx| {
            field.set_text(&text, cx);
            field.select_range(
                if is_folder {
                    0..text.len()
                } else {
                    0..stem(OsStr::new(&text)).len()
                },
                cx,
            );
        });
        let events = cx.subscribe_in(&field, window, |this, _, event, _, cx| match event {
            TextFieldEvent::Submitted => this.finish(true, cx),
            TextFieldEvent::Cancelled => this.finish(false, cx),
            TextFieldEvent::Changed => {}
        });
        let focus = field.read(cx).focus_handle(cx);
        let blur = cx.on_blur(&focus, window, |this, _, cx| {
            let drawn = this
                .renaming
                .as_ref()
                .is_some_and(|renaming| this.shows(renaming, cx));
            this.finish(drawn, cx);
        });
        window.focus(&focus, cx);
        self.renaming = Some(Renaming {
            folder,
            name,
            field,
            _events: events,
            _blur: blur,
        });
        cx.notify();
    }

    fn finish(&mut self, keep: bool, cx: &mut Context<Self>) {
        let Some(renaming) = self.renaming.take() else {
            return;
        };
        let typed = renaming.field.read(cx).text().to_owned();
        self.host
            .nav
            .update(cx, nova_files_shell::Navigator::focus_list);
        cx.notify();
        let name = renamed(&renaming.name, &typed);
        if !keep || typed.is_empty() || name == renaming.name {
            return;
        }
        let path = renaming.path();
        if !self.host.scene(cx).searching {
            self.host.entries.update(cx, |entries, cx| {
                entries.select_when_listed(renaming.folder, vec![name.clone()], cx);
            });
        }
        self.host.island.update(cx, |island, cx| {
            island.start_job(Operation::Rename { path, name }, cx);
        });
    }

    fn new_folder(&mut self, _: &NewFolder, window: &mut Window, cx: &mut Context<Self>) {
        let scene = self.host.scene(cx);
        let Some(inside) = self.folder(&scene) else {
            return;
        };
        let Some(listing) = self.listing.as_ref().filter(|shown| shown.path == inside) else {
            return;
        };
        let name = free_name(listing);
        self.made
            .wait(&self.host, (inside.join(&name), made_folder), window, cx);
        self.host.island.update(cx, |island, cx| {
            island.start_job(Operation::NewFolder { inside, name }, cx);
        });
    }

    fn watch(&mut self, event: &Event, _: &mut Context<Self>) {
        self.made.watch(event);
    }

    fn listed(&mut self, listing: &Listing, _: bool, cx: &mut Context<Self>) {
        self.listing = Some(listing.clone());
        let Some(made) = self.made.listed(listing) else {
            return;
        };
        let part = cx.entity().downgrade();
        cx.defer(move |cx| {
            made.update(cx, |_, window, cx| {
                part.update(cx, |part, cx| part.start(window, cx)).ok();
            })
            .ok();
        });
    }
}

fn field_of(part: &RenamePart, entry: &Entry, cx: &App) -> Option<Entity<TextField>> {
    let renaming = part.renaming.as_ref()?;
    (part.path_of(entry, cx)? == renaming.path()).then(|| renaming.field.clone())
}

const fn renaming(part: &RenamePart, _: &App) -> bool {
    part.renaming.is_some()
}

const fn anywhere(_: &RenamePart, _: &Scene, _: &App) -> bool {
    true
}

fn rename_item(part: &RenamePart, scene: &Scene, cx: &App) -> Vec<MenuItem> {
    let lone = part.host.entries.read(cx).selected().len() == 1;
    if scene.rules.replaces_menu || !lone || part.in_folder(scene).is_none() {
        return Vec::new();
    }
    vec![MenuItem::run(Icon::Rename, &RENAME)]
}

fn new_folder_item(part: &RenamePart, scene: &Scene, _: &App) -> Vec<MenuItem> {
    if scene.rules.replaces_menu || part.folder(scene).is_none() {
        return Vec::new();
    }
    vec![MenuItem::run(Icon::NewFolder, &NEW_FOLDER)]
}

fn island_button(part: &RenamePart, scene: &Scene, cx: &App) -> Vec<MenuItem> {
    if scene.rules.trashed || part.host.entries.read(cx).selected().len() != 1 {
        return Vec::new();
    }
    vec![
        MenuItem::run(Icon::Rename, &RENAME)
            .with_selector("island-rename")
            .icon_only(),
    ]
}

impl Part for RenamePart {
    fn open(host: &Host, _: &mut Context<Self>) -> Self {
        Self {
            host: host.clone(),
            renaming: None,
            made: RenameWhenMade::default(),
            listing: None,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.command(&NEW_FOLDER, anywhere, Self::new_folder);
        slots.command(&RENAME, anywhere, Self::rename_selected);
        slots.menu_in(MenuAt::Entry, MenuGroup::Edit, Placed::SECOND, rename_item);
        slots.menu_in(
            MenuAt::Sheet,
            MenuGroup::Edit,
            Placed::FIRST,
            new_folder_item,
        );
        slots.menu_in(
            MenuAt::Island,
            MenuGroup::Edit,
            Placed::SECOND,
            island_button,
        );
        slots.name_field(field_of);
        slots.busy(renaming);
        slots.listed(Self::listed);
        slots.watch(Self::watch);
    }
}

fn made_folder(operation: &Operation) -> Option<PathBuf> {
    match operation {
        Operation::NewFolder { inside, name } => Some(inside.join(name)),
        _ => None,
    }
}
