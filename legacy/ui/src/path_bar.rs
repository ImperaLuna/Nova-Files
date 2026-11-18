mod typing;

use std::path::{Path, PathBuf};

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui::{Context, Pixels, Point, SharedString, Task};
use nova_files_api::{
    Command, Connection, Connector, EntryKind, Event, Listing, ListingId, Order, Place,
};

use crate::assets::Icon;
use crate::crumbs::{Crumb, folder_icon};
use crate::view::place_icon;
use crate::window::{ChannelSink, MainWindow};

const MENU_LISTING: ListingId = ListingId(0);

pub use typing::PathField;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub path: PathBuf,
    pub label: SharedString,
    pub icon: Icon,
    pub on: bool,
}

impl From<Crumb> for MenuItem {
    fn from(crumb: Crumb) -> Self {
        Self {
            path: crumb.path,
            label: crumb.label,
            icon: crumb.icon,
            on: false,
        }
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
    pub items: Vec<MenuItem>,
    _lister: Option<Lister>,
}

pub struct PathBar {
    pub(crate) menu: Option<PathMenu>,
    dismissed: Option<(MenuOwner, Point<Pixels>)>,
    connector: Connector,
    pub(crate) field: Option<PathField>,
}

struct Lister {
    _connection: Connection,
    _events: Task<()>,
}

impl PathBar {
    pub const fn new(connector: Connector) -> Self {
        Self {
            menu: None,
            dismissed: None,
            field: None,
            connector,
        }
    }
}

impl MainWindow {
    pub(crate) fn path_menu(&self) -> Option<&PathMenu> {
        self.path_bar
            .menu
            .as_ref()
            .filter(|menu| self.folder() == Some(menu.folder.as_path()))
    }

    pub(crate) fn press_path_menu(
        &mut self,
        owner: MenuOwner,
        at: Point<Pixels>,
        items: Vec<MenuItem>,
    ) {
        if let Some(folder) = self.menu_opens(owner, at) {
            self.path_bar.menu = Some(PathMenu {
                owner,
                folder,
                items,
                _lister: None,
            });
        }
    }

    pub(crate) fn press_arrow(
        &mut self,
        index: usize,
        folder: &Path,
        at: Point<Pixels>,
        cx: &Context<Self>,
    ) {
        let owner = MenuOwner::Arrow(index);
        let Some(on_screen) = self.menu_opens(owner, at) else {
            return;
        };
        self.path_bar.menu = self.menu_lister(folder, cx).map(|lister| PathMenu {
            owner,
            folder: on_screen,
            items: Vec::new(),
            _lister: Some(lister),
        });
    }

    fn menu_opens(&mut self, owner: MenuOwner, at: Point<Pixels>) -> Option<PathBuf> {
        if self.path_bar.dismissed.take() == Some((owner, at)) {
            return None;
        }
        self.folder().map(Path::to_path_buf)
    }

    fn menu_lister(&self, folder: &Path, cx: &Context<Self>) -> Option<Lister> {
        let (sink, mut events) = mpsc::unbounded();
        let connection = self.path_bar.connector.connect(ChannelSink(sink)).ok()?;
        connection
            .send(Command::ListFolder {
                id: MENU_LISTING,
                path: folder.to_path_buf(),
                order: Order::default(),
            })
            .ok()?;
        let events = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await
                && this
                    .update(cx, |this, cx| this.menu_event(event, cx))
                    .is_ok()
            {}
        });
        Some(Lister {
            _connection: connection,
            _events: events,
        })
    }

    fn menu_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::FolderListed(listing) => {
                let items = menu_items(&listing, self.folder(), &self.places);
                match &mut self.path_bar.menu {
                    Some(menu) if !items.is_empty() => menu.items = items,
                    _ => self.close_path_menu(),
                }
                cx.notify();
            }
            Event::FolderUnreadable { .. } => {
                self.close_path_menu();
                cx.notify();
            }
            _ => {}
        }
    }

    pub(crate) fn close_path_menu(&mut self) {
        self.path_bar.menu = None;
    }

    pub(crate) fn dismiss_path_menu(&mut self, at: Point<Pixels>) {
        self.path_bar.dismissed = self.path_menu().map(|menu| (menu.owner, at));
        self.close_path_menu();
    }

    pub(crate) fn choose_path_menu_item(&mut self, index: usize) {
        let chosen = self
            .path_menu()
            .and_then(|menu| menu.items.get(index))
            .map(|item| item.path.clone());
        self.close_path_menu();
        if let Some(path) = chosen {
            self.open(&path);
        }
    }

    pub fn path_menu_names(&self) -> Option<Vec<String>> {
        self.path_menu().map(|menu| {
            menu.items
                .iter()
                .map(|item| item.label.to_string())
                .collect()
        })
    }

    pub(crate) fn has_folders(&self) -> bool {
        self.listing
            .as_ref()
            .and_then(|listing| listing.entries.first())
            .is_some_and(|entry| entry.kind == EntryKind::Folder)
    }
}

fn menu_items(listing: &Listing, on_screen: Option<&Path>, places: &[Place]) -> Vec<MenuItem> {
    listing
        .entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Folder)
        .map(|entry| {
            let path = listing.path.join(&entry.name);
            MenuItem {
                icon: folder_icon(&path, places, place_icon),
                label: entry.name.to_string_lossy().into_owned().into(),
                on: on_screen.is_some_and(|folder| folder.starts_with(&path)),
                path,
            }
        })
        .collect()
}
