use std::path::Path;

use gpui::{App, Context, Entity, ScrollDelta, ScrollStrategy};
use nova_files_folders_api::{FolderView, IconSize, ViewMode};
use nova_files_folders_ui::{FoldersPart, Grid};
use nova_files_shell::Views;

use super::MainWindow;

#[derive(Debug, Default)]
pub struct Sizing {
    pub(super) list_width: gpui::Pixels,
    pub(super) thumbnails_side: u32,
    pub(super) gallery: nova_files_folders_ui::GalleryBounds,
}

const STRIP_ICONS: IconSize = IconSize::Small;

impl MainWindow {
    fn folders(&self) -> Option<Entity<FoldersPart>> {
        self.part::<FoldersPart>()
    }

    fn details_only(&self) -> bool {
        self.showing_found()
            || self.folder_rules().views == Views::DetailsOnly
            || self
                .listed_page()
                .is_some_and(|page| page.views() == Views::DetailsOnly)
    }

    pub fn folder_view(&self, cx: &App) -> FolderView {
        if self.details_only() {
            return FolderView::default();
        }
        let folders = self.folders();
        self.folder()
            .zip(folders.as_ref())
            .map_or_else(FolderView::default, |(folder, folders)| {
                folders.read(cx).view_of(folder, cx)
            })
    }

    pub fn showing_icons(&self, cx: &App) -> bool {
        self.folder_view(cx).mode == ViewMode::Icons
    }

    pub fn grid(&self, cx: &App) -> Grid {
        let width = self.sizing.list_width;
        match self.folder_view(cx) {
            FolderView {
                mode: ViewMode::Icons,
                icons,
            } => Grid::icons(icons, width),
            _ if self.showing_gallery(cx) => Grid::strip(STRIP_ICONS),
            _ => Grid::details(width),
        }
    }

    pub fn showing_details(&self, cx: &App) -> bool {
        !self.showing_icons(cx) && !self.showing_gallery(cx)
    }

    pub(super) fn icon_slot(&self, cx: &App) -> gpui::Pixels {
        self.grid(cx).icon
    }

    pub(crate) fn scroll_to_entry(&self, index: usize, cx: &App) {
        let line = self.grid(cx).line_of(index);
        self.scroll.scroll_to_item(line, ScrollStrategy::Nearest);
    }

    pub(crate) fn show_as(&self, mode: ViewMode, cx: &mut Context<Self>) {
        let view = self.folder_view(cx);
        self.change_view(FolderView { mode, ..view }, cx);
    }

    pub(crate) fn zoom_icons(&self, bigger: bool, cx: &mut Context<Self>) {
        let view = self.folder_view(cx);
        if view.mode != ViewMode::Icons {
            return;
        }
        let icons = if bigger {
            view.icons.bigger()
        } else {
            view.icons.smaller()
        };
        self.change_view(FolderView { icons, ..view }, cx);
    }

    pub(crate) fn zoom_scrolled(&self, delta: ScrollDelta, cx: &mut Context<Self>) -> bool {
        if !self.showing_icons(cx) {
            return false;
        }
        let Some(folders) = self.folders() else {
            return true;
        };
        let steps = folders.update(cx, |folders, _| folders.zoom_steps(delta));
        if steps != 0 {
            let view = self.folder_view(cx);
            let icons = view.icons.stepped(steps);
            self.change_view(FolderView { icons, ..view }, cx);
        }
        true
    }

    fn change_view(&self, view: FolderView, cx: &mut Context<Self>) {
        if self.details_only() || view == self.folder_view(cx) {
            return;
        }
        let (Some(folder), Some(folders)) = (self.folder().map(Path::to_path_buf), self.folders())
        else {
            return;
        };
        folders.update(cx, |folders, cx| folders.set_view(folder, view, cx));
        if let Some(cursor) = self.cursor() {
            self.scroll_to_entry(cursor, cx);
        }
        cx.notify();
    }
}
