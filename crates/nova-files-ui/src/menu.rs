//! A small menu that opens where the pointer was right-clicked, such as
//! a pinned place's "Remove from Places" or a drive's Eject. The window holds at most one;
//! it closes on a choice or on a click outside it. Drawing is
//! in `view/menu`.
//!
//! Each item names what it does as a [`MenuAction`], which the window
//! carries out once the item is chosen, so a menu holds no closures and
//! a test can read what it offers.

use std::path::PathBuf;

use gpui::{Context, Pixels, Point, SharedString};

use nova_files_api::{DriveAction, DriveId};

use crate::assets::Icon;
use crate::drives::ShownDrive;
use crate::window::MainWindow;

/// The menu on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    /// Where its top left corner sits, in window coordinates.
    pub at: Point<Pixels>,
    pub items: Vec<MenuItem>,
}

/// One item of a [`Menu`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub icon: Icon,
    pub label: SharedString,
    pub action: MenuAction,
}

/// What a menu item does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    /// Take a pinned folder out of Places.
    Unpin(PathBuf),
    /// Open a drive, mounting it first when needed.
    OpenDrive(ShownDrive),
    /// Open a drive in a new tab, mounting it first when needed.
    OpenDriveInNewTab(ShownDrive),
    /// Make a drive safe to unplug.
    Eject(DriveId),
}

impl MainWindow {
    /// Opens a menu of `items` at `at`, in place of any menu open.
    pub(crate) fn open_menu(
        &mut self,
        at: Point<Pixels>,
        items: Vec<MenuItem>,
        cx: &mut Context<Self>,
    ) {
        self.menu = (!items.is_empty()).then_some(Menu { at, items });
        cx.notify();
    }

    /// Closes the menu, if one is open.
    pub(crate) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    /// Carries out item `index` of the menu on screen and closes it.
    pub(crate) fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(item) = self
            .menu
            .take()
            .and_then(|menu| menu.items.into_iter().nth(index))
        else {
            return;
        };
        match item.action {
            MenuAction::Unpin(path) => self.unpin(path),
            MenuAction::OpenDrive(drive) => self.open_drive(drive),
            MenuAction::OpenDriveInNewTab(drive) => self.open_drive_in_new_tab(drive, cx),
            MenuAction::Eject(drive) => self.act_on_drive(DriveAction::Eject(drive)),
        }
        cx.notify();
    }

    /// The menu on screen, if any.
    pub const fn menu(&self) -> Option<&Menu> {
        self.menu.as_ref()
    }
}
