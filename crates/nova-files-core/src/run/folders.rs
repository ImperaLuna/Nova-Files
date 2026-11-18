//! The commands about folders and the files in them: listing, rows,
//! places, the trash, search, properties, previews, opening files and
//! the recent folders. `run` sends only these here. Folders list their
//! hidden entries as the settings say, but the trash lists all of them.

use std::path::Path;
use std::sync::Arc;

use nova_files_api::{ClientGone, ClientId, Command, HiddenFiles};

use super::Setup;
use crate::jobs::{self, Client};
use crate::preview::{self, Request};
use crate::properties;
use crate::trash::{FILES, Trashes};

/// Answers one of the folder commands for client `id`.
pub(super) fn answer(
    id: ClientId,
    client: &Client,
    command: Command,
    setup: &Setup,
) -> Result<(), ClientGone> {
    match command {
        Command::ListFolder { id, path, order } => {
            let hidden = hidden_files(setup, &path);
            jobs::list_folder(client, (id, path), order, hidden, &setup.inside);
        }
        Command::Places => setup.places.listen(id, client.events()),
        Command::ChangeBookmark(change) => setup.places.change(change),
        Command::ListTrash => return jobs::list_trash(client, setup.home.clone()),
        Command::Rows { id, first, count } => {
            client.want_rows(id, first, count, setup.settings.current().thumbnails);
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
        Command::OpenFile { path, activation } => {
            let opening = (Arc::clone(&setup.mime_types), Arc::clone(&setup.inside));
            jobs::open_file(client, opening, path, activation);
        }
        Command::OpenWith(request) => setup.open_with.answer(client.events(), request),
        Command::FolderUsed(folder) => setup.recent.used(folder),
        Command::RecentFolders { id, count, skip } => {
            let count = usize::try_from(count).unwrap_or(usize::MAX);
            setup.recent.answer(client.events(), id, count, skip);
        }
        // `run` sends the other commands elsewhere.
        _ => {}
    }
    Ok(())
}

/// Whether a listing of `path` shows its hidden entries: as the settings
/// say, except in the home trash, which lists all it holds so every item
/// in it can be seen and put back.
fn hidden_files(setup: &Setup, path: &Path) -> HiddenFiles {
    let trash = Trashes::of(setup.home.as_ref()).home;
    if trash.is_some_and(|trash| path.starts_with(trash.join(FILES))) {
        HiddenFiles::Shown
    } else {
        setup.settings.current().hidden_files
    }
}
