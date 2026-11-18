use std::path::PathBuf;

use gpui::{App, Context, Modifiers, Pixels, Point, point};
use nova_files_shell::MenuAt;
use nova_files_api::{EntryId, HiddenFiles, ListingId, ViewMode};

use super::MainWindow;
use crate::assets::Icon;
use crate::commands::{
    COPY, CUT, DETAILS, EMPTY_TRASH, GALLERY, HIDDEN, ICONS, NEW_FOLDER, OPEN, PASTE, PREVIEW,
    PROPERTIES, RENAME, RESTORE, SELECT_ALL, TRASH,
};
use crate::menu::{Look, MenuAction, MenuItem};
use crate::selection::{Click, Selection};
use crate::theme::size;

impl MainWindow {
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
        let groups = self.entry_menu(cx);
        self.open_menu_groups(at, groups, false, cx);
    }

    pub(crate) fn space_right_clicked(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        if self.has_menus() {
            let groups = self.space_menu(cx);
            self.open_menu_groups(at, groups, false, cx);
        }
    }

    pub(crate) fn menu_key(&mut self, cx: &mut Context<Self>) {
        if !self.has_menus() {
            return;
        }
        let (at, groups) = if self.selected_count() > 0 {
            (self.under_cursor(), self.entry_menu(cx))
        } else {
            (self.list_corner(), self.space_menu(cx))
        };
        self.open_menu_groups(at, groups, true, cx);
    }

    fn has_menus(&self) -> bool {
        self.pick.is_none() && !self.is_on_start() && self.listing.is_some()
    }

    fn part_menu(&self, at: MenuAt, cx: &App) -> Vec<Vec<MenuItem>> {
        self.parts
            .menu(at, &self.scene(), cx)
            .into_iter()
            .map(|items| items.into_iter().map(MenuItem::from).collect())
            .collect()
    }

    fn entry_menu(&self, cx: &App) -> Vec<Vec<MenuItem>> {
        if self.trash_open() {
            return vec![vec![MenuItem::run(Icon::Undo, &RESTORE)]];
        }
        let from_parts = self.part_menu(MenuAt::Entry, cx);
        if self.folder_rules().replaces_menu {
            return from_parts;
        }
        let one = self.selected_count() == 1;
        let lone = |item: MenuItem| one.then_some(item);
        let rename = lone(MenuItem::run(Icon::Rename, &RENAME)).filter(|_| self.folder().is_some());
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
                rename,
            ]
            .into_iter()
            .flatten()
            .collect(),
            vec![MenuItem::run(Icon::Trash, &TRASH).with_look(Look::Danger)],
            vec![MenuItem::run(Icon::Info, &PROPERTIES)],
        ]
        .into_iter()
        .chain(from_parts)
        .collect()
    }

    fn lone_folder(&self) -> Option<PathBuf> {
        match self.selection()?.1[..] {
            [id] => self.folder_of_entry(id),
            _ => None,
        }
    }

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

    fn space_menu(&self, cx: &Context<Self>) -> Vec<Vec<MenuItem>> {
        let any = self
            .listing
            .as_ref()
            .is_some_and(|listing| !listing.entries.is_empty());
        let select_all = any.then(|| MenuItem::run(Icon::SelectAll, &SELECT_ALL));
        let from_parts = self.part_menu(MenuAt::Sheet, cx);
        if self.folder_rules().replaces_menu && !from_parts.is_empty() {
            return from_parts
                .into_iter()
                .chain([select_all.into_iter().collect()])
                .collect();
        }
        if self.folder_rules().replaces_menu {
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
        if let Some(place) = self.gathered_shown() {
            return vec![
                vec![super::gathering::add_item(place)],
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
        .into_iter()
        .chain(from_parts)
        .collect()
    }

    fn under_cursor(&self) -> Point<Pixels> {
        let grid = self.grid();
        let state = self.scroll.0.borrow();
        let list = state.base_handle.bounds();
        let scrolled = state.base_handle.offset().y;
        let entry = self.cursor().unwrap_or_default();
        #[expect(clippy::cast_precision_loss, reason = "a line index fits in f32")]
        let (line, column) = (grid.line_of(entry) as f32, grid.column_of(entry) as f32);
        let below = grid.cell.height * (line + 1.0);
        let y = (list.top() + scrolled + below).clamp(list.top(), list.bottom());
        point(list.left() + grid.cell.width * column + size::ROW_PAD, y)
    }

    fn list_corner(&self) -> Point<Pixels> {
        let list = self.scroll.0.borrow().base_handle.bounds();
        point(list.left() + size::ROW_PAD, list.top() + size::ROW_PAD)
    }
}
