use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use gpui::RenderImage;
use nova_files_api::IconKind;

pub struct PeekItem {
    pub kind: IconKind,
    pub hidden: bool,
    pub image: Option<Arc<RenderImage>>,
}

#[derive(Default)]
pub struct ShownPeeks(HashMap<usize, Vec<PeekItem>>);

impl ShownPeeks {
    pub fn get(&self, row: usize) -> Option<&[PeekItem]> {
        self.0.get(&row).map(Vec::as_slice)
    }

    #[must_use]
    pub fn insert(&mut self, row: usize, items: Vec<PeekItem>) -> Vec<Arc<RenderImage>> {
        self.0.insert(row, items).map(images).unwrap_or_default()
    }

    #[must_use]
    pub fn keep_rows(&mut self, rows: &Range<usize>) -> Vec<Arc<RenderImage>> {
        self.0
            .extract_if(|row, _| !rows.contains(row))
            .flat_map(|(_, items)| images(items))
            .collect()
    }

    #[must_use]
    pub fn take_all(&mut self) -> Vec<Arc<RenderImage>> {
        self.0
            .drain()
            .flat_map(|(_, items)| images(items))
            .collect()
    }
}

fn images(items: Vec<PeekItem>) -> Vec<Arc<RenderImage>> {
    items.into_iter().filter_map(|item| item.image).collect()
}

#[cfg(test)]
mod tests {
    use image::{Frame, RgbaImage};

    use super::*;

    fn item(image: bool) -> PeekItem {
        PeekItem {
            kind: IconKind::Picture,
            hidden: false,
            image: image.then(|| Arc::new(RenderImage::new([Frame::new(RgbaImage::new(1, 1))]))),
        }
    }

    #[test]
    fn peeks_off_screen_hand_back_their_pictures() {
        let mut shown = ShownPeeks::default();
        assert_eq!(shown.insert(0, vec![item(true), item(false)]).len(), 0);
        assert_eq!(shown.insert(1, vec![item(true)]).len(), 0);
        assert_eq!(shown.insert(1, vec![item(true), item(true)]).len(), 1);

        assert_eq!(shown.keep_rows(&(1..2)).len(), 1);
        assert!(shown.get(0).is_none());
        assert_eq!(shown.get(1).map(<[PeekItem]>::len), Some(2));
        assert_eq!(shown.take_all().len(), 2);
        assert!(shown.get(1).is_none());
    }
}
