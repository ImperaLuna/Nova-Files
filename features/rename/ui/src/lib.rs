#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::ops::Range;
use std::path::PathBuf;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement as _, ParentElement as _, SharedString, Styled as _, Subscription, Window, div,
};
use nova_files_api::numbered::stem;
use nova_files_api::{Entry, EntryKind, Event, Listing, ListingId, Operation};
use nova_files_shell::assets::Icon;
use nova_files_shell::text::{TextField, TextFieldEvent};
use nova_files_shell::theme::Colors;
use nova_files_shell::{
    Host, Location, MenuAt, MenuGroup, MenuItem, Part, Placed, RenameWhenMade, Scene, Slots,
    command_spec,
};

mod name;
mod several;
mod words;

use name::{free_name, renamed};
use nova_files_rename_api::{
    NewFileJob, NewFolderJob, RenameJob, RenameManyJob, new_file, new_folder, rename, rename_many,
};
use several::{Named, Plan};
use words::{NewFileWords, NewFolderWords, RenameManyWords, RenameWords};

pub use nova_files_shell::actions::{NewFile, NewFolder, RenameSelected};

command_spec!(
    RENAME,
    RenameSelected,
    "file.rename",
    "Rename",
    ["f2"],
    needs[Change]
);
command_spec!(
    NEW_FOLDER,
    NewFolder,
    "file.new-folder",
    "New folder",
    ["ctrl-shift-n"],
    needs[Change]
);
command_spec!(
    NEW_FILE,
    NewFile,
    "file.new-file",
    "New file",
    ["ctrl-alt-n"],
    needs[Change]
);

type MakeJob = fn(PathBuf, OsString) -> Operation;
type MadePath = fn(&Operation) -> Option<PathBuf>;

const NEW_FOLDER_NAME: &str = "New folder";
const NEW_FILE_NAME: &str = "New file";

type Picked = ((PathBuf, OsString, bool), Vec<Other>, HashSet<OsString>);

const NEW_NAME: &str = "new-name";

pub struct RenamePart {
    host: Host,
    renaming: Option<Renaming>,
    made: RenameWhenMade,
    listing: Option<Listing>,
}

struct Other {
    name: OsString,
    is_folder: bool,
}

struct Renaming {
    folder: PathBuf,
    name: OsString,
    is_folder: bool,
    others: Vec<Other>,
    unselected: HashSet<OsString>,
    plan: Plan,
    field: Entity<TextField>,
    seen: bool,
    _events: Subscription,
    _blur: Subscription,
}

impl Renaming {
    fn path(&self) -> PathBuf {
        self.folder.join(&self.name)
    }

    fn planned(&self, typed: &str) -> Plan {
        if self.others.is_empty() {
            return Plan::default();
        }
        let others: Vec<Named<'_>> = self
            .others
            .iter()
            .map(|other| Named {
                name: &other.name,
                is_folder: other.is_folder,
            })
            .collect();
        let unselected = self.unselected.iter().map(OsString::as_os_str).collect();
        several::plan(
            Named {
                name: &self.name,
                is_folder: self.is_folder,
            },
            typed,
            &others,
            &unselected,
        )
    }

    fn new_names(&self, name: &OsString) -> Vec<OsString> {
        let others = self.others.iter().map(|other| {
            self.plan
                .others
                .get(&other.name)
                .map_or(&other.name, |planned| &planned.name)
        });
        others.chain([name]).cloned().collect()
    }
}

