use nova_files_api::HiddenFiles;
use nova_files_settings_api::{self as settings_api, SettingChange};

use super::MainWindow;
use crate::location::Location;

impl MainWindow {
    pub(crate) fn toggle_hidden(&self) {
        let hidden = self.hidden_files.unwrap_or_default().toggled();
        if let Some(connection) = &self.connection {
            connection
                .send(settings_api::Command::Change(SettingChange::HiddenFiles(
                    hidden,
                )))
                .ok();
        }
    }

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
