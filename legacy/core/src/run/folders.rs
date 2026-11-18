use std::path::Path;
use std::sync::Arc;

use nova_files_api::{ClientId, Command, HiddenFiles, Thumbnails};

use super::Setup;
use crate::gathered;
use crate::jobs::{self, Client};
use crate::preview::{self, Request};
use crate::properties;
use crate::trash;
use nova_files_jobs::trash_info::FILES;

pub(super) fn answer(id: ClientId, client: &Client, command: Command, setup: &Setup) {
    match command {
        Command::ListFolder { id, path, order } => {
            let hidden = hidden_files(setup, &path);
            jobs::list_folder(client, (id, path), order, hidden, &setup.inside);
        }
        Command::Places => setup.places.listen(id, client.events()),
        Command::ChangeBookmark(change) => setup.places.change(change),
        Command::Gathered => setup.gathered.listen(id, client.events()),
        Command::ChangeGathered(change) => setup.gathered.change(change),
        Command::ListGathered { id, place, order } => {
            let hidden = setup.settings.current().hidden_files;
            gathered::list(client, id, setup.gathered.folders_of(place), order, hidden);
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
        Command::Search { id, query, limit } => setup.searcher.search(client, id, query, limit),
        Command::Find { id, query, within } => setup.searcher.find(client, id, query, within),
        Command::Properties { id, path } => properties::answer(client.events(), id, path),
        Command::Preview { id, path, fit } => {
            let request = Request {
                id,
                path,
                fit,
                current: Arc::clone(client.current_preview()),
                types: Arc::clone(&setup.mime_types),
                helper: setup.thumbnails.as_ref().map(|setup| setup.helper.clone()),
            };
            preview::answer(client.events(), request);
        }
        Command::FolderUsed(folder) => setup.recent.used(folder),
        Command::RecentFolders { id, count, skip } => {
            let count = usize::try_from(count).unwrap_or(usize::MAX);
            setup.recent.answer(client.events(), id, count, skip);
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