impl RenamePart {
    fn in_folder(&self, scene: &Scene) -> Option<PathBuf> {
        if self.renaming.is_some() || scene.picker {
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

    fn renames(&self, scene: &Scene, cx: &App) -> bool {
        let selected = self.host.entries.read(cx).selected_count();
        let several = selected > 1 && !scene.searching;
        (selected == 1 || several) && self.in_folder(scene).is_some()
    }

    fn rows_drawn(&mut self, _: ListingId, _: Range<usize>, cx: &mut Context<Self>) {
        let Some(renaming) = self.renaming.as_mut() else {
            return;
        };
        if self.host.entries.read(cx).drawn(&renaming.name) {
            renaming.seen = true;
        } else if renaming.seen {
            self.finish(true, cx);
        }
    }

    fn rename_selected(&mut self, _: &RenameSelected, window: &mut Window, cx: &mut Context<Self>) {
        self.start(window, cx);
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let scene = self.host.scene(cx);
        if self.in_folder(&scene).is_none() {
            return;
        }
        let Some((edited, others, unselected)) = self.picked(scene.searching, cx) else {
            return;
        };
        let (path, name, is_folder) = edited;
        let Some(folder) = path.parent().map(PathBuf::from) else {
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
            TextFieldEvent::Submitted if this.clashes() => {}
            TextFieldEvent::Submitted => this.finish(true, cx),
            TextFieldEvent::Cancelled => this.finish(false, cx),
            TextFieldEvent::Changed => this.plan_again(cx),
        });
        let focus = field.read(cx).focus_handle(cx);
        let blur = cx.on_blur(&focus, window, |this, _, cx| {
            let drawn = this
                .renaming
                .as_ref()
                .is_some_and(|renaming| this.shows(renaming, cx));
            this.finish(drawn && !this.clashes(), cx);
        });
        window.focus(&focus, cx);
        let seen = self.host.entries.read(cx).drawn(&name);
        self.renaming = Some(Renaming {
            folder,
            name,
            is_folder,
            others,
            unselected,
            plan: Plan::default(),
            field,
            seen,
            _events: events,
            _blur: blur,
        });
        cx.notify();
    }

    fn picked(&self, searching: bool, cx: &App) -> Option<Picked> {
        let entries = self.host.entries.read(cx);
        let listing = entries.listing()?;
        let mut selected = entries.selected();
        if selected.is_empty() || (searching && selected.len() > 1) {
            return None;
        }
        selected.sort_by_key(|entry| entry.id.0);
        let under_cursor = |entry: &&&Entry| usize::try_from(entry.id.0).ok() == entries.cursor();
        let edited = *selected
            .iter()
            .find(under_cursor)
            .or_else(|| selected.first())?;
        let others: Vec<Other> = selected
            .iter()
            .filter(|entry| entry.id != edited.id)
            .map(|entry| Other {
                name: entry.name.clone(),
                is_folder: entry.kind == EntryKind::Folder,
            })
            .collect();
        let unselected = if others.is_empty() {
            HashSet::new()
        } else {
            let ids: HashSet<_> = selected.iter().map(|entry| entry.id).collect();
            listing
                .entries
                .iter()
                .filter(|entry| !ids.contains(&entry.id))
                .map(|entry| entry.name.clone())
                .collect()
        };
        let edited = (
            listing.path_of(edited),
            edited.name.clone(),
            edited.kind == EntryKind::Folder,
        );
        Some((edited, others, unselected))
    }

    fn clashes(&self) -> bool {
        self.renaming
            .as_ref()
            .is_some_and(|renaming| renaming.plan.clashes)
    }

    fn plan_again(&mut self, cx: &mut Context<Self>) {
        let Some(renaming) = self.renaming.as_mut() else {
            return;
        };
        renaming.plan = renaming.planned(renaming.field.read(cx).text());
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
        let untouched = name == renaming.name && renaming.plan.others.is_empty();
        if !keep || typed.is_empty() || untouched {
            return;
        }
        let path = renaming.path();
        let job = if renaming.others.is_empty() {
            rename(path, name.clone())
        } else {
            let others = renaming
                .plan
                .others
                .iter()
                .map(|(old, planned)| (renaming.folder.join(old), planned.name.clone()));
            rename_many(std::iter::once((path, name.clone())).chain(others))
        };
        if !self.host.scene(cx).searching {
            let names = renaming.new_names(&name);
            self.host.entries.update(cx, |entries, cx| {
                entries.select_when_listed(renaming.folder, names, cx);
            });
        }
        self.host.island.update(cx, |island, cx| {
            island.start_job(job, cx);
        });
    }

    fn new_folder(&mut self, _: &NewFolder, window: &mut Window, cx: &mut Context<Self>) {
        self.make(NEW_FOLDER_NAME, (new_folder, made_folder), window, cx);
    }

    fn new_file(&mut self, _: &NewFile, window: &mut Window, cx: &mut Context<Self>) {
        self.make(NEW_FILE_NAME, (new_file, made_file), window, cx);
    }

    fn make(
        &mut self,
        base: &str,
        (job, makes): (MakeJob, MadePath),
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let scene = self.host.scene(cx);
        let Some(inside) = self.folder(&scene) else {
            return;
        };
        let Some(listing) = self.listing.as_ref().filter(|shown| shown.path == inside) else {
            return;
        };
        let name = free_name(listing, base);
        self.made
            .wait(&self.host, (inside.join(&name), makes), window, cx);
        self.host.island.update(cx, |island, cx| {
            island.start_job(job(inside, name), cx);
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

fn new_name(part: &RenamePart, entry: &Entry, cx: &App) -> Option<AnyElement> {
    let renaming = part.renaming.as_ref()?;
    let planned = renaming.plan.others.get(&entry.name)?;
    if part.path_of(entry, cx)?.parent()? != renaming.folder {
        return None;
    }
    let colors = Colors::of(cx);
    let name: SharedString = planned.name.to_string_lossy().into_owned().into();
    Some(
        div()
            .debug_selector(|| NEW_NAME.to_owned())
            .min_w_0()
            .truncate()
            .text_color(if planned.clashes {
                colors.danger
            } else {
                colors.text
            })
            .child(name)
            .into_any_element(),
    )
}

const fn renaming(part: &RenamePart, _: &App) -> bool {
    part.renaming.is_some()
}

const fn anywhere(_: &RenamePart, _: &Scene, _: &App) -> bool {
    true
}

fn rename_item(part: &RenamePart, scene: &Scene, cx: &App) -> Vec<MenuItem> {
    if !part.renames(scene, cx) {
        return Vec::new();
    }
    vec![MenuItem::run(Icon::Rename, &RENAME)]
}

fn new_folder_item(part: &RenamePart, scene: &Scene, _: &App) -> Vec<MenuItem> {
    if part.folder(scene).is_none() {
        return Vec::new();
    }
    vec![
        MenuItem::run(Icon::NewFolder, &NEW_FOLDER),
        MenuItem::run(Icon::NewFile, &NEW_FILE),
    ]
}

fn island_button(part: &RenamePart, scene: &Scene, cx: &App) -> Vec<MenuItem> {
    if !part.renames(scene, cx) {
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
        slots.command(&NEW_FILE, anywhere, Self::new_file);
        slots.job_words::<NewFileJob>(NewFileWords);
        slots.job_words::<RenameJob>(RenameWords);
        slots.job_words::<RenameManyJob>(RenameManyWords);
        slots.job_words::<NewFolderJob>(NewFolderWords);
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
        slots.name_cell(new_name);
        slots.busy(renaming);
        slots.listed(Self::listed);
        slots.rows_drawn(Self::rows_drawn);
        slots.watch(Self::watch);
    }
}

fn made_file(operation: &Operation) -> Option<PathBuf> {
    operation
        .get::<NewFileJob>()
        .map(|job| job.inside.join(&job.name))
}

fn made_folder(operation: &Operation) -> Option<PathBuf> {
    operation
        .get::<NewFolderJob>()
        .map(|job| job.inside.join(&job.name))
}
