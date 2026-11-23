use std::ops::Range;
use std::sync::Arc;

use gpui::{App, Context, RenderImage, Window};
use nova_files_api::{Command, Details, EntryId, EntryKind, FileIcon, ThemeIcon, ThumbnailSize};

use super::MainWindow;
use crate::columns::{self, ColumnText};
use crate::theme_icons;
use crate::view::{RowIcon, entry_kind};
use nova_files_shell::Grid;
use nova_files_shell::device::device_side;

pub struct RowRead {
    pub details: Details,
    pub icon: FileIcon,
}

impl MainWindow {
    pub fn columns(&self, index: usize) -> Option<ColumnText> {
        if index >= self.listing.as_ref()?.entries.len() {
            return None;
        }
        let modified = self
            .details
            .get(&index)
            .and_then(|read| read.details.modified);
        Some(ColumnText {
            modified: modified.map(columns::modified).unwrap_or_default(),
            size: self
                .size_of_row(index)
                .map(columns::size)
                .unwrap_or_default(),
        })
    }

    pub fn theme_icon(&self, index: usize) -> Option<&Arc<RenderImage>> {
        match &self.details.get(&index)?.icon {
            FileIcon::Theme { icon, .. } => self.theme_icons.get(&icon.path),
            FileIcon::BuiltIn(_) => None,
        }
    }

