//! The path bar's state: which of its menus is open, if any, and what it
//! lists. Drawing is in `view::path` and `view::path_menu`; the crumbs
//! themselves are worked out each time the bar draws (see `crumbs`).
//!
//! The "…" crumb's menu lists the folders a long path folds away.
//!
//! A menu belongs to the folder it was opened in: once the window shows
//! another folder, it no longer draws, and the next menu replaces it.
//! A press outside a menu closes it before its button hears the press, so
//! the menu remembers that press: when it lands on the menu's own button,
//! the button leaves the menu closed instead of opening it again.

use std::path::{Path, PathBuf};

use gpui::{Pixels, Point, SharedString};

use crate::assets::Icon;
use crate::crumbs::Crumb;
use crate::window::MainWindow;

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
}

/// An open menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMenu {
    pub owner: MenuOwner,
    /// The folder on screen when it opened.
    pub folder: PathBuf,
    pub items: Vec<MenuItem>,
}

/// A window's path bar.
#[derive(Debug, Default)]
pub struct PathBar {
    pub(crate) menu: Option<PathMenu>,
    /// The menu the last press outside one closed, and where that press
    /// was.
    dismissed: Option<(MenuOwner, Point<Pixels>)>,
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
    /// unless that press just closed it. Whether it opened.
    pub(crate) fn press_path_menu(
        &mut self,
        owner: MenuOwner,
        at: Point<Pixels>,
        items: Vec<MenuItem>,
    ) -> bool {
        if self.path_bar.dismissed.take() == Some((owner, at)) {
            return false;
        }
        let Some(folder) = self.folder().map(Path::to_path_buf) else {
            return false;
        };
        self.path_bar.menu = Some(PathMenu {
            owner,
            folder,
            items,
        });
        true
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
}
