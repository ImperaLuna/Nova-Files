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

use gpui::Context;
use nova_files_api::{Command, FolderView, FolderViews, PlaceKind, ViewMode};

use super::MainWindow;

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
        if let Some(connection) = &self.connection {
            connection
                .send(Command::SetFolderView { folder, view })
                .ok();
        }
        cx.notify();
    }

    /// Takes the views the core sent, newest first, in place of those
    /// known before, keeping the changes it hasn't taken yet on top.
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
