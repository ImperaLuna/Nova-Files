//! The rows on screen: which rows the core is told about, the details,
//! icons and thumbnails it sends for them, and their text. Thumbnails are
//! shrunk off the UI thread to the size they're drawn (see `thumbnail`),
//! and one the window lets go of is dropped from GPUI's image atlas too
//! (see `shown_thumbnails`). Theme icons are made the same way, once per
//! icon (see `theme_icons`).

use std::ops::Range;
use std::sync::Arc;

use gpui::{App, Context, RenderImage, Window};
use nova_files_api::{
    Command, Details, EntryId, EntryKind, FileIcon, ListingId, ThemeIcon, Thumbnails,
};

use super::MainWindow;
use crate::columns::{self, ColumnText};
use crate::theme::size;
use crate::view::{RowIcon, entry_kind};
use crate::{theme_icons, thumbnail};

/// What the core sent for one row.
pub struct RowRead {
    pub details: Details,
    pub icon: FileIcon,
}

/// A thumbnail shrunk for row `index` of listing `id`, `side` device
/// pixels across.
struct Shrunk {
    id: ListingId,
    index: usize,
    side: u32,
    image: RenderImage,
}

impl MainWindow {
    /// The Modified and Size text of entry `index` on screen, empty until
    /// the core has read that entry's details.
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

    /// The thumbnail of row `index`, once the core has sent it.
    pub fn thumbnail(&self, index: usize) -> Option<&Arc<RenderImage>> {
        self.thumbnails.get(index)
    }

    /// The theme icon of row `index`, once made.
    pub fn theme_icon(&self, index: usize) -> Option<&Arc<RenderImage>> {
        match &self.details.get(&index)?.icon {
            FileIcon::Theme { icon, .. } => self.theme_icons.get(&icon.path),
            FileIcon::BuiltIn(_) => None,
        }
    }

    /// What row `index`, an entry of `kind`, shows in its icon slot: its
    /// thumbnail, else its theme icon once made, else the built-in icon
    /// for what the core says it is (until it says, for `kind`).
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

    /// Keeps the details and icons of the listing's rows `first` onward,
    /// and starts making the theme icons not made yet.
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
        // A row the core sent no icon for draws its entry's built-in one.
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

    /// Makes each theme icon of `icons` not made or being made yet, off
    /// the UI thread, at the row's size.
    fn make_theme_icons<'a>(
        &mut self,
        icons: impl Iterator<Item = &'a FileIcon>,
        cx: &mut Context<Self>,
    ) {
        let side = self.icon_side();
        if self.theme_icons.side() != side {
            release(self.theme_icons.resize(side), None, cx);
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

    /// Keeps a theme icon made at `side`. One that didn't decode is no
    /// longer being made: its rows keep their built-in icon, and it is
    /// tried again when the rows next arrive.
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

    /// The list drew rows `range`. It also draws its first row alone, each
    /// frame, to measure it; asking for that row alone would swap
    /// the request back and forth every frame, and each answer would draw
    /// another frame. The rows on screen are the last range drawn, so the
    /// request waits until the frame is done.
    pub(crate) fn rows_drawn(
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

    /// Rows `visible` are on screen: asks for their details and thumbnails
    /// and one screen either side, and forgets thumbnails now out of range.
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
        };
        if let Some(connection) = &self.connection {
            connection.send(command).ok();
        }
        release(self.thumbnails.keep_rows(&wanted), Some(window), cx);
        self.rows_wanted = wanted;
    }

    /// The window is drawing at `scale_factor`. Thumbnails shrunk for
    /// another scale (the window moved to another screen) are dropped and
    /// asked for again: an empty request first, so the core sends them anew.
    /// Theme icons are dropped and made again from the icons the rows have.
    pub fn set_scale_factor(
        &mut self,
        scale_factor: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = self.thumbnail_side();
        self.scale_factor = scale_factor;
        if self.theme_icons.side() != self.icon_side() {
            release(self.theme_icons.resize(self.icon_side()), Some(window), cx);
            let icons: Vec<FileIcon> = self
                .details
                .values()
                .map(|read| read.icon.clone())
                .collect();
            self.make_theme_icons(icons.iter(), cx);
        }
        if self.thumbnail_side() == before {
            return;
        }
        release(self.thumbnails.take_all(), Some(window), cx);
        self.forget_rows();
    }

    /// Tells the core no rows are wanted, so it counts every thumbnail as
    /// not sent and makes them again when the next frame asks for its
    /// rows.
    pub(super) fn forget_rows(&mut self) {
        if let Some(listing) = &self.listing
            && !self.rows_wanted.is_empty()
            && let Some(connection) = &self.connection
        {
            let forget = Command::Rows {
                id: listing.id,
                first: EntryId(0),
                count: 0,
            };
            connection.send(forget).ok();
        }
        self.rows_wanted = 0..0;
    }

    /// A theme icon's side in device pixels at the current scale.
    fn icon_side(&self) -> u32 {
        thumbnail::device_side(size::ROW_ICON, self.scale_factor)
    }

    /// A thumbnail's side in device pixels at the current scale.
    fn thumbnail_side(&self) -> u32 {
        thumbnail::device_side(size::THUMBNAIL, self.scale_factor)
    }

    /// Shrinks row `index`'s thumbnail off the UI thread, then keeps it.
    pub(super) fn shrink_thumbnail(
        &self,
        id: ListingId,
        index: usize,
        png: Arc<[u8]>,
        cx: &Context<Self>,
    ) {
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

    /// Keeps a thumbnail shrunk to `side` if its row is still wanted and
    /// the window still draws at that size.
    fn thumbnail_ready(&mut self, made: Shrunk, cx: &mut Context<Self>) {
        if self.listing.as_ref().map(|listing| listing.id) == Some(made.id)
            && self.rows_wanted.contains(&made.index)
            && made.side == self.thumbnail_side()
            // Hidden while it was being shrunk.
            && self.settings.thumbnails == Thumbnails::Shown
        {
            let replaced = self.thumbnails.insert(made.index, Arc::new(made.image));
            release(replaced, None, cx);
            cx.notify();
        }
    }
}

/// Drops `images` from GPUI's image atlas. `window` is the window being
/// drawn or updated, if any: GPUI takes it out of its list meanwhile, so
/// it has to be named.
pub(super) fn release(
    images: impl IntoIterator<Item = Arc<RenderImage>>,
    mut window: Option<&mut Window>,
    cx: &mut App,
) {
    for image in images {
        cx.drop_image(image, window.as_deref_mut());
    }
}
