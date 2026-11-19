#![forbid(unsafe_code)]

mod chip;
mod page;

use std::mem;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, Context, SharedString, Window};
use nova_files_api::Entry;
use nova_files_gathered_api::{
    Command, Event, Gathered, GatheredChange, GatheredFolder, Gathering, Holds,
};
use nova_files_shell::assets::Icon;
use nova_files_shell::island::words::{name_of, short_path, what};
use nova_files_shell::{
    Column, Host, Location, MenuAt, MenuGroup, MenuItem, Navigator, PageRef, Part, Placed, RowItem,
    Scene, SidebarAt, SidebarRow, Slots, Undo, command_spec, format,
};

pub use page::{GAMES_LABEL, GatheredPage, PROJECTS_LABEL, look};

mod actions {
    #![expect(
        clippy::derive_partial_eq_without_eq,
        reason = "GPUI's actions! macro derives PartialEq only"
    )]
    gpui::actions!(nova_files_gathered, [AddFolder]);
}

pub use actions::AddFolder;

command_spec!(
    ADD_FOLDER,
    AddFolder,
    "gathered.add-folder",
    "Add folder…",
    []
);

pub const LIBRARY_LABEL: &str = "Library";
const LIBRARY_HEAD: &str = "head-library";
const ROW_PREFIX: &str = "place-";
const ADD_TO: &str = "Add to";
const PICK_LABEL: &str = "Add";
const ONE_PROJECT: &str = "One project";
const PROJECTS_INSIDE: &str = "Projects inside";
const FOLDER: &str = "folder";
const FOLDERS: &str = "folders";

pub struct GatheredPart {
    host: Host,
    folders: Arc<Gathered>,
    heard: bool,
    to_gather: Option<(Gathering, Vec<PathBuf>)>,
    adding: Option<Adding>,
}

struct Adding {
    place: Gathering,
    asked: Vec<PathBuf>,
    added: Vec<PathBuf>,
}

impl GatheredPart {
    pub fn count(&self, place: Gathering) -> String {
        format::count(self.folders.of(place).len(), FOLDER, FOLDERS)
    }

    pub fn library_of_row(&self, index: usize, cx: &App) -> Option<String> {
        let entries = self.host.entries.read(cx);
        let listing = entries.listing()?;
        let entry = listing.entries.get(index)?;
        self.library_of(&listing.path_of(entry))
    }

    fn library_of(&self, game: &Path) -> Option<String> {
        let folder = game.parent()?;
        let libraries = self.folders.of(Gathering::Games);
        let library = libraries
            .iter()
            .find(|library| library.holds == Holds::One && library.path == game)
            .or_else(|| {
                libraries
                    .iter()
                    .find(|library| library.holds == Holds::Many && library.path == folder)
            });
        Some(
            library
                .and_then(|library| library.name.clone())
                .unwrap_or_else(|| short_path(folder, &self.host.home)),
        )
    }

    fn library_cell(&self, entry: &Entry, cx: &App) -> Option<SharedString> {
        let entries = self.host.entries.read(cx);
        let path = entries.listing()?.path_of(entry);
        Some(self.library_of(&path).unwrap_or_default().into())
    }

