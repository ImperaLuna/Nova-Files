use std::path::Path;

use nova_files_api::{Command, HiddenFiles};
use nova_files_settings_api::Thumbnails;

use super::Setup;
use crate::backend::jobs::{self, Client};
use crate::backend::trash;
use nova_files_jobs::trash_info::FILES;

pub(super) fn answer(client: &Client, command: Command, setup: &Setup) {
    match command {
        Command::ListFolder { id, path, order } => {
            let hidden = hidden_files(setup, &path);
            jobs::list_folder(client, (id, path), order, hidden, &setup.inside);
        }
        Command::Rows {
            id,
            first,
            count,
            size,
        } => {
            let shown = setup.settings.current().thumbnails == Thumbnails::Shown;
            client.want_rows(id, first, count, shown.then_some(size));
        }
        _ => {}
    }
}

fn hidden_files(setup: &Setup, path: &Path) -> HiddenFiles {
    let trash = trash::trashes(setup.home.as_ref()).home;
    if trash.is_some_and(|trash| path.starts_with(trash.join(FILES))) {
        HiddenFiles::Shown
    } else {
        setup.settings.current().hidden_files
    }
}
