use std::collections::HashMap;

use gpui::Context;
use nova_files_api::{EntryId, FolderSizes, SortKey};

use nova_files_shell::PageRef;

use super::MainWindow;

#[derive(Debug, Default)]
pub struct Shares {
    measured: HashMap<usize, u64>,
    largest: u64,
}

impl Shares {
    pub(super) fn forget(&mut self, folder_left: bool) {
        self.measured.clear();
        if folder_left {
            self.largest = 0;
        }
    }

    pub(super) fn seen(&mut self, bytes: u64) {
        self.largest = self.largest.max(bytes);
    }
}

impl MainWindow {
    pub(super) fn folders_measured(
        &mut self,
        sizes: &[(EntryId, u64)],
        done: bool,
        cx: &mut Context<Self>,
    ) {
        for &(id, bytes) in sizes {
            if let Ok(index) = usize::try_from(id.0) {
                self.shares.measured.insert(index, bytes);
                self.shares.seen(bytes);
            }
        }
        if done
            && !self.waiting()
            && self.shows_shares()
            && let Some(place) = self.place_on_screen_now()
        {
            self.request_place(&place);
        }
        cx.notify();
    }

    pub fn shows_shares(&self) -> bool {
        self.shown_order.key == SortKey::Size
            && self.folder_sizes == Some(FolderSizes::Measured)
            && !self.searching()
            && self
                .listed_page
                .as_ref()
                .map_or_else(|| self.folder().is_some(), PageRef::measures_folders)
    }

    pub(super) fn size_of_row(&self, index: usize) -> Option<u64> {
        self.shares
            .measured
            .get(&index)
            .copied()
            .or_else(|| self.details.get(&index).and_then(|read| read.details.size))
    }

    pub fn share(&self, index: usize) -> Option<f32> {
        let bytes = self.size_of_row(index)?;
        (self.shares.largest > 0).then(|| fraction(bytes, self.shares.largest))
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a bar a hundred pixels long needs no more"
)]
fn fraction(part: u64, whole: u64) -> f32 {
    part.min(whole) as f32 / whole as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_is_the_part_of_the_largest() {
        assert!((fraction(50, 200) - 0.25).abs() < f32::EPSILON);
        assert!((fraction(200, 200) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn leaving_a_folder_forgets_the_largest_but_listing_it_again_keeps_it() {
        let mut shares = Shares::default();
        shares.seen(90);
        shares.measured.insert(0, 90);
        shares.forget(false);
        assert!(shares.measured.is_empty());
        assert_eq!(shares.largest, 90);
        shares.forget(true);
        assert_eq!(shares.largest, 0);
    }
}
