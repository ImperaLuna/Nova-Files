//! The right-click menus on the sheet, kept short as on the design page.
//!
//! On an entry: Open, Open in new tab (a folder) and Preview, Cut, Copy
//! and Rename, then Move to Trash; with several selected only what applies to all of them (Cut,
//! Copy, Move to Trash). A right-click on an entry that isn't selected
//! selects it alone first; one inside the selection keeps it. In the
//! trash an entry offers Restore; inside a trashed folder, nothing.
//!
//! On empty space: New folder and Paste (only with files on the
//! clipboard), Select all (with something to select), Details, Icons and
//! Gallery (a tick on the view showing), then Show hidden files, which
//! shows a tick while they show. In the trash: Empty Trash
//! while any trash holds anything, and Select all. A search's files aren't a folder, so there
//! only Select all is offered. The Start page and a picker have no menu.
//!
//! Each item sends the window action its key sends (see `menu`). The Menu
//! key (or Shift+F10) opens the same menu from the keyboard, under the
//! row the keyboard is on, or at the list's top for empty space.

use std::path::PathBuf;

use gpui::{Context, Modifiers, Pixels, Point, point};
use nova_files_api::{EntryId, HiddenFiles, ListingId, ViewMode};

use super::MainWindow;
use crate::assets::Icon;
use crate::commands::{
    COPY, CUT, DETAILS, EMPTY_TRASH, GALLERY, HIDDEN, ICONS, NEW_FOLDER, OPEN, PASTE, PREVIEW,
    RENAME, RESTORE, SELECT_ALL, TRASH,
};
use crate::menu::{Look, MenuAction, MenuItem};
use crate::selection::{Click, Selection};
use crate::theme::size;

impl MainWindow {
    /// A right-click at `at` on the entry `id` of `listing`.
    pub(crate) fn entry_right_clicked(
        &mut self,
        (listing, id): (ListingId, EntryId),
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !self.has_menus() {
            return;
        }
        if !self.selects((listing, id)) {
            let only = Click::of(Modifiers::none(), true);
            self.reselect(
                |before, picks| Some(Selection::clicked(before, listing, id, only, picks)),
                cx,
            );
        }
        let groups = self.entry_menu();
        self.open_menu_groups(at, groups, false, cx);
    }

    /// A right-click at `at` on the sheet's empty space.
    pub(crate) fn space_right_clicked(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        if self.has_menus() {
            let groups = self.space_menu(cx);
            self.open_menu_groups(at, groups, false, cx);
        }
    }

    /// The Menu key: the selection's menu under the row the keyboard is
    /// on, or the empty space's menu at the list's top; its first item
    /// marked.
    pub(crate) fn menu_key(&mut self, cx: &mut Context<Self>) {
        if !self.has_menus() {
            return;
        }
        let (at, groups) = if self.selected_count() > 0 {
            (self.under_cursor(), self.entry_menu())
        } else {
            (self.list_corner(), self.space_menu(cx))
        };
        self.open_menu_groups(at, groups, true, cx);
    }

    /// Whether right-clicks on the sheet open a menu: a main window,
    /// showing a folder or a search's files.
    fn has_menus(&self) -> bool {
        self.pick.is_none() && !self.is_on_start() && self.listing.is_some()
    }

    /// The menu for the entries selected.
    fn entry_menu(&self) -> Vec<Vec<MenuItem>> {
        if self.trash_open() {
            return vec![vec![MenuItem::run(Icon::Undo, &RESTORE)]];
        }
        // Inside a trashed folder nothing can be changed or put back on
        // its own.
        if self.in_trash() {
            return Vec::new();
        }
        let one = self.selected_count() == 1;
        let lone = |item: MenuItem| one.then_some(item);
        let new_tab = self.lone_folder().map(|folder| {
            MenuItem::new(
                Icon::Plus,
                super::tabs::OPEN_IN_NEW_TAB_LABEL,
                MenuAction::OpenInNewTab(folder),
            )
        });
        vec![
            [
                lone(MenuItem::run(Icon::Open, &OPEN)),
                new_tab,
                lone(MenuItem::run(Icon::Eye, &PREVIEW)),
            ]
            .into_iter()
            .flatten()
            .collect(),
            [
                Some(MenuItem::run(Icon::Cut, &CUT)),
                Some(MenuItem::run(Icon::Copy, &COPY)),
                lone(MenuItem::run(Icon::Rename, &RENAME)),
            ]
            .into_iter()
            .flatten()
            .collect(),
            vec![MenuItem::run(Icon::Trash, &TRASH).with_look(Look::Danger)],
        ]
    }

