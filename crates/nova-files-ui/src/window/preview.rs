//! The quick look: the selected file or folder, large, over the window.
//!
//! Space opens it on the entry the keyboard is on, or else the first one
//! selected, and closes it again; Escape closes it too. While it is open,
//! the arrow keys move the cursor as they do in the list, the selection
//! follows, and the quick look shows the entry the cursor lands on, as it
//! does whatever else moves the cursor. Enter
//! opens that entry as a double-click would, and closes the quick look.
//! Going to another folder closes it.
//!
//! The window never touches the disk: it asks the core for a
//! [`Command::Preview`] of the entry and draws the answer (see
//! `view::preview`). An answer for any request but the newest is dropped.
//! Until the answer comes, the quick look keeps showing what it showed
//! before, so stepping through a folder never flashes empty. A picture
//! comes decoded at the size it is drawn; it is turned into GPUI's pixel
//! order off the UI thread, and the one before is dropped from GPUI's
//! image atlas once replaced, as is the last one when the quick look
//! closes.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{Context, Pixels, RenderImage, Size, Window};
use image::{Frame, RgbaImage};
use nova_files_api::{
    Command, EntryId, Fit, Listing, ListingId, Picture, Preview, PreviewContent, PreviewId,
};

use super::MainWindow;
use super::rows::release;
use crate::selection::{Move, entry_at};

/// GPUI draws images in BGRA order; the core sends RGBA.
const RED: usize = 0;
const BLUE: usize = 2;
const BYTES_PER_PIXEL: usize = 4;

/// The quick look's part of the window.
#[derive(Default)]
pub struct QuickLooks {
    /// The quick look on screen, if any.
    pub(crate) open: Option<QuickLook>,
    /// The box a picture is decoded to fit: the room the quick look has,
    /// in device pixels, as of the last frame.
    fit: Option<Fit>,
    next_id: u64,
}

/// What the quick look shows, as [`MainWindow::previewed`] tells it.
#[derive(Debug, Clone)]
pub struct Previewed {
    /// The name of the entry it is on.
    pub name: String,
    /// What the core sent for that entry, once it came.
    pub preview: Option<Arc<Preview>>,
    /// Whether that entry's picture is ready to draw.
    pub picture: bool,
}

/// The quick look on screen.
pub struct QuickLook {
    /// The entry shown, in the listing on screen.
    pub listing: ListingId,
    pub entry: EntryId,
    /// The request for that entry; only its answer counts.
    asked: PreviewId,
    /// What the core last sent, for this entry or the one before.
    pub shown: Option<Shown>,
}

/// What the quick look draws large.
pub struct Shown {
    pub entry: EntryId,
    /// `None` when the core couldn't look into it.
    pub preview: Option<Arc<Preview>>,
    /// The preview's picture, ready to draw.
    pub picture: Option<Arc<RenderImage>>,
}

