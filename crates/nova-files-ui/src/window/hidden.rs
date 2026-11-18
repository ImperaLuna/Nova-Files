//! Hidden files: Ctrl+H lists the entries whose names start with a dot, or
//! leaves them out again. Which one is a setting the core keeps, so every
//! window follows it and it lasts between runs; the core leaves hidden
//! entries out of the listings it sends (see the core's `list`), except
//! in the trash. Ctrl+H asks the core to change the setting, and the
//! window reads its folder again once the new settings arrive, keeping the selection by name (see
//! `selecting`). A search's files are the index's, which this leaves
//! alone. Hidden rows show dimmed (see `view::sheet`).

use nova_files_api::{Command, HiddenFiles, SettingChange};

use super::MainWindow;
use crate::location::Location;

impl MainWindow {
    /// Asks the core to show hidden files if they are left out, or to
    /// leave them out if they show.
    pub(crate) fn toggle_hidden(&self) {
        let hidden = self.settings.hidden_files.toggled();
        if let Some(connection) = &self.connection {
            // A core that stopped can't change anything; the window says
            // so already (see `CoreStatus`).
            connection
                .send(Command::ChangeSetting(SettingChange::HiddenFiles(hidden)))
                .ok();
        }
    }

    /// Takes `hidden`, from settings the core sent: when hidden files
    /// were shown and now aren't, or the other way round, reads the place
    /// asked for last again. The first settings change nothing: the core
    /// already listed that folder under them.
    pub(super) fn hidden_arrived(&mut self, hidden: HiddenFiles) {
        let before = self.hidden_files.replace(hidden);
        if before.is_none_or(|before| before == hidden) || self.searching() {
            return;
        }
        if let Some(place) = self.place_asked_last() {
            self.request_place(&place);
        }
    }

    /// The place the window asked for last: the one still on its way, or
    /// else the folder or gathered place on screen.
    pub(super) fn place_asked_last(&self) -> Option<Location> {
        if self.waiting() {
            self.requested.clone()
        } else {
            self.place_on_screen_now()
        }
    }
}
