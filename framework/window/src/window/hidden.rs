use nova_files_api::HiddenFiles;

use super::MainWindow;
use nova_files_shell::Location;

impl MainWindow {
    pub(super) fn hidden_arrived(&mut self, hidden: HiddenFiles) {
        let before = self.hidden_files.replace(hidden);
        if before.is_none_or(|before| before == hidden) || self.searching() {
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
