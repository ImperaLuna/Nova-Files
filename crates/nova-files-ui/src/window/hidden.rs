//! Hidden files: Ctrl+H lists the entries whose names start with a dot, or
//! leaves them out again. Which one is a setting the core keeps, so every
//! window follows it and it lasts between runs; the core leaves hidden
//! entries out of the listings it sends (see the core's `list`). Ctrl+H
//! asks the core to change the setting, and the window reads its folder
//! again once the new settings arrive, keeping the selection by name (see
//! `selecting`). A search's files are the index's, which this leaves
//! alone. Hidden rows show dimmed (see `view::sheet`).

use std::path::{Path, PathBuf};

use nova_files_api::{Command, SettingChange};

use super::MainWindow;

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

    /// Reads the folder asked for last again once hidden files are shown
    /// or left out (see `events`).
    pub(super) fn hidden_changed(&mut self) {
        if self.searching() {
            return;
        }
        if let Some(folder) = self.folder_asked_last() {
            self.request(&folder);
        }
    }

    /// The folder the window asked for last: the one still on its way, or
    /// else the one on screen.
    pub(super) fn folder_asked_last(&self) -> Option<PathBuf> {
        if self.waiting() {
            self.requested.clone()
        } else {
            self.folder().map(Path::to_path_buf)
        }
    }
}