    /// The folder selected, when it is the only entry selected.
    fn lone_folder(&self) -> Option<PathBuf> {
        match self.selection()?.1[..] {
            [id] => self.folder_of_entry(id),
            _ => None,
        }
    }

    /// One item for each view, the one showing ticked.
    fn view_items(&self) -> Vec<MenuItem> {
        let showing = self.folder_view().mode;
        [
            (ViewMode::Details, Icon::List, &DETAILS),
            (ViewMode::Icons, Icon::Tiles, &ICONS),
            (ViewMode::Gallery, Icon::Pictures, &GALLERY),
        ]
        .into_iter()
        .map(|(mode, icon, spec)| {
            if mode == showing {
                MenuItem::run(Icon::Check, spec).with_look(Look::On)
            } else {
                MenuItem::run(icon, spec)
            }
        })
        .collect()
    }

    /// The menu for the sheet's empty space.
    fn space_menu(&self, cx: &Context<Self>) -> Vec<Vec<MenuItem>> {
        let any = self
            .listing
            .as_ref()
            .is_some_and(|listing| !listing.entries.is_empty());
        let select_all = any.then(|| MenuItem::run(Icon::SelectAll, &SELECT_ALL));
        if self.in_trash() {
            // Every trash counts, not only the one listed here.
            let full = self.trash_open() && self.trash_count().is_some_and(|count| count > 0);
            let empty_trash = full.then(|| MenuItem::run(Icon::Trash, &EMPTY_TRASH));
            return vec![
                empty_trash
                    .map(|item| item.with_look(Look::Danger))
                    .into_iter()
                    .collect(),
                select_all.into_iter().collect(),
            ];
        }
        if self.showing_found() {
            return vec![select_all.into_iter().collect()];
        }
        let paste = Self::clipboard_has_files(cx).then(|| MenuItem::run(Icon::Paste, &PASTE));
        let hidden = if self.settings.hidden_files == HiddenFiles::Shown {
            MenuItem::run(Icon::Check, &HIDDEN).with_look(Look::On)
        } else {
            MenuItem::run(Icon::Eye, &HIDDEN)
        };
        vec![
            [Some(MenuItem::run(Icon::NewFolder, &NEW_FOLDER)), paste]
                .into_iter()
                .flatten()
                .collect(),
            select_all.into_iter().collect(),
            self.view_items(),
            vec![hidden],
        ]
    }

    /// Just under the row the keyboard is on, kept within the list.
    fn under_cursor(&self) -> Point<Pixels> {
        let state = self.scroll.0.borrow();
        let list = state.base_handle.bounds();
        let scrolled = state.base_handle.offset().y;
        let row = self.cursor().unwrap_or_default();
        // Rows are counted in screens of a few thousand at most.
        #[expect(clippy::cast_precision_loss, reason = "a row index fits in f32")]
        let below = size::ROW_HEIGHT * (row as f32 + 1.0);
        let y = (list.top() + scrolled + below).clamp(list.top(), list.bottom());
        point(list.left() + size::ROW_PAD, y)
    }

    /// The top left of the list.
    fn list_corner(&self) -> Point<Pixels> {
        let list = self.scroll.0.borrow().base_handle.bounds();
        point(list.left() + size::ROW_PAD, list.top() + size::ROW_PAD)
    }
}
