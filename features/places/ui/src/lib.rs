#![forbid(unsafe_code)]

mod folder_tree;
mod shown;
mod tree;

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, Context};
use nova_files_places_api::{self as places_api, Bookmark, BookmarkChange, Place};
use nova_files_shell::assets::Icon;
use nova_files_shell::{
    Host, Location, Part, Published, RowItem, Scene, SidebarAt, SidebarRow, Slots,
};

pub use folder_tree::{Toggle, TreeRow};
pub use shown::{HOME_LABEL, Shown, folder_name, known, shown};
pub use tree::TreePart;

const PIN_BUTTON: &str = "pin";
const PICK_TITLE: &str = "Pin a folder";
const PICK_LABEL: &str = "Pin to Places";
const UNPIN_LABEL: &str = "Remove from Places";
const ROW_PREFIX: &str = "place-";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Known {
    pub places: Vec<Place>,
    pub pinned: Arc<[Bookmark]>,
}

pub struct PlacesPart {
    host: Host,
    known: Known,
    shown: Vec<Shown>,
}

impl PlacesPart {
    pub const fn known(&self) -> &Known {
        &self.known
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
    part.shown
        .iter()
        .enumerate()
        .map(|(index, place)| {
            SidebarRow::new(
                format!("{ROW_PREFIX}{index}"),
                place.icon,
                place.name.clone(),
                Location::Folder(place.path.clone()),
            )
            .rank(SidebarRow::OWN)
            .here(scene.in_folder(&place.path))
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

fn pin_button(part: &PlacesPart, scene: &Scene, _: &App) -> Option<RowItem> {
    if scene.picker {
        return None;
    }
    let nav = part.host.nav.clone();
    let change = Rc::new(part.change());
    Some(RowItem::new(Icon::Plus, PICK_LABEL, move |cx| {
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
    }))
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
    }
}