impl MainWindow {
    /// Space: opens the quick look on the cursor's entry, or the first one
    /// selected, or closes it. In a picker it does nothing.
    pub(crate) fn toggle_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick_looks.open.is_some() {
            self.close_preview(Some(window), cx);
            return;
        }
        if self.pick.is_some() {
            return;
        }
        let Some((listing, ids)) = self.selection() else {
            return;
        };
        if self.listing_id() != Some(listing) {
            return;
        }
        let cursor = self
            .cursor()
            .and_then(|cursor| u32::try_from(cursor).ok())
            .map(EntryId)
            .filter(|cursor| ids.contains(cursor));
        let Some(entry) = cursor.or_else(|| ids.first().copied()) else {
            return;
        };
        self.preview_entry(listing, entry, None, cx);
    }

    /// Closes the quick look, letting go of its picture. `window` is the
    /// window being updated, if any (see `rows::release`).
    pub(crate) fn close_preview(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if let Some(open) = self.quick_looks.open.take() {
            release(open.shown.and_then(|shown| shown.picture), window, cx);
            cx.notify();
        }
    }

    /// An arrow key while the quick look is open: the cursor moves as in
    /// the list, the selection follows, and the quick look with it (see
    /// [`Self::preview_follow_cursor`]). Whether the quick look was open to
    /// take it.
    pub(crate) fn step_preview(&mut self, step: Move, cx: &mut Context<Self>) -> bool {
        if self.quick_looks.open.is_none() {
            return false;
        }
        self.move_cursor(step, false, cx);
        true
    }

    /// The selection changed: an open quick look moves to the entry the
    /// cursor is on now, whatever moved it, keeping what it shows until
    /// the core answers.
    pub(crate) fn preview_follow_cursor(&mut self, cx: &mut Context<Self>) {
        let Some(open) = &self.quick_looks.open else {
            return;
        };
        let listing = open.listing;
        let Some(entry) = self
            .cursor()
            .and_then(|cursor| u32::try_from(cursor).ok())
            .map(EntryId)
            .filter(|&entry| entry != open.entry && self.listing_id() == Some(listing))
        else {
            return;
        };
        let shown = self.quick_looks.open.take().and_then(|open| open.shown);
        self.preview_entry(listing, entry, shown, cx);
    }

    /// Enter while the quick look is open: opens its entry as a
    /// double-click would and closes the quick look. Whether the quick
    /// look was open to take it.
    pub(crate) fn open_previewed(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(entry) = self.quick_looks.open.as_ref().map(|open| open.entry) else {
            return false;
        };
        self.close_preview(Some(window), cx);
        self.open_entries(&[entry]);
        true
    }

    /// Whether the quick look is open.
    pub const fn previewing(&self) -> bool {
        self.quick_looks.open.is_some()
    }

    /// What the quick look shows, while it is open.
    pub fn previewed(&self) -> Option<Previewed> {
        let open = self.quick_looks.open.as_ref()?;
        let listing = self.listing.as_ref()?;
        let entry = listing.entries.get(usize::try_from(open.entry.0).ok()?)?;
        let current = open
            .shown
            .as_ref()
            .filter(|shown| shown.entry == open.entry);
        Some(Previewed {
            name: entry.name.to_string_lossy().into_owned(),
            preview: current.and_then(|shown| shown.preview.clone()),
            picture: current.is_some_and(|shown| shown.picture.is_some()),
        })
    }

    /// The quick look on screen.
    pub(crate) const fn quick_look(&self) -> Option<&QuickLook> {
        self.quick_looks.open.as_ref()
    }

    /// The room the quick look has this frame, `scale_factor` device
    /// pixels to a pixel.
    pub(crate) fn set_preview_room(&mut self, room: Size<Pixels>, scale_factor: f32) {
        let device = |side: Pixels| crate::thumbnail::device_side(side, scale_factor);
        self.quick_looks.fit = Some(Fit {
            width: device(room.width),
            height: device(room.height),
        });
    }

    /// Shows entry `entry` of listing `listing`, keeping `shown` on screen
    /// until the core answers.
    fn preview_entry(
        &mut self,
        listing: ListingId,
        entry: EntryId,
        shown: Option<Shown>,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.entry_path(listing, entry) else {
            release(shown.and_then(|shown| shown.picture), None, cx);
            cx.notify();
            return;
        };
        self.quick_looks.next_id += 1;
        let asked = PreviewId(self.quick_looks.next_id);
        self.quick_looks.open = Some(QuickLook {
            listing,
            entry,
            asked,
            shown,
        });
        if let Some(connection) = &self.connection {
            let fit = self.quick_looks.fit.unwrap_or(DEFAULT_FIT);
            connection
                .send(Command::Preview {
                    id: asked,
                    path,
                    fit,
                })
                .ok();
        }
        cx.notify();
    }

    /// The path of entry `entry` of listing `listing`, while it is the
    /// listing on screen.
    fn entry_path(&self, listing: ListingId, entry: EntryId) -> Option<PathBuf> {
        let on_screen = self.listing.as_ref().filter(|shown| shown.id == listing)?;
        let index = usize::try_from(entry.0).ok()?;
        let entry = on_screen.entries.get(index)?;
        Some(on_screen.path.join(&entry.name))
    }

    /// The core answered request `id`: `preview`, or `None` when it
    /// couldn't look. A picture is made ready to draw first, off the UI
    /// thread.
    pub(super) fn preview_arrived(
        &mut self,
        id: PreviewId,
        preview: Option<Arc<Preview>>,
        cx: &mut Context<Self>,
    ) {
        if self.quick_looks.open.as_ref().map(|open| open.asked) != Some(id) {
            return;
        }
        let Some(PreviewContent::Picture(picture)) =
            preview.as_deref().map(|preview| &preview.content)
        else {
            self.show_preview(id, preview, None, cx);
            return;
        };
        let picture = picture.clone();
        // Kept without its pixels, which the image drawn holds instead.
        let preview = preview.map(|preview| Arc::new(without_pixels(&preview)));
        let converting = cx
            .background_executor()
            .spawn(async move { render_image(&picture) });
        cx.spawn(async move |this, cx| {
            let image = converting.await;
            this.update(cx, |this, cx| {
                this.show_preview(id, preview, image.map(Arc::new), cx);
            })
            .ok();
        })
        .detach();
    }

    /// Puts request `id`'s answer on screen, if it is still the newest,
    /// and lets go of the picture it replaces.
    fn show_preview(
        &mut self,
        id: PreviewId,
        preview: Option<Arc<Preview>>,
        picture: Option<Arc<RenderImage>>,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self
            .quick_looks
            .open
            .as_mut()
            .filter(|open| open.asked == id)
        else {
            release(picture, None, cx);
            return;
        };
        let shown = Shown {
            entry: open.entry,
            preview,
            picture,
        };
        let before = open.shown.replace(shown);
        release(before.and_then(|before| before.picture), None, cx);
        cx.notify();
    }

    /// A listing arrived, in place of `before`: a quick look of another
    /// folder closes, and in a fresh read of the same folder it stays on
    /// its entry by name, or closes when that is gone. It asks for that
    /// entry again, since it may have changed, and shows what it showed
    /// until the answer comes.
    pub(super) fn preview_listing_arrived(
        &mut self,
        before: Option<&Listing>,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = &self.quick_looks.open else {
            return;
        };
        let name = before
            .filter(|before| before.id == open.listing)
            .and_then(|before| entry_at(&before.entries, open.entry))
            .map(|entry| entry.name.clone());
        let found = self.listing.as_ref().and_then(|listing| {
            let name = name.as_deref()?;
            let same_folder = before.is_some_and(|before| before.path == listing.path);
            let index = listing
                .entries
                .iter()
                .position(|entry| same_folder && entry.name == name)?;
            Some((listing.id, EntryId(u32::try_from(index).ok()?)))
        });
        match (found, self.quick_looks.open.take()) {
            (Some((listing, entry)), Some(open)) => {
                let shown = open.shown.map(|mut shown| {
                    if shown.entry == open.entry {
                        shown.entry = entry;
                    }
                    shown
                });
                self.preview_entry(listing, entry, shown, cx);
            }
            (_, open) => {
                self.quick_looks.open = open;
                self.close_preview(None, cx);
            }
        }
    }
}