    pub fn row_icon(&self, index: usize, kind: EntryKind) -> RowIcon<'_> {
        if let Some(image) = self.theme_icon(index) {
            return RowIcon::Theme(image);
        }
        match self.details.get(&index).map(|read| &read.icon) {
            Some(FileIcon::Theme { kind, .. } | FileIcon::BuiltIn(kind)) => RowIcon::BuiltIn(*kind),
            None => RowIcon::BuiltIn(entry_kind(kind)),
        }
    }

    pub(super) fn details_arrived(
        &mut self,
        first: EntryId,
        details: &[Details],
        icons: &[FileIcon],
        cx: &mut Context<Self>,
    ) {
        let Some(listing) = &self.listing else { return };
        let Ok(first) = usize::try_from(first.0) else {
            return;
        };
        let entries = listing.entries.get(first..).unwrap_or_default();
        let read = entries
            .iter()
            .zip(details)
            .enumerate()
            .map(|(at, (entry, details))| {
                let icon = icons
                    .get(at)
                    .cloned()
                    .unwrap_or_else(|| FileIcon::BuiltIn(entry_kind(entry.kind)));
                let read = RowRead {
                    details: *details,
                    icon,
                };
                (first + at, read)
            });
        self.details.extend(read);
        if let Some(largest) = details.iter().filter_map(|read| read.size).max() {
            self.shares.seen(largest);
        }
        self.make_theme_icons(icons.iter(), cx);
        cx.notify();
    }

    fn make_theme_icons<'a>(
        &mut self,
        icons: impl Iterator<Item = &'a FileIcon>,
        cx: &mut Context<Self>,
    ) {
        let side = self.icon_side(cx);
        if let Some(gone) = self.theme_icons.resize(side) {
            release(gone, None, cx);
        }
        let mut wanted: Vec<Arc<ThemeIcon>> = Vec::new();
        for icon in icons {
            if let FileIcon::Theme { icon, .. } = icon
                && self.theme_icons.start(icon)
            {
                wanted.push(Arc::clone(icon));
            }
        }
        for icon in wanted {
            let renderer = cx.svg_renderer();
            let making = cx.background_executor().spawn(async move {
                let image = theme_icons::make(&icon, side, &renderer);
                (icon, image)
            });
            cx.spawn(async move |this, cx| {
                let made = making.await;
                this.update(cx, |this, cx| this.theme_icon_ready(made, side, cx))
                    .ok();
            })
            .detach();
        }
    }

    fn theme_icon_ready(
        &mut self,
        (icon, image): (Arc<ThemeIcon>, Option<Arc<RenderImage>>),
        side: u32,
        cx: &mut Context<Self>,
    ) {
        let handed_back = self.theme_icons.finish(icon.path.clone(), side, image);
        release(handed_back, None, cx);
        cx.notify();
    }

    pub(crate) fn rows_drawn(
        &mut self,
        (lines, drawn): (Range<usize>, Grid),
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.sizing.list_width = self.scroll.0.borrow().base_handle.bounds().size.width;
        let grid = self.grid(cx);
        if grid.relaid(&drawn) {
            if let Some(cursor) = self.cursor() {
                self.scroll_to_entry(cursor, cx);
            }
            cx.notify();
        }
        let len = self
            .listing
            .as_ref()
            .map_or(0, |listing| listing.entries.len());
        self.entries_drawn(drawn.entries_of(&lines, len), window, cx);
    }

    pub(crate) fn entries_drawn(
        &mut self,
        range: Range<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.rows_drawn = range.clone();
        self.host
            .entries
            .update(cx, |entries, _| entries.set_drawn(range));
        cx.defer_in(window, |this, _, cx| {
            this.show_rows(this.rows_drawn.clone(), cx);
        });
    }

    fn show_rows(&mut self, visible: Range<usize>, cx: &mut App) {
        let Some(listing) = &self.listing else { return };
        let screen = visible.len();
        let wanted = visible.start.saturating_sub(screen)
            ..visible
                .end
                .saturating_add(screen)
                .min(listing.entries.len());
        if wanted == self.rows_wanted {
            return;
        }
        let (Ok(first), Ok(count)) = (u32::try_from(wanted.start), u32::try_from(wanted.len()))
        else {
            return;
        };
        let command = Command::Rows {
            id: listing.id,
            first: EntryId(first),
            count,
            size: self.thumbnail_size(cx),
            peeks: !self.showing_details(cx),
        };
        if let Some(connection) = &self.connection {
            connection.send(command).ok();
        }
        self.parts.rows_drawn(listing.id, &wanted, cx);
        self.rows_wanted = wanted;
    }

    pub fn set_scale_factor(
        &mut self,
        scale_factor: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.host.entries.read(cx).scale_factor().to_bits() != scale_factor.to_bits() {
            self.host
                .entries
                .update(cx, |entries, _| entries.set_scale_factor(scale_factor));
            self.parts.rescaled(scale_factor, cx);
        }
        self.scale_factor = scale_factor;
        if let Some(gone) = self.theme_icons.resize(self.icon_side(cx)) {
            release(gone, Some(window), cx);
            let icons: Vec<FileIcon> = self
                .details
                .values()
                .map(|read| read.icon.clone())
                .collect();
            self.make_theme_icons(icons.iter(), cx);
        }
        let side = self.thumbnail_side(cx);
        if side == self.sizing.thumbnails_side {
            return;
        }
        self.sizing.thumbnails_side = side;
        self.host
            .entries
            .update(cx, |entries, _| entries.set_thumbnail_side(side));
        self.parts.thumbnail_side(side, cx);
        self.forget_rows(cx);
    }

    pub(super) fn forget_rows(&mut self, cx: &App) {
        if let Some(listing) = &self.listing
            && !self.rows_wanted.is_empty()
            && let Some(connection) = &self.connection
        {
            let forget = Command::Rows {
                id: listing.id,
                first: EntryId(0),
                count: 0,
                size: self.thumbnail_size(cx),
                peeks: !self.showing_details(cx),
            };
            connection.send(forget).ok();
        }
        self.rows_wanted = 0..0;
    }

    fn icon_side(&self, cx: &App) -> u32 {
        device_side(self.icon_slot(cx), self.scale_factor)
    }

    pub(super) fn thumbnail_size(&self, cx: &App) -> ThumbnailSize {
        ThumbnailSize::to_fill(self.thumbnail_side(cx))
    }

    fn thumbnail_side(&self, cx: &App) -> u32 {
        device_side(self.icon_slot(cx), self.scale_factor)
    }
}

// GPUI takes the window being drawn out of its list, so drop_image needs it passed in.
// https://github.com/ImperaLuna/zed/tree/4cc99ed469e7cc566898de34cbf4e4c993ba2d64/crates/gpui
pub(super) fn release(
    images: impl IntoIterator<Item = Arc<RenderImage>>,
    mut window: Option<&mut Window>,
    cx: &mut App,
) {
    for image in images {
        cx.drop_image(image, window.as_deref_mut());
    }
}
