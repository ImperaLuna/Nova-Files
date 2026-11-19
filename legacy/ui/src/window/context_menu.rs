use std::path::PathBuf;

use gpui::{App, Context, Modifiers, Pixels, Point, point};
use nova_files_api::{EntryId, HiddenFiles, ListingId};
use nova_files_folders_api::ViewMode;
use nova_files_shell::{
    Look as ShellLook, MenuAt, MenuGroup, MenuItem as ShellItem, Placed, arrange,
};

use super::MainWindow;
use crate::assets::Icon;
use crate::commands::{
    DETAILS, GALLERY, HIDDEN, ICONS, OPEN, PREVIEW, PROPERTIES, SELECT_ALL, TRASH,
};
use crate::menu::{MenuAction, MenuItem};
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
            (self.under_cursor(cx), self.entry_menu(cx))
        } else {
            (self.list_corner(), self.space_menu(cx))
        };
        self.open_menu_groups(at, groups, true, cx);
    }

    fn has_menus(&self) -> bool {
        !self.picker() && !self.is_on_start() && self.listing.is_some()
    }

    fn arranged(&self, at: MenuAt, window: Vec<Placed>, cx: &App) -> Vec<Vec<MenuItem>> {
        let mut placed = window;
        placed.extend(self.parts.menu(at, &self.scene(cx), cx));
        arrange(placed)
            .into_iter()
            .map(|items| items.into_iter().map(MenuItem::from).collect())
            .collect()
    }

    fn entry_menu(&self, cx: &App) -> Vec<Vec<MenuItem>> {
        if self.folder_rules().replaces_menu {
            return self.arranged(MenuAt::Entry, Vec::new(), cx);
        }
        let one = self.selected_count() == 1;
        let lone = |item: ShellItem| one.then_some(item);
        let open = [
            lone(ShellItem::run(Icon::Open, &OPEN)),
            lone(ShellItem::run(Icon::Eye, &PREVIEW)),
        ];
        let mut groups = self.arranged(
            MenuAt::Entry,
            vec![
                Placed::new(
                    MenuGroup::Open,
                    Placed::FIRST,
                    open.into_iter().flatten().collect(),
                ),
                Placed::new(
                    MenuGroup::Danger,
                    Placed::FIRST,
                    vec![ShellItem::run(Icon::Trash, &TRASH).with_look(ShellLook::Danger)],
                ),
                Placed::new(
                    MenuGroup::Info,
                    Placed::FIRST,
                    vec![ShellItem::run(Icon::Info, &PROPERTIES)],
                ),
            ],
            cx,
        );
        if let Some(folder) = self.lone_folder() {
            let new_tab = MenuItem::new(
                Icon::Plus,
                super::tabs::OPEN_IN_NEW_TAB_LABEL,
                MenuAction::OpenInNewTab(folder),
            );
            match groups.first_mut() {
                Some(first) => first.insert(first.len().min(1), new_tab),
                None => groups.push(vec![new_tab]),
            }
        }
        groups
    }

    fn lone_folder(&self) -> Option<PathBuf> {
        match self.selection()?.1[..] {
            [id] => self.folder_of_entry(id),
            _ => None,
        }
    }

    fn view_items(&self, cx: &App) -> Vec<ShellItem> {
        let showing = self.folder_view(cx).mode;
        [
            (ViewMode::Details, Icon::List, &DETAILS),
            (ViewMode::Icons, Icon::Tiles, &ICONS),
            (ViewMode::Gallery, Icon::Pictures, &GALLERY),
        ]
        .into_iter()
        .map(|(mode, icon, spec)| {
            if mode == showing {
                ShellItem::run(Icon::Check, spec).with_look(ShellLook::On)
            } else {
                ShellItem::run(icon, spec)
            }
        })
        .collect()
    }

    fn space_menu(&self, cx: &Context<Self>) -> Vec<Vec<MenuItem>> {
        let any = self
            .listing
            .as_ref()
            .is_some_and(|listing| !listing.entries.is_empty());
        let select_all = any.then(|| ShellItem::run(Icon::SelectAll, &SELECT_ALL));
        let select = Placed::new(
            MenuGroup::Select,
            Placed::FIRST,
            select_all.into_iter().collect(),
        );
        if self.folder_rules().replaces_menu {
            let after_parts = Placed::new(MenuGroup::Part, Placed::LAST, select.items);
            return self.arranged(MenuAt::Sheet, vec![after_parts], cx);
        }
        if let Some(place) = self.gathered_shown() {
            return vec![
                vec![super::gathering::add_item(place)],
                select.items.into_iter().map(MenuItem::from).collect(),
            ];
        }
        if self.showing_found() {
            return vec![select.items.into_iter().map(MenuItem::from).collect()];
        }
        let hidden = if self.hidden_files == Some(HiddenFiles::Shown) {
            ShellItem::run(Icon::Check, &HIDDEN).with_look(ShellLook::On)
        } else {
            ShellItem::run(Icon::Eye, &HIDDEN)
        };
        self.arranged(
            MenuAt::Sheet,
            vec![
                select,
                Placed::new(MenuGroup::View, Placed::FIRST, self.view_items(cx)),
                Placed::new(MenuGroup::Show, Placed::FIRST, vec![hidden]),
            ],
            cx,
        )
    }

    fn under_cursor(&self, cx: &App) -> Point<Pixels> {
        let grid = self.grid(cx);
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
