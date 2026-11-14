//! The path bar's state: which of its menus is open, if any, and what it
//! lists. Drawing is in `view::path` and `view::path_menu`; the crumbs
//! themselves are worked out each time the bar draws (see `crumbs`).
//!
//! Two menus: the "…" crumb's, listing the folders a long path folds
//! away, and an arrow's, listing the folders in the crumb before it. An
//! arrow's menu asks the core for that folder over a connection of its
//! own, so its listing never takes the place of the one on screen (the
//! core serves one listing per client). The menu opens empty and fills
//! when the answer comes; a folder that can't be read, or holds no
//! folders, closes it. The connection lives as long as the menu, so the
//! core lets go of the listing once the menu closes, and a closed menu's
//! answer has nowhere to land.
//!
//! A menu belongs to the folder it was opened in: once the window shows
//! another folder, it no longer draws, and the next menu replaces it.
//! A press outside a menu closes it before its button hears the press, so
//! the menu remembers that press: when it lands on the menu's own button,
//! the button leaves the menu closed instead of opening it again.

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

/// The one listing an arrow menu's connection asks for.
const MENU_LISTING: ListingId = ListingId(0);

/// One folder in a path bar menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub path: PathBuf,
    pub label: SharedString,
    pub icon: Icon,
    /// Whether the folder on screen is this folder or inside it.
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

/// What a menu hangs from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuOwner {
    /// The "…" crumb a long path folds its middle into.
    Folded,
    /// The arrow after the crumb at this index.
    Arrow(usize),
}

/// An open menu.
pub struct PathMenu {
    pub owner: MenuOwner,
    /// The folder on screen when it opened.
    pub folder: PathBuf,
    pub items: Vec<MenuItem>,
    /// An arrow menu's own line to the core, which its folders come
    /// over.
    _lister: Option<Lister>,
}

/// A window's path bar.
pub struct PathBar {
    pub(crate) menu: Option<PathMenu>,
    /// The menu the last press outside one closed, and where that press
    /// was.
    dismissed: Option<(MenuOwner, Point<Pixels>)>,
    connector: Connector,
}

/// An arrow menu's connection to the core and the task taking its events.
struct Lister {
    _connection: Connection,
    _events: Task<()>,
}

impl PathBar {
    pub const fn new(connector: Connector) -> Self {
        Self {
            menu: None,
            dismissed: None,
            connector,
        }
    }
}

impl MainWindow {
    /// The path bar's open menu, if it belongs to the folder on screen.
    pub(crate) fn path_menu(&self) -> Option<&PathMenu> {
        self.path_bar
            .menu
            .as_ref()
            .filter(|menu| self.folder() == Some(menu.folder.as_path()))
    }

    /// Opens `items` as `owner`'s menu for a press at `at` on its button,
    /// unless that press just closed it.
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

    /// Opens the menu of the arrow after crumb `index`, which lists the
    /// folders in `folder`, for a press at `at`.
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

    /// Whether a press at `at` on `owner`'s button opens its menu: not
    /// when that press just closed it, nor before a folder is on screen.
    /// The folder on screen, when it does.
    fn menu_opens(&mut self, owner: MenuOwner, at: Point<Pixels>) -> Option<PathBuf> {
        if self.path_bar.dismissed.take() == Some((owner, at)) {
            return None;
        }
        self.folder().map(Path::to_path_buf)
    }

    /// A new line to the core that asks for `folder`'s listing and hands
    /// the answer to [`Self::menu_event`]; none when the core has stopped.
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
        // Ends with the menu, the window or the core.
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

    /// Fills the open menu from its listing, or closes it when its folder
    /// can't be read or holds no folders.
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

    /// Closes the path bar's menu.
    pub(crate) fn close_path_menu(&mut self) {
        self.path_bar.menu = None;
    }

    /// Closes the path bar's menu for a press at `at` outside it.
    pub(crate) fn dismiss_path_menu(&mut self, at: Point<Pixels>) {
        self.path_bar.dismissed = self.path_menu().map(|menu| (menu.owner, at));
        self.close_path_menu();
    }

    /// Goes to the `index`th folder of the open menu and closes it.
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

    /// The names in the path bar's open menu, empty while it waits for
    /// them; none when no menu is open.
    pub fn path_menu_names(&self) -> Option<Vec<String>> {
        self.path_menu().map(|menu| {
            menu.items
                .iter()
                .map(|item| item.label.to_string())
                .collect()
        })
    }

    /// Whether the folder on screen holds a folder, so its arrow has a
    /// menu to open. Folders come first in every order, so the first
    /// entry tells.
    pub(crate) fn has_folders(&self) -> bool {
        self.listing
            .as_ref()
            .and_then(|listing| listing.entries.first())
            .is_some_and(|entry| entry.kind == EntryKind::Folder)
    }
}

/// The folders of `listing` as menu items, in its order; the one the
/// folder `on_screen` is in, or is, marked.
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
