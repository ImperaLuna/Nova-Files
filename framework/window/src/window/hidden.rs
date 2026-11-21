use nova_files_api::{FolderSizes, HiddenFiles, SortKey};

use super::MainWindow;
use nova_files_shell::Location;

impl MainWindow {
    pub(super) fn hidden_arrived(&mut self, hidden: HiddenFiles) {
        let before = self.hidden_files.replace(hidden);
        if before.is_some_and(|before| before != hidden) {
            self.list_again();
        }
    }

    pub(super) fn folder_sizes_arrived(&mut self, sizes: FolderSizes) {
        let before = self.folder_sizes.replace(sizes);
        let by_size = self.order.key == SortKey::Size;
        if by_size && before.is_some_and(|before| before != sizes) {
            self.list_again();
        }
    }

    fn list_again(&mut self) {
        if self.searching() {
            return;
        }
        if let Some(place) = self.place_asked_last() {
            self.request_place(&place);
        }
    }

    pub(super) fn place_asked_last(&self) -> Option<Location> {
        if self.waiting() {
            self.requested.clone()
        } else {
            self.place_on_screen_now()
        }
    }
}
