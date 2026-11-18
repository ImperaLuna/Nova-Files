use std::ops::Range;
use std::sync::Arc;

use gpui::{App, Context, RenderImage, Window};
use nova_files_api::{
    Command, Details, EntryId, EntryKind, FileIcon, ListingId, ThemeIcon, ThumbnailSize,
};
use nova_files_settings_api::Thumbnails;

use super::MainWindow;
use crate::columns::{self, ColumnText};
use crate::grid::Grid;
use crate::view::{RowIcon, entry_kind};
use crate::{theme_icons, thumbnail};

pub struct RowRead {
    pub details: Details,
    pub icon: FileIcon,
}

struct Shrunk {
    id: ListingId,
    index: usize,
    side: u32,
    image: RenderImage,
}

impl MainWindow {
    pub fn columns(&self, index: usize) -> Option<ColumnText> {
        if index >= self.listing.as_ref()?.entries.len() {
            return None;
        }
        let details = self
            .details
            .get(&index)
            .map(|read| read.details)
            .unwrap_or_default();
        Some(ColumnText {
            modified: details.modified.map(columns::modified).unwrap_or_default(),
            size: details.size.map(columns::size).unwrap_or_default(),
        })
    }

    pub fn thumbnail(&self, index: usize) -> Option<&Arc<RenderImage>> {
        self.thumbnails.get(index)
    }

    pub fn theme_icon(&self, index: usize) -> Option<&Arc<RenderImage>> {
        match &self.details.get(&index)?.icon {
            FileIcon::Theme { icon, .. } => self.theme_icons.get(&icon.path),
            FileIcon::BuiltIn(_) => None,
        }
    }

    pub fn row_icon(&self, index: usize, kind: EntryKind) -> RowIcon<'_> {
        if let Some(image) = self.thumbnail(index) {
            return RowIcon::Thumbnail(image);
        }
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
        self.make_theme_icons(icons.iter(), cx);
        cx.notify();
    }

    fn make_theme_icons<'a>(
        &mut self,
        icons: impl Iterator<Item = &'a FileIcon>,
        cx: &mut Context<Self>,
    ) {
        let side = self.icon_side();
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
        let grid = self.grid();
        if grid.relaid(&drawn) {
            if let Some(cursor) = self.cursor() {
                self.scroll_to_entry(cursor);
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
        self.rows_drawn = range;
        cx.defer_in(window, |this, window, cx| {
            this.show_rows(this.rows_drawn.clone(), window, cx);
        });
    }

    fn show_rows(&mut self, visible: Range<usize>, window: &mut Window, cx: &mut App) {
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
            size: self.thumbnail_size(),
        };
        if let Some(connection) = &self.connection {
            connection.send(command).ok();
        }
        release(self.thumbnails.keep_rows(&wanted), Some(window), cx);
        self.rows_wanted = wanted;
    }

    pub fn set_scale_factor(
        &mut self,
        scale_factor: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.scale_factor = scale_factor;
        if let Some(gone) = self.theme_icons.resize(self.icon_side()) {
            release(gone, Some(window), cx);
            let icons: Vec<FileIcon> = self
                .details
                .values()
                .map(|read| read.icon.clone())
                .collect();
            self.make_theme_icons(icons.iter(), cx);
        }
        let side = self.thumbnail_side();
        if side == self.sizing.thumbnails_side {
            return;
        }
        self.sizing.thumbnails_side = side;
        release(self.thumbnails.take_all(), Some(window), cx);
        self.forget_rows();
    }

    pub(super) fn forget_rows(&mut self) {
        if let Some(listing) = &self.listing
            && !self.rows_wanted.is_empty()
            && let Some(connection) = &self.connection
        {
            let forget = Command::Rows {
                id: listing.id,
                first: EntryId(0),
                count: 0,
                size: self.thumbnail_size(),
            };
            connection.send(forget).ok();
        }
        self.rows_wanted = 0..0;
    }

    fn icon_side(&self) -> u32 {
        thumbnail::device_side(self.icon_slot(), self.scale_factor)
    }

    pub(super) fn thumbnail_size(&self) -> ThumbnailSize {
        ThumbnailSize::to_fill(self.thumbnail_side())
    }

    fn thumbnail_side(&self) -> u32 {
        thumbnail::device_side(self.icon_slot(), self.scale_factor)
    }

    pub(super) fn thumbnail_arrived(
        &self,
        (id, entry, size): (ListingId, EntryId, ThumbnailSize),
        png: Arc<[u8]>,
        cx: &Context<Self>,
    ) {
        if let Ok(index) = usize::try_from(entry.0)
            && size == self.thumbnail_size()
            && self.rows_wanted.contains(&index)
            && self.settings.thumbnails == Thumbnails::Shown
        {
            self.shrink_thumbnail(id, index, png, cx);
        }
    }

    fn shrink_thumbnail(&self, id: ListingId, index: usize, png: Arc<[u8]>, cx: &Context<Self>) {
        let side = self.thumbnail_side();
        let shrinking = cx
            .background_executor()
            .spawn(async move { thumbnail::shrink(&png, side) });
        cx.spawn(async move |this, cx| {
            let Some(image) = shrinking.await else { return };
            let made = Shrunk {
                id,
                index,
                side,
                image,
            };
            this.update(cx, |this, cx| this.thumbnail_ready(made, cx))
                .ok();
        })
        .detach();
    }

    fn thumbnail_ready(&mut self, made: Shrunk, cx: &mut Context<Self>) {
        if self.listing.as_ref().map(|listing| listing.id) == Some(made.id)
            && self.rows_wanted.contains(&made.index)
            && made.side == self.thumbnail_side()
            && self.settings.thumbnails == Thumbnails::Shown
        {
            let replaced = self.thumbnails.insert(made.index, Arc::new(made.image));
            release(replaced, None, cx);
            cx.notify();
        }
    }
}

// GPUI takes the window being drawn out of its list, so drop_image needs it passed in.
// https://github.com/ImperaLuna/zed/tree/nova/v0.233.10/crates/gpui
pub(super) fn release(
    images: impl IntoIterator<Item = Arc<RenderImage>>,
    mut window: Option<&mut Window>,
    cx: &mut App,
) {
    for image in images {
        cx.drop_image(image, window.as_deref_mut());
    }
}
