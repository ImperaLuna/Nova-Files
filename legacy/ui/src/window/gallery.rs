use std::ops::Range;

use gpui::{App, Context, Entity, Pixels, Size, Window};
use nova_files_folders_api::ViewMode;
use nova_files_folders_ui::{GalleryBounds, stage_room, strip};
use nova_files_preview_ui::PreviewPart;

use super::MainWindow;
use crate::selection::Move;

impl MainWindow {
    pub(crate) fn preview(&self) -> Option<Entity<PreviewPart>> {
        self.part::<PreviewPart>()
    }

    pub(crate) fn previewing(&self, cx: &App) -> bool {
        self.preview()
            .is_some_and(|preview| preview.read(cx).previewing())
    }

    pub fn showing_gallery(&self, cx: &App) -> bool {
        !self.picker() && self.folder_view(cx).mode == ViewMode::Gallery
    }

    pub(crate) const fn gallery_bounds(&self) -> &GalleryBounds {
        &self.sizing.gallery
    }

    pub(crate) fn stage_room(&self, cx: &App) -> Size<Pixels> {
        stage_room(self.sizing.gallery.get().size, self.grid(cx).cell.height)
    }

    pub(crate) fn strip_entries(&self, len: usize, cx: &App) -> Range<usize> {
        let grid = self.grid(cx);
        strip(
            self.sizing.gallery.get().size.width,
            grid,
            self.cursor().unwrap_or_default(),
            len,
        )
    }

    pub(crate) fn keep_gallery(&mut self, window: &Window, cx: &mut Context<Self>) {
        let preview = self.preview();
        if !self.showing_gallery(cx) {
            if let Some(preview) = preview {
                preview.update(cx, PreviewPart::undock);
            }
            return;
        }
        let Some(listing) = self.listing.as_ref() else {
            return;
        };
        let len = listing.entries.len();
        if len == 0 {
            return;
        }
        if self.cursor().is_none() {
            self.move_cursor(Move::First, false, cx);
        }
        if let (Some(preview), Some(cursor)) = (preview, self.cursor()) {
            preview.update(cx, |preview, cx| preview.dock(cursor, cx));
        }
        self.entries_drawn(self.strip_entries(len, cx), window, cx);
    }
}
