use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{Context, Pixels, ScrollDelta, ScrollStrategy, px};
use nova_files_api::PlaceKind;
use nova_files_folders_api::{Command, FolderView, FolderViews, IconSize, ViewMode};
use nova_files_shell::Views;

use super::MainWindow;
use crate::grid::Grid;

const ZOOM_STEP: Pixels = px(40.0);

#[derive(Debug, Default)]
pub struct Sizing {
    pub(super) list_width: Pixels,
    zoom_scroll: Pixels,
    pub(super) thumbnails_side: u32,
    pub(super) gallery: super::gallery::GalleryBounds,
}

const STRIP_ICONS: IconSize = IconSize::Small;

const PICTURE_PLACES: [PlaceKind; 2] = [PlaceKind::Pictures, PlaceKind::Videos];

impl MainWindow {
    pub fn folder_view(&self) -> FolderView {
        if self.showing_found() || self.folder_rules().views == Views::DetailsOnly {
            return FolderView::default();
        }
        self.folder()
            .map_or_else(FolderView::default, |folder| self.view_of(folder))
    }

    pub fn showing_icons(&self) -> bool {
        self.folder_view().mode == ViewMode::Icons
    }

    pub fn grid(&self) -> Grid {
        let width = self.sizing.list_width;
        match self.folder_view() {
            FolderView {
                mode: ViewMode::Icons,
                icons,
            } => Grid::icons(icons, width),
            _ if self.showing_gallery() => Grid::strip(STRIP_ICONS),
            _ => Grid::details(width),
        }
    }

    pub fn showing_details(&self) -> bool {
        !self.showing_icons() && !self.showing_gallery()
    }

    pub(super) fn icon_slot(&self) -> Pixels {
        self.grid().icon
    }

    pub(crate) fn scroll_to_entry(&self, index: usize) {
        let line = self.grid().line_of(index);
        self.scroll.scroll_to_item(line, ScrollStrategy::Nearest);
    }

    fn view_of(&self, folder: &Path) -> FolderView {
        if let Some(view) = self.folder_views.get(folder) {
            return *view;
        }
        let pictures = self
            .places
            .iter()
            .any(|place| PICTURE_PLACES.contains(&place.kind) && folder.starts_with(&place.path));
        let mode = if pictures {
            ViewMode::Icons
        } else {
            ViewMode::Details
        };
        FolderView {
            mode,
            ..FolderView::default()
        }
    }

    pub(crate) fn show_as(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        let view = self.folder_view();
        self.change_view(FolderView { mode, ..view }, cx);
    }

    pub(crate) fn zoom_icons(&mut self, bigger: bool, cx: &mut Context<Self>) {
        let view = self.folder_view();
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

    pub(crate) fn zoom_scrolled(&mut self, delta: ScrollDelta, cx: &mut Context<Self>) -> bool {
        if !self.showing_icons() {
            return false;
        }
        self.sizing.zoom_scroll += delta.pixel_delta(ZOOM_STEP).y;
        let steps = (self.sizing.zoom_scroll / ZOOM_STEP).trunc();
        if steps != 0.0 {
            self.sizing.zoom_scroll -= ZOOM_STEP * steps;
            let view = self.folder_view();
            #[expect(clippy::cast_possible_truncation, reason = "a few steps of a wheel")]
            let icons = view.icons.stepped(steps as isize);
            self.change_view(FolderView { icons, ..view }, cx);
        }
        true
    }

    fn change_view(&mut self, view: FolderView, cx: &mut Context<Self>) {
        if self.showing_found()
            || self.folder_rules().views == Views::DetailsOnly
            || view == self.folder_view()
        {
            return;
        }
        let Some(folder) = self.folder().map(Path::to_path_buf) else {
            return;
        };
        self.folder_views.insert(folder.clone(), view);
        self.pending_views.insert(folder.clone(), view);
        if let Some(cursor) = self.cursor() {
            self.scroll_to_entry(cursor);
        }
        if let Some(connection) = &self.connection {
            connection.send(Command::SetView { folder, view }).ok();
        }
        cx.notify();
    }

    pub(super) fn folder_views_arrived(&mut self, views: &FolderViews, cx: &mut Context<Self>) {
        let mut known = views.iter().rev().cloned().collect::<HashMap<PathBuf, _>>();
        self.pending_views
            .retain(|folder, view| known.get(folder) != Some(view));
        known.extend(
            self.pending_views
                .iter()
                .map(|(folder, view)| (folder.clone(), *view)),
        );
        self.folder_views = known;
        cx.notify();
    }
}
