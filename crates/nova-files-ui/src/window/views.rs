//! How the folder on screen is shown: Details, Icons at one of five sizes,
//! or Gallery. Each folder keeps the view it was last given; the core
//! saves them and sends every window the whole list whenever one changes
//! (see the core's `folder_views`), so all windows and tabs agree. A
//! folder never given one shows Details, except the Pictures and Videos
//! places, which start in Icons. Search results and the trash always show
//! Details.
//!
//! Ctrl+1, 2 and 3 pick the view, as does the empty space's right-click
//! menu; Ctrl+plus and minus step the icons' size while Icons shows. A
//! change shows at once and is sent to the core to keep. It stays pending
//! until the core's list holds it, so a list the core sent before it
//! arrived doesn't take it back.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{Context, Pixels, ScrollDelta, ScrollStrategy, px};
use nova_files_api::{Command, FolderView, FolderViews, PlaceKind, ViewMode};

use super::MainWindow;
use crate::grid::{self, Grid};
use crate::theme::size;

/// How far Ctrl+scroll goes for each step of the icons' size: one turn
/// of a wheel, or that far on a touchpad.
const ZOOM_STEP: Pixels = px(40.0);

/// What the view's layout keeps between frames.
#[derive(Debug, Default)]
pub struct Sizing {
    /// How wide the list was when it last drew, which Icons shares out
    /// among its cells (see `grid`).
    pub(super) list_width: Pixels,
    /// Ctrl+scroll not yet taken as a step of the icons' size.
    zoom_scroll: Pixels,
    /// The side, in device pixels, the thumbnails held were shrunk to;
    /// none at first.
    pub(super) thumbnails_side: u32,
}

/// The places whose folders are mostly pictures, so they start in Icons.
const PICTURE_PLACES: [PlaceKind; 2] = [PlaceKind::Pictures, PlaceKind::Videos];

impl MainWindow {
    /// How the folder on screen is shown.
    pub fn folder_view(&self) -> FolderView {
        if self.showing_found() || self.in_trash() {
            return FolderView::default();
        }
        self.folder()
            .map_or_else(FolderView::default, |folder| self.view_of(folder))
    }

    /// Whether the entries on screen show as Icons.
    pub fn showing_icons(&self) -> bool {
        self.folder_view().mode == ViewMode::Icons
    }

    /// How the entries on screen are laid out, across the list's width
    /// when it last drew.
    pub fn grid(&self) -> Grid {
        let width = self.sizing.list_width;
        match self.folder_view() {
            FolderView {
                mode: ViewMode::Icons,
                icons,
            } => Grid::icons(icons, width),
            _ => Grid::details(width),
        }
    }

    /// The side of an entry's icon in the view on screen, which its
    /// thumbnail fills.
    pub(super) fn icon_slot(&self) -> Pixels {
        match self.folder_view() {
            FolderView {
                mode: ViewMode::Icons,
                icons,
            } => grid::icon_side(icons),
            _ => size::THUMBNAIL,
        }
    }

    /// Scrolls the list just far enough to show entry `index`.
    pub(crate) fn scroll_to_entry(&self, index: usize) {
        let line = self.grid().line_of(index);
        self.scroll.scroll_to_item(line, ScrollStrategy::Nearest);
    }

    /// The view `folder` was given, or the one it starts with.
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

    /// Shows the folder on screen in `mode`.
    pub(crate) fn show_as(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        let view = self.folder_view();
        self.change_view(FolderView { mode, ..view }, cx);
    }

    /// Makes the icons one size bigger, or smaller, while Icons shows.
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

    /// Ctrl+scroll over the entries by `delta`: while Icons shows, the
    /// icons grow as it goes up and shrink as it goes down, a step for
    /// each [`ZOOM_STEP`]. Whether it was taken, so the list doesn't
    /// scroll too.
    pub(crate) fn zoom_scrolled(&mut self, delta: ScrollDelta, cx: &mut Context<Self>) -> bool {
        if !self.showing_icons() {
            return false;
        }
        self.sizing.zoom_scroll += delta.pixel_delta(ZOOM_STEP).y;
        while self.sizing.zoom_scroll.abs() >= ZOOM_STEP {
            let bigger = self.sizing.zoom_scroll > Pixels::ZERO;
            self.sizing.zoom_scroll -= if bigger { ZOOM_STEP } else { -ZOOM_STEP };
            self.zoom_icons(bigger, cx);
        }
        true
    }

    /// Gives the folder on screen `view`, here at once and in the core
    /// for good. Search results and the trash keep Details.
    fn change_view(&mut self, view: FolderView, cx: &mut Context<Self>) {
        if self.showing_found() || self.in_trash() || view == self.folder_view() {
            return;
        }
        let Some(folder) = self.folder().map(Path::to_path_buf) else {
            return;
        };
        self.folder_views.insert(folder.clone(), view);
        self.pending_views.insert(folder.clone(), view);
        // Another layout puts the cursor elsewhere.
        if let Some(cursor) = self.cursor() {
            self.scroll_to_entry(cursor);
        }
        if let Some(connection) = &self.connection {
            // A core that stopped can't keep it; the window says so
            // already (see `CoreStatus`).
            connection
                .send(Command::SetFolderView { folder, view })
                .ok();
        }
        cx.notify();
    }

    /// Takes the views the core sent, newest first, in place of those
    /// known before, keeping the changes it hasn't taken yet on top.
    pub(super) fn folder_views_arrived(&mut self, views: &FolderViews, cx: &mut Context<Self>) {
        // Collected oldest first, so the newest view of a folder wins.
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
