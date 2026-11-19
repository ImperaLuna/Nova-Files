use std::mem;
use std::path::PathBuf;

use gpui::{Context, Pixels, Point, SharedString};
use nova_files_gathered_api::{Command, GatheredChange, GatheredFolder, Gathering, Holds};

use super::MainWindow;
use super::gathered::Adding;
use crate::assets::Icon;
use crate::dropping::ADD_TO;
use crate::island::{self, NoteUndo};
use crate::menu::{MenuAction, MenuItem};
use crate::places;

pub const ADD_FOLDER_LABEL: &str = "Add folder…";
const PICK_LABEL: &str = "Add";
const ONE_PROJECT: &str = "One project";
const PROJECTS_INSIDE: &str = "Projects inside";
const FOLDER: &str = "folder";
const FOLDERS: &str = "folders";

impl MainWindow {
    pub fn gathered_count(&self, place: Gathering) -> String {
        island::count(self.gathered.folders.of(place).len(), FOLDER, FOLDERS)
    }

    pub(crate) fn open_gathered_menu(
        &mut self,
        place: Gathering,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let folders = self
            .gathered
            .folders
            .of(place)
            .iter()
            .map(|folder| {
                MenuItem::new(
                    Icon::Close,
                    self.folder_label(folder),
                    MenuAction::Ungather(place, folder.path.clone()),
                )
                .with_detail(self.folder_detail(place, folder))
            })
            .collect();
        let add = vec![add_item(place)];
        self.open_menu_groups(at, vec![folders, add], false, cx);
    }

    fn folder_label(&self, folder: &GatheredFolder) -> String {
        folder.name.clone().unwrap_or_else(|| match folder.holds {
            Holds::One => island::name_of(&folder.path),
            Holds::Many => island::short_path(&folder.path, &self.home),
        })
    }

    fn folder_detail(&self, place: Gathering, folder: &GatheredFolder) -> Option<SharedString> {
        match (place, folder.holds) {
            (Gathering::Projects, Holds::One) => Some(ONE_PROJECT.into()),
            (Gathering::Projects, Holds::Many) => Some(PROJECTS_INSIDE.into()),
            (Gathering::Games, _) => folder
                .name
                .as_ref()
                .map(|_| island::short_path(&folder.path, &self.home).into()),
        }
    }

    pub(crate) fn ungather(&mut self, place: Gathering, path: PathBuf, cx: &mut Context<Self>) {
        let folders = self.gathered.folders.of(place);
        let Some((at, folder)) = folders
            .iter()
            .enumerate()
            .find(|(_, folder)| folder.path == path)
        else {
            return;
        };
        let label = format!(
            "Removed {} from {}",
            self.folder_label(folder),
            places::gathering_look(place).0
        );
        let back = GatheredChange::Add {
            place,
            path: path.clone(),
            holds: Some(folder.holds),
            at: u32::try_from(at).ok(),
            name: folder.name.clone(),
        };
        if self.change_gathered(GatheredChange::Remove { place, path }) {
            let undo = NoteUndo::Send(vec![Command::ChangeGathered(back)]);
            self.show_note(label, Some(undo), cx);
        }
    }

    pub fn pick_folder_to_gather(&mut self, place: Gathering, cx: &mut Context<Self>) {
        let title = format!("{ADD_TO} {}", places::gathering_look(place).0);
        self.pick_folder(&title, PICK_LABEL, cx, move |this, path, cx| {
            this.gather(place, vec![path], cx);
        });
    }

    pub(crate) fn gather(&mut self, place: Gathering, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if !self.gathered.heard {
            self.ask_gathered();
            self.gathered.to_gather = Some((place, paths));
            return;
        }
        let asked: Vec<PathBuf> = paths
            .into_iter()
            .filter(|path| !self.gathered.folders.has(place, path))
            .filter(|path| {
                self.change_gathered(GatheredChange::Add {
                    place,
                    path: path.clone(),
                    holds: None,
                    at: None,
                    name: None,
                })
            })
            .collect();
        self.gathered.adding = (!asked.is_empty()).then(|| Adding {
            place,
            asked,
            added: Vec::new(),
        });
        cx.notify();
    }

    pub(super) fn note_added(&mut self, cx: &mut Context<Self>) {
        let Some(adding) = &mut self.gathered.adding else {
            return;
        };
        let folders = self.gathered.folders.of(adding.place);
        let (now, still): (Vec<PathBuf>, Vec<PathBuf>) = mem::take(&mut adding.asked)
            .into_iter()
            .partition(|path| folders.iter().any(|folder| &folder.path == path));
        adding.asked = still;
        if now.is_empty() {
            return;
        }
        adding.added.extend(now);
        let place = adding.place;
        let label = format!(
            "Added {} to {}",
            island::what(&adding.added),
            places::gathering_look(place).0
        );
        let undo = adding
            .added
            .iter()
            .map(|path| {
                Command::ChangeGathered(GatheredChange::Remove {
                    place,
                    path: path.clone(),
                })
            })
            .collect();
        self.show_note(label, Some(NoteUndo::Send(undo)), cx);
    }

    fn change_gathered(&self, change: GatheredChange) -> bool {
        self.connection
            .as_ref()
            .is_some_and(|connection| connection.send(Command::ChangeGathered(change)).is_ok())
    }
}

pub fn add_item(place: Gathering) -> MenuItem {
    MenuItem::new(Icon::Plus, ADD_FOLDER_LABEL, MenuAction::AddGathered(place))
}
