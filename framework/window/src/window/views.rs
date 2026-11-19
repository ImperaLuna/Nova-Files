use gpui::{App, ScrollStrategy};
use nova_files_shell::{GalleryBounds, Grid, Layout, Views};

use super::MainWindow;

#[derive(Debug, Default)]
pub struct Sizing {
    pub(super) list_width: gpui::Pixels,
    pub(super) thumbnails_side: u32,
    pub(super) gallery: GalleryBounds,
}

impl MainWindow {
    pub fn details_only(&self) -> bool {
        self.showing_found()
            || self.folder_rules().views == Views::DetailsOnly
            || self
                .listed_page()
                .is_some_and(|page| page.views() == Views::DetailsOnly)
    }

    pub fn layout(&self, cx: &App) -> Layout {
        if self.details_only() {
            return Layout::Details;
        }
        self.folder()
            .and_then(|folder| self.parts.layout(folder, cx))
            .unwrap_or_default()
    }

    pub fn showing_icons(&self, cx: &App) -> bool {
        matches!(self.layout(cx), Layout::Icons { .. })
    }

    pub fn grid(&self, cx: &App) -> Grid {
        self.layout(cx).grid(self.sizing.list_width)
    }

    pub fn showing_details(&self, cx: &App) -> bool {
        self.layout(cx) == Layout::Details
    }

    pub(super) fn icon_slot(&self, cx: &App) -> gpui::Pixels {
        self.grid(cx).icon
    }

    pub(crate) fn scroll_to_entry(&self, index: usize, cx: &App) {
        let line = self.grid(cx).line_of(index);
        self.scroll.scroll_to_item(line, ScrollStrategy::Nearest);
    }
}
