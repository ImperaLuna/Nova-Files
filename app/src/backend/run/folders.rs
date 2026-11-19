use std::path::Path;
use std::sync::Arc;

use nova_files_api::{Command, HiddenFiles};
use nova_files_archives_core::inside::Inside;
use nova_files_entries_core::{InnerFolders, list_folder};
use nova_files_settings_api::Thumbnails;

use super::Setup;
use crate::backend::jobs::Client;
use nova_files_jobs::trash_info::FILES;
use nova_files_trash_core::Trashes;

pub(super) fn answer(client: &Client, command: Command, setup: &Setup) {
    match command {
        Command::ListFolder { id, path, order } => {
            let hidden = hidden_files(setup, &path);
            let inner: Arc<dyn InnerFolders> = Arc::<Inside>::clone(&setup.inside);
            list_folder(client.entries(), (id, path), (order, hidden), &inner);
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
    let trash = Trashes::of_home(setup.home.as_ref()).home;
    if trash.is_some_and(|trash| path.starts_with(trash.join(FILES))) {
        HiddenFiles::Shown
    } else {
        setup.settings.current().hidden_files
    }
}
