//! The thumbnails a window holds, by row.
//!
//! Drawing an image copies it into GPUI's image atlas, and dropping the
//! last `Arc` doesn't take it out again: only `drop_image` does. So every
//! way a thumbnail leaves this set (a row scrolled out of range, a new
//! folder, another scale, a newer thumbnail for the same row) hands the
//! image back, and the window drops it from the atlas. Without that, each
//! thumbnail ever shown stays in memory for the window's lifetime.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use gpui::RenderImage;

#[derive(Default)]
pub struct ShownThumbnails(HashMap<usize, Arc<RenderImage>>);

impl ShownThumbnails {
    /// The thumbnail of `row`, if there is one.
    pub fn get(&self, row: usize) -> Option<&Arc<RenderImage>> {
        self.0.get(&row)
    }

    /// Keeps `image` for `row`, handing back the one it replaces.
    #[must_use]
    pub fn insert(&mut self, row: usize, image: Arc<RenderImage>) -> Option<Arc<RenderImage>> {
        self.0.insert(row, image)
    }

    /// Forgets the thumbnails of rows outside `rows`, handing them back.
    #[must_use]
    pub fn keep_rows(&mut self, rows: &Range<usize>) -> Vec<Arc<RenderImage>> {
        self.0
            .extract_if(|row, _| !rows.contains(row))
            .map(|(_, image)| image)
            .collect()
    }

    /// Forgets every thumbnail, handing them back.
    #[must_use]
    pub fn take_all(&mut self) -> Vec<Arc<RenderImage>> {
        self.0.drain().map(|(_, image)| image).collect()
    }
}

#[cfg(test)]
mod tests {
    use image::{Frame, RgbaImage};

    use super::*;

    fn image() -> Arc<RenderImage> {
        Arc::new(RenderImage::new([Frame::new(RgbaImage::new(1, 1))]))
    }

    fn ids(images: &[Arc<RenderImage>]) -> Vec<gpui::ImageId> {
        let mut ids: Vec<_> = images.iter().map(|image| image.id).collect();
        ids.sort_unstable_by_key(|id| id.0);
        ids
    }

    #[test]
    fn rows_out_of_range_are_handed_back() {
        let mut shown = ShownThumbnails::default();
        let images: Vec<_> = (0..5).map(|_| image()).collect();
        for (row, image) in images.iter().enumerate() {
            assert!(shown.insert(row, Arc::clone(image)).is_none());
        }

        let gone = shown.keep_rows(&(1..3));

        assert_eq!(
            ids(&gone),
            ids(&[&images[0], &images[3], &images[4]].map(Arc::clone))
        );
        assert!(shown.get(0).is_none());
        assert_eq!(shown.get(1).map(|image| image.id), Some(images[1].id));
        assert_eq!(shown.get(2).map(|image| image.id), Some(images[2].id));
        assert_eq!(ids(&shown.keep_rows(&(1..3))), []);
    }

    #[test]
    fn a_replaced_thumbnail_is_handed_back() {
        let mut shown = ShownThumbnails::default();
        let first = image();
        assert!(shown.insert(4, Arc::clone(&first)).is_none());

        let replaced = shown.insert(4, image());

        assert_eq!(replaced.map(|image| image.id), Some(first.id));
    }

    #[test]
    fn taking_all_empties_the_set() {
        let mut shown = ShownThumbnails::default();
        let images: Vec<_> = (0..3).map(|_| image()).collect();
        for (row, image) in images.iter().enumerate() {
            assert!(shown.insert(row, Arc::clone(image)).is_none());
        }

        assert_eq!(ids(&shown.take_all()), ids(&images));
        assert!(shown.get(0).is_none());
        assert_eq!(ids(&shown.take_all()), []);
    }
}
