use std::sync::Arc;

use gpui::RenderImage;
use image::{Frame, RgbaImage};
use nova_files_api::{EntryId, Fit, ListingId, Picture};
use nova_files_preview_api::{Preview, PreviewContent, PreviewId};

// GPUI draws images in BGRA order.
// https://github.com/ImperaLuna/zed/tree/47424cdc3f1a49829960e1ecbcdcdd967d034265/crates/gpui
const RED: usize = 0;
const BLUE: usize = 2;
const BYTES_PER_PIXEL: usize = 4;

pub const DEFAULT_FIT: Fit = Fit {
    width: 1920,
    height: 1080,
};

#[derive(Default)]
pub struct QuickLooks {
    pub open: Option<QuickLook>,
    pub fit: Option<Fit>,
    pub docked: bool,
    next_id: u64,
}

impl QuickLooks {
    pub const fn next_id(&mut self) -> PreviewId {
        self.next_id += 1;
        PreviewId(self.next_id)
    }
}

pub struct QuickLook {
    pub listing: ListingId,
    pub entry: EntryId,
    pub asked: PreviewId,
    pub shown: Option<Shown>,
}

impl QuickLook {
    pub fn current(&self) -> Option<&Shown> {
        self.shown
            .as_ref()
            .filter(|shown| shown.entry == self.entry)
    }
}

pub struct Shown {
    pub entry: EntryId,
    pub preview: Option<Arc<Preview>>,
    pub picture: Option<Arc<RenderImage>>,
}

#[derive(Debug, Clone)]
pub struct Previewed {
    pub name: String,
    pub preview: Option<Arc<Preview>>,
    pub picture: bool,
}

pub fn moved(mut shown: Shown, from: EntryId, to: EntryId) -> Shown {
    if shown.entry == from {
        shown.entry = to;
    }
    shown
}

pub fn without_pixels(preview: &Preview) -> Preview {
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

pub fn render_image(picture: &Picture) -> Option<RenderImage> {
    let mut pixels = picture.pixels.to_vec();
    for pixel in pixels.as_chunks_mut::<BYTES_PER_PIXEL>().0 {
        pixel.swap(RED, BLUE);
    }
    let image = RgbaImage::from_raw(picture.width, picture.height, pixels)?;
    Some(RenderImage::new([Frame::new(image)]))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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
