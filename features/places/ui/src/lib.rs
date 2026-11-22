#![forbid(unsafe_code)]

mod folder_tree;
mod pinning;
mod shown;
mod tree;

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{App, Context, SharedString};
use nova_files_places_api::{self as places_api, BookmarkChange, Known};
use nova_files_shell::assets::Icon;
use nova_files_shell::{
    Gap, Host, Location, Part, Published, RowItem, Scene, SectionButton, SidebarAt, SidebarRow,
    Slots,
};

pub use folder_tree::{Toggle, TreeRow};
pub use shown::{HOME_LABEL, Shown, folder_name, known, shown};
pub use tree::TreePart;

const PIN_BUTTON: &str = "pin";
const PICK_TITLE: &str = "Pin a folder";
const PICK_LABEL: &str = "Pin to Places";
const UNPIN_LABEL: &str = "Remove from Places";
const CHOOSE_LABEL: &str = "Choose a folder…";
const ROW_PREFIX: &str = "place-";

pub struct PlacesPart {
    host: Host,
    known: Known,
    shown: Vec<Shown>,
}

impl PlacesPart {
    pub const fn known(&self) -> &Known {
        &self.known
    }

    fn folder_look(&self, folder: &Path, _: &App) -> Option<(SharedString, Icon)> {
        self.shown
            .iter()
            .find(|shown| shown.path == folder)
            .map(|shown| (shown.name.clone(), shown.icon))
    }

    fn arrived(&mut self, event: places_api::Event, cx: &mut Context<Self>) {
        match event {
            places_api::Event::Places(places) => {
                Published::set(known(&places), cx);
                self.known.places = places;
            }
            places_api::Event::Bookmarks(pinned) => self.known.pinned = pinned,
        }
        self.shown = shown(&self.known.places, &self.known.pinned);
        Published::set(self.known.clone(), cx);
        cx.notify();
    }

    fn pinned_at(&self, row: &str) -> Option<PathBuf> {
        let index: usize = row.strip_prefix(ROW_PREFIX)?.parse().ok()?;
        let shown = self.shown.get(index)?;
        shown.pinned.then(|| shown.path.clone())
    }

    fn change(&self) -> impl Fn(BookmarkChange) + 'static {
        let link = self.host.link.clone();
        move |change| {
            link.send(places_api::Command::ChangeBookmark(change)).ok();
        }
    }
}

fn rows(part: &PlacesPart, scene: &Scene, _: &App) -> Vec<SidebarRow> {
    let last_standard = part.shown.iter().rposition(|place| place.standard);
    part.shown
        .iter()
        .enumerate()
        .map(|(index, place)| {
            let (above, below) = match part.pin_position(place) {
                Some(at) => (Some(at), Some(at + 1)),
                None if Some(index) == last_standard => (None, Some(0)),
                None => (None, None),
            };
            SidebarRow::new(
                format!("{ROW_PREFIX}{index}"),
                place.icon,
                place.name.clone(),
                Location::Folder(place.path.clone()),
            )
            .rank(SidebarRow::OWN)
            .here(scene.in_folder(&place.path))
            .gaps(above.map(|at| part.gap(at)), below.map(|at| part.gap(at)))
            .drags(place.pinned && !place.standard)
        })
        .collect()
}

fn row_menu(part: &PlacesPart, row: &str, _: &App) -> Vec<Vec<RowItem>> {
    let Some(path) = part.pinned_at(row) else {
        return Vec::new();
    };
    let change = part.change();
    vec![vec![RowItem::new(Icon::Close, UNPIN_LABEL, move |_| {
        change(BookmarkChange::Remove { path: path.clone() });
    })]]
}

fn pin_button(part: &PlacesPart, scene: &Scene, _: &App) -> Option<SectionButton> {
    if scene.picker {
        return None;
    }
    let here = scene
        .location
        .as_ref()
        .and_then(Location::folder)
        .filter(|_| !scene.searching);
    let group = here
        .and_then(|folder| part.here_item(folder))
        .into_iter()
        .chain([part.choose_item()])
        .collect();
    Some(SectionButton::Opens {
        icon: Icon::Plus,
        groups: vec![group],
    })
}

impl PlacesPart {
    fn here_item(&self, folder: &Path) -> Option<RowItem> {
        let change = self.change();
        let path = folder.to_path_buf();
        match self.shown.iter().find(|shown| shown.path == folder) {
            Some(shown) if shown.pinned => Some(RowItem::new(
                Icon::Close,
                format!("Remove {} from Places", shown.name),
                move |_| change(BookmarkChange::Remove { path: path.clone() }),
            )),
            Some(_) => None,
            None => Some(RowItem::new(
                Icon::Plus,
                format!("Pin {} to Places", folder_name(folder)),
                move |_| {
                    change(BookmarkChange::Add {
                        path: path.clone(),
                        label: None,
                        at: None,
                    });
                },
            )),
        }
    }

    fn pin_position(&self, place: &Shown) -> Option<usize> {
        if place.standard {
            return None;
        }
        self.known
            .pinned
            .iter()
            .position(|pin| pin.path == place.path)
    }

    fn gap(&self, at: usize) -> Gap {
        let change = self.change();
        let pinned: Vec<PathBuf> = self
            .known
            .pinned
            .iter()
            .map(|pin| pin.path.clone())
            .collect();
        Gap::new(PICK_LABEL, move |paths, _| {
            for each in pinning::pin_at(&pinned, at, paths) {
                change(each);
            }
        })
    }

    fn choose_item(&self) -> RowItem {
        let nav = self.host.nav.clone();
        let change = Rc::new(self.change());
        RowItem::new(Icon::Folder, CHOOSE_LABEL, move |cx| {
            let change = Rc::clone(&change);
            nav.update(cx, |nav, cx| {
                nav.pick_folder(
                    PICK_TITLE,
                    PICK_LABEL,
                    move |path, _| {
                        change(BookmarkChange::Add {
                            path,
                            label: None,
                            at: None,
                        });
                    },
                    cx,
                );
            });
        })
    }
}

impl Part for PlacesPart {
    fn open(host: &Host, _: &mut Context<Self>) -> Self {
        host.link.send(places_api::Command::Places).ok();
        Self {
            host: host.clone(),
            known: Known::default(),
            shown: Vec::new(),
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(Self::arrived);
        slots.sends::<places_api::Command>();
        slots.sidebar(SidebarAt::Places, rows);
        slots.row_menu(row_menu);
        slots.section_button(SidebarAt::Places, PIN_BUTTON, pin_button);
        slots.folder_look(Self::folder_look);
    }
}