/// The box a picture fits before the window has drawn once.
const DEFAULT_FIT: Fit = Fit {
    width: 1920,
    height: 1080,
};

/// `preview` with its picture's pixels left out, its size kept.
fn without_pixels(preview: &Preview) -> Preview {
    let content = match &preview.content {
        PreviewContent::Picture(picture) => PreviewContent::Picture(Picture {
            pixels: Arc::from([]),
            ..picture.clone()
        }),
        other => other.clone(),
    };
    Preview {
        content,
        ..preview.clone()
    }
}

/// `picture` in GPUI's pixel order; `None` when its pixels don't match
/// its size.
fn render_image(picture: &Picture) -> Option<RenderImage> {
    let mut pixels = picture.pixels.to_vec();
    for pixel in pixels.as_chunks_mut::<BYTES_PER_PIXEL>().0 {
        pixel.swap(RED, BLUE);
    }
    let image = RgbaImage::from_raw(picture.width, picture.height, pixels)?;
    Some(RenderImage::new([Frame::new(image)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_turns_into_bgra_of_its_size() {
        let picture = Picture {
            width: 2,
            height: 1,
            pixels: Arc::from([1, 2, 3, 4, 5, 6, 7, 8].as_slice()),
        };
        let image = render_image(&picture).unwrap();
        let size = image.size(0);
        assert_eq!((size.width.0, size.height.0), (2, 1));
        assert_eq!(image.as_bytes(0).unwrap(), [3, 2, 1, 4, 7, 6, 5, 8]);
    }

    #[test]
    fn pixels_that_dont_match_the_size_make_no_image() {
        let picture = Picture {
            width: 2,
            height: 2,
            pixels: Arc::from([0; 8].as_slice()),
        };
        assert!(render_image(&picture).is_none());
    }

    #[test]
    fn a_kept_preview_drops_its_pixels_and_keeps_its_size() {
        let preview = Preview {
            path: PathBuf::from("/a.png"),
            kind_name: Some("PNG image".to_owned()),
            bytes: 8,
            content: PreviewContent::Picture(Picture {
                width: 2,
                height: 1,
                pixels: Arc::from([0; 8].as_slice()),
            }),
        };
        let kept = without_pixels(&preview);
        let PreviewContent::Picture(picture) = &kept.content else {
            unreachable!("a picture stays a picture");
        };
        assert_eq!((picture.width, picture.height), (2, 1));
        assert!(picture.pixels.is_empty());
        assert_eq!(kept.kind_name, preview.kind_name);
    }
}
