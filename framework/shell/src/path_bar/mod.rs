mod crumbs;
mod draw;
mod typing;

use std::path::{Path, PathBuf};

use gpui::{App, Context, EventEmitter, Pixels, Point, SharedString};
use nova_files_api::{Command, EntryKind, Event, Listing, ListingId, Order};
use nova_files_messages::Envelope;

use crate::assets::Icon;
use crate::host::Host;
use crate::line::Line;
use crate::location::Location;
use crate::published::{KnownFolders, Published};

pub use crumbs::{
    Crumb, KEPT_AT_END, MOST_SHOWN, PageCrumbs, Shown, crumbs, folded, named, page_crumbs, shown,
    under_home,
};
pub use draw::{BarInput, draw, menu_chip};
pub use typing::{shown_path, typed_target};

use typing::PathField;

const MENU_LISTING: ListingId = ListingId(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub path: PathBuf,
    pub label: SharedString,
    pub icon: Icon,
    pub on: bool,
}

impl MenuItem {
    fn of(crumb: &Crumb) -> Option<Self> {
        Some(Self {
            path: crumb.folder()?.to_path_buf(),
            label: crumb.label.clone(),
            icon: crumb.icon,
            on: false,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuOwner {
    Folded,
    Arrow(usize),
}

pub struct PathMenu {
    pub owner: MenuOwner,
    pub folder: PathBuf,
    asks: u64,
    pub items: Vec<MenuItem>,
    _line: Option<Line>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spot<'a> {
    pub on_screen: Option<&'a Path>,
    pub asks: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathBarEvent {
    Typed(PathBuf),
    Left,
}

pub struct PathBar {
    host: Host,
    menu: Option<PathMenu>,
    dismissed: Option<(MenuOwner, Point<Pixels>)>,
    field: Option<PathField>,
}

impl EventEmitter<PathBarEvent> for PathBar {}

impl PathBar {
    pub fn new(host: &Host) -> Self {
        Self {
            host: host.clone(),
            menu: None,
            dismissed: None,
            field: None,
        }
    }

    pub fn menu(&self, at: Spot<'_>) -> Option<&PathMenu> {
        self.menu
            .as_ref()
            .filter(|menu| at.on_screen == Some(menu.folder.as_path()) && at.asks == menu.asks)
    }

    pub fn menu_names(&self, at: Spot<'_>) -> Option<Vec<String>> {
        self.menu(at).map(|menu| {
            menu.items
                .iter()
                .map(|item| item.label.to_string())
                .collect()
        })
    }

    fn press_folded(
        &mut self,
        at: Point<Pixels>,
        crumbs: &[Crumb],
        spot: Spot<'_>,
        cx: &mut Context<Self>,
    ) {
        if let Some(folder) = self.menu_opens(MenuOwner::Folded, at, spot) {
            self.menu = Some(PathMenu {
                owner: MenuOwner::Folded,
                folder,
                asks: spot.asks,
                items: crumbs.iter().filter_map(MenuItem::of).collect(),
                _line: None,
            });
        }
        cx.notify();
    }

    fn press_arrow(
        &mut self,
        (index, folder): (usize, &Path),
        at: Point<Pixels>,
        spot: Spot<'_>,
        cx: &mut Context<Self>,
    ) {
        let owner = MenuOwner::Arrow(index);
        if let Some(on_screen) = self.menu_opens(owner, at, spot) {
            self.menu = self.menu_line(folder, cx).map(|line| PathMenu {
                owner,
                folder: on_screen,
                asks: spot.asks,
                items: Vec::new(),
                _line: Some(line),
            });
        }
        cx.notify();
    }

    fn menu_opens(
        &mut self,
        owner: MenuOwner,
        at: Point<Pixels>,
        spot: Spot<'_>,
    ) -> Option<PathBuf> {
        if self.dismissed.take() == Some((owner, at)) {
            return None;
        }
        spot.on_screen.map(Path::to_path_buf)
    }

    fn menu_line(&self, folder: &Path, cx: &mut Context<Self>) -> Option<Line> {
        let line = self.host.lines.open(cx, Self::menu_heard);
        line.send(Command::ListFolder {
            id: MENU_LISTING,
            path: folder.to_path_buf(),
            order: Order::default(),
            filter: String::new(),
        })
        .ok()?;
        Some(line)
    }

    fn menu_heard(&mut self, envelope: Envelope, cx: &mut Context<Self>) {
        let Some(event) = envelope.taken_as::<Event>() else {
            return;
        };
        match event {
            Event::FolderListed(listing) => {
                let known = Published::<KnownFolders>::get(cx)
                    .cloned()
                    .unwrap_or_default();
                if let Some(menu) = &mut self.menu {
                    let items = menu_items(&listing, &menu.folder, &known);
                    if items.is_empty() {
                        self.menu = None;
                    } else {
                        menu.items = items;
                    }
                }
                cx.notify();
            }
            Event::FolderUnreadable { .. } => {
                self.menu = None;
                cx.notify();
            }
            _ => {}
        }
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        cx.notify();
    }

    fn dismiss_menu(&mut self, at: Point<Pixels>, spot: Spot<'_>, cx: &mut Context<Self>) {
        self.dismissed = self.menu(spot).map(|menu| (menu.owner, at));
        self.close_menu(cx);
    }

    fn choose(&mut self, index: usize, spot: Spot<'_>, cx: &mut Context<Self>) {
        let chosen = self
            .menu(spot)
            .and_then(|menu| menu.items.get(index))
            .map(|item| item.path.clone());
        self.close_menu(cx);
        if let Some(path) = chosen {
            self.go(Location::Folder(path), cx);
        }
    }

    fn go(&self, place: Location, cx: &mut App) {
        self.host.nav.update(cx, |nav, cx| nav.go(place, cx));
    }
}

fn menu_items(listing: &Listing, on_screen: &Path, known: &KnownFolders) -> Vec<MenuItem> {
    listing
        .entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Folder)
        .map(|entry| {
            let path = listing.path.join(&entry.name);
            MenuItem {
                icon: known.icon_at(&path),
                label: entry.name.to_string_lossy().into_owned().into(),
                on: on_screen.starts_with(&path),
                path,
            }
        })
        .collect()
}