    fn arrived(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Gathered(gathered) => self.heard(gathered, cx),
            Event::GatheredChanged(place) => self.changed(place, cx),
        }
    }

    fn heard(&mut self, gathered: Arc<Gathered>, cx: &mut Context<Self>) {
        let before = mem::replace(&mut self.folders, gathered);
        if mem::replace(&mut self.heard, true) {
            for place in Gathering::ALL {
                if before.of(place) != self.folders.of(place) {
                    self.changed(place, cx);
                }
            }
            self.note_added(cx);
        } else if let Some((place, paths)) = self.to_gather.take() {
            self.gather(place, paths, cx);
        }
        cx.notify();
    }

    pub fn changed(&self, place: Gathering, cx: &mut App) {
        let scene = self.host.scene(cx);
        if scene.on_page::<GatheredPage>() == Some(&GatheredPage::new(place)) {
            self.host.nav.update(cx, Navigator::reload);
        }
    }

    fn folder_label(&self, folder: &GatheredFolder) -> String {
        folder.name.clone().unwrap_or_else(|| match folder.holds {
            Holds::One => name_of(&folder.path),
            Holds::Many => short_path(&folder.path, &self.host.home),
        })
    }

    fn folder_detail(&self, place: Gathering, folder: &GatheredFolder) -> Option<String> {
        match (place, folder.holds) {
            (Gathering::Projects, Holds::One) => Some(ONE_PROJECT.to_owned()),
            (Gathering::Projects, Holds::Many) => Some(PROJECTS_INSIDE.to_owned()),
            (Gathering::Games, _) => folder
                .name
                .as_ref()
                .map(|_| short_path(&folder.path, &self.host.home)),
        }
    }

    fn open_menu(&self, place: Gathering, at: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        let part = cx.entity().downgrade();
        let folders = self
            .folders
            .of(place)
            .iter()
            .map(|folder| {
                let (part, path) = (part.clone(), folder.path.clone());
                let item = RowItem::new(Icon::Close, self.folder_label(folder), move |cx| {
                    let path = path.clone();
                    part.update(cx, |part, cx| part.ungather(place, path, cx))
                        .ok();
                });
                match self.folder_detail(place, folder) {
                    Some(detail) => item.with_detail(detail),
                    None => item,
                }
            })
            .collect();
        let add = RowItem::new(Icon::Plus, ADD_FOLDER.label, move |cx| {
            part.update(cx, |part, cx| part.pick(place, cx)).ok();
        });
        self.host.nav.update(cx, |nav, cx| {
            nav.menu_at(at, vec![folders, vec![add]], cx);
        });
    }

    fn ungather(&self, place: Gathering, path: PathBuf, cx: &mut Context<Self>) {
        let Some((at, folder)) = self
            .folders
            .of(place)
            .iter()
            .enumerate()
            .find(|(_, folder)| folder.path == path)
        else {
            return;
        };
        let label = format!(
            "Removed {} from {}",
            self.folder_label(folder),
            look(place).0
        );
        let back = GatheredChange::Add {
            place,
            path: path.clone(),
            holds: Some(folder.holds),
            at: u32::try_from(at).ok(),
            name: folder.name.clone(),
        };
        if self.change(GatheredChange::Remove { place, path }) {
            self.note(label, vec![back], cx);
        }
    }

    fn add_folder(&mut self, _: &AddFolder, _: &mut Window, cx: &mut Context<Self>) {
        let scene = self.host.scene(cx);
        if let Some(page) = scene.on_page::<GatheredPage>() {
            self.pick(page.place, cx);
        }
    }

    fn pick(&self, place: Gathering, cx: &mut Context<Self>) {
        let part = cx.entity().downgrade();
        let title = format!("{ADD_TO} {}", look(place).0);
        self.host.nav.update(cx, |nav, cx| {
            nav.pick_folder(
                title,
                PICK_LABEL,
                move |path, cx| {
                    part.update(cx, |part, cx| part.gather(place, vec![path], cx))
                        .ok();
                },
                cx,
            );
        });
    }

    fn gather(&mut self, place: Gathering, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if !self.heard {
            self.to_gather = Some((place, paths));
            return;
        }
        let asked: Vec<PathBuf> = paths
            .into_iter()
            .filter(|path| !self.folders.has(place, path))
            .filter(|path| {
                self.change(GatheredChange::Add {
                    place,
                    path: path.clone(),
                    holds: None,
                    at: None,
                    name: None,
                })
            })
            .collect();
        self.adding = (!asked.is_empty()).then(|| Adding {
            place,
            asked,
            added: Vec::new(),
        });
        cx.notify();
    }

    fn note_added(&mut self, cx: &mut Context<Self>) {
        let Some(adding) = &mut self.adding else {
            return;
        };
        let folders = self.folders.of(adding.place);
        let (now, still): (Vec<PathBuf>, Vec<PathBuf>) = mem::take(&mut adding.asked)
            .into_iter()
            .partition(|path| folders.iter().any(|folder| &folder.path == path));
        adding.asked = still;
        if now.is_empty() {
            return;
        }
        adding.added.extend(now);
        let place = adding.place;
        let label = format!("Added {} to {}", what(&adding.added), look(place).0);
        let undo = adding
            .added
            .iter()
            .map(|path| GatheredChange::Remove {
                place,
                path: path.clone(),
            })
            .collect();
        self.note(label, undo, cx);
    }

    fn note(&self, label: String, undo: Vec<GatheredChange>, cx: &mut Context<Self>) {
        let link = self.host.link.clone();
        let undo = Undo::Run(Rc::new(move |_| {
            for change in &undo {
                link.send(Command::ChangeGathered(change.clone())).ok();
            }
        }));
        self.host.island.update(cx, |island, cx| {
            island.note(label, Some(undo), cx);
        });
    }

    fn change(&self, change: GatheredChange) -> bool {
        self.host.link.send(Command::ChangeGathered(change)).is_ok()
    }
}

fn on_page(_: &GatheredPart, scene: &Scene, _: &App) -> bool {
    scene.on_page::<GatheredPage>().is_some()
}

fn add_items(_: &GatheredPart, scene: &Scene, _: &App) -> Vec<MenuItem> {
    if scene.on_page::<GatheredPage>().is_none() {
        return Vec::new();
    }
    vec![MenuItem::run(Icon::Plus, &ADD_FOLDER)]
}

fn rows(_: &GatheredPart, scene: &Scene, _: &App) -> Vec<SidebarRow> {
    if scene.picker {
        return Vec::new();
    }
    Gathering::ALL
        .into_iter()
        .map(|place| {
            let (name, icon) = look(place);
            let page = GatheredPage::new(place);
            let here = scene.on_page::<GatheredPage>() == Some(&page);
            let opens = Location::Page(PageRef::new(page));
            SidebarRow::new(
                format!("{ROW_PREFIX}{}", name.to_lowercase()),
                icon,
                name,
                opens,
            )
            .rank(SidebarRow::GATHERED)
            .here(here)
        })
        .collect()
}

fn library_column(_: &GatheredPart, scene: &Scene, _: &App) -> Option<Column<GatheredPart>> {
    (scene.on_page::<GatheredPage>()?.place == Gathering::Games).then_some(Column {
        head: LIBRARY_LABEL.into(),
        selector: LIBRARY_HEAD,
        cell: |part, entry, cx| part.library_cell(entry, cx),
    })
}

impl Part for GatheredPart {
    fn open(host: &Host, _: &mut Context<Self>) -> Self {
        host.link.send(Command::Gathered).ok();
        Self {
            host: host.clone(),
            folders: Arc::default(),
            heard: false,
            to_gather: None,
            adding: None,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(Self::arrived);
        slots.sends::<Command>();
        slots.listing_page::<GatheredPage>(GatheredPage::list);
        slots.command(&ADD_FOLDER, on_page, Self::add_folder);
        slots.menu_in(MenuAt::Sheet, MenuGroup::Edit, Placed::FIRST, add_items);
        slots.sidebar(SidebarAt::Places, rows);
        slots.drops::<GatheredPage>(|part, page, paths, cx| part.gather(page.place, paths, cx));
        slots.column(library_column);
        slots.path_chip(chip::chip);
    }
}
