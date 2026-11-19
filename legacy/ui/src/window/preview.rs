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

// GPUI draws images in BGRA order.
// https://github.com/ImperaLuna/zed/tree/nova/v0.233.10/crates/gpui
const RED: usize = 0;
const BLUE: usize = 2;
const BYTES_PER_PIXEL: usize = 4;

#[derive(Default)]
pub struct QuickLooks {
    pub(crate) open: Option<QuickLook>,
    fit: Option<Fit>,
    pub(crate) docked: bool,
    next_id: u64,
}

impl QuickLook {
    pub(crate) fn current(&self) -> Option<&Shown> {
        self.shown
            .as_ref()
            .filter(|shown| shown.entry == self.entry)
    }
}

impl QuickLooks {
    const fn next_id(&mut self) -> PreviewId {
        self.next_id += 1;
        PreviewId(self.next_id)
    }
}

#[derive(Debug, Clone)]
pub struct Previewed {
    pub name: String,
    pub preview: Option<Arc<Preview>>,
    pub picture: bool,
}

pub struct QuickLook {
    pub listing: ListingId,
    pub entry: EntryId,
    asked: PreviewId,
    pub shown: Option<Shown>,
}

pub struct Shown {
    pub entry: EntryId,
    pub preview: Option<Arc<Preview>>,
    pub picture: Option<Arc<RenderImage>>,
}

impl MainWindow {
    pub(crate) fn toggle_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick_looks.open.is_some() {
            if self.quick_looks.docked {
                self.quick_looks.docked = false;
                cx.notify();
            } else {
                self.dismiss_preview(window, cx);
            }
            return;
        }
        if self.picker() {
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

    pub(crate) fn dismiss_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.showing_gallery() {
            self.quick_looks.docked = true;
            cx.notify();
        } else {
            self.close_preview(Some(window), cx);
        }
    }

    pub(crate) fn close_preview(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        self.quick_looks.docked = false;
        if let Some(open) = self.quick_looks.open.take() {
            release(open.shown.and_then(|shown| shown.picture), window, cx);
            cx.notify();
        }
    }

    pub(crate) fn step_sideways(&mut self, step: Move, cx: &mut Context<Self>) {
        if !self.step_preview(step, cx) && self.showing_icons() {
            self.move_cursor(step, false, cx);
        }
    }

    pub(crate) fn step_preview(&mut self, step: Move, cx: &mut Context<Self>) -> bool {
        let Some(open) = &self.quick_looks.open else {
            return false;
        };
        let shown = usize::try_from(open.entry.0).ok();
        self.move_cursor_from(shown, step, cx);
        true
    }

    pub(crate) fn preview_follow_cursor(&mut self, cx: &mut Context<Self>) {
        let Some(open) = &self.quick_looks.open else {
            return;
        };
        let listing = open.listing;
        let Some(entry) = self
            .cursor()
            .and_then(|cursor| u32::try_from(cursor).ok())
            .map(EntryId)
            .filter(|&entry| entry != open.entry)
        else {
            return;
        };
        let shown = self.quick_looks.open.take().and_then(|open| open.shown);
        self.preview_entry(listing, entry, shown, cx);
    }

    pub(crate) fn open_previewed(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.previewing() {
            return false;
        }
        let Some(entry) = self.quick_looks.open.as_ref().map(|open| open.entry) else {
            return false;
        };
        self.close_preview(Some(window), cx);
        self.open_entries(&[entry], cx);
        true
    }

    pub const fn previewing(&self) -> bool {
        self.quick_looks.open.is_some() && !self.quick_looks.docked
    }

    pub fn previewed(&self) -> Option<Previewed> {
        let open = self.quick_looks.open.as_ref()?;
        let listing = self.listing.as_ref()?;
        let entry = listing.entries.get(usize::try_from(open.entry.0).ok()?)?;
        let current = open.current();
        Some(Previewed {
            name: entry.name.to_string_lossy().into_owned(),
            preview: current.and_then(|shown| shown.preview.clone()),
            picture: current.is_some_and(|shown| shown.picture.is_some()),
        })
    }

    pub const fn preview_fit(&self) -> Option<Fit> {
        self.quick_looks.fit
    }

    pub(crate) const fn quick_look(&self) -> Option<&QuickLook> {
        self.quick_looks.open.as_ref()
    }

    pub(crate) fn set_preview_room(
        &mut self,
        room: Size<Pixels>,
        scale_factor: f32,
        cx: &mut Context<Self>,
    ) {
        let device = |side: Pixels| crate::thumbnail::device_side(side, scale_factor);
        let fit = Some(Fit {
            width: device(room.width),
            height: device(room.height),
        });
        if self.quick_looks.fit == fit {
            return;
        }
        self.quick_looks.fit = fit;
        if let Some(open) = self.quick_looks.open.take() {
            self.preview_entry(open.listing, open.entry, open.shown, cx);
        }
    }

    pub(super) fn preview_entry(
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
        let asked = self.quick_looks.next_id();
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

    fn entry_path(&self, listing: ListingId, entry: EntryId) -> Option<PathBuf> {
        let on_screen = self.listing.as_ref().filter(|shown| shown.id == listing)?;
        let index = usize::try_from(entry.0).ok()?;
        let entry = on_screen.entries.get(index)?;
        Some(on_screen.path_of(entry))
    }

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
                let shown = open.shown.map(|shown| moved(shown, open.entry, entry));
                self.preview_entry(listing, entry, shown, cx);
            }
            (_, open) => {
                self.quick_looks.open = open;
                self.close_preview(None, cx);
            }
        }
    }
}

fn moved(mut shown: Shown, from: EntryId, to: EntryId) -> Shown {
    if shown.entry == from {
        shown.entry = to;
    }
    shown
}

const DEFAULT_FIT: Fit = Fit {
    width: 1920,
    height: 1080,
};

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
    fn what_is_shown_moves_with_its_entry() {
        let shown = |entry| Shown {
            entry: EntryId(entry),
            preview: None,
            picture: None,
        };
        let moved_entry = |entry, from, to| moved(shown(entry), EntryId(from), EntryId(to)).entry;
        assert_eq!(moved_entry(2, 2, 5), EntryId(5));
        assert_eq!(moved_entry(1, 2, 5), EntryId(1));
    }

    #[test]
    fn every_request_gets_a_new_id() {
        let mut looks = QuickLooks::default();
        let first = looks.next_id();
        let second = looks.next_id();
        assert_ne!(first, second);
        assert!(second > first);
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
