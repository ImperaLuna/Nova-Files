//! The commands about folders and the files in them: listing, rows,
//! places, the trash, search, properties, previews and opening files.
//! `run` sends only these here.

use std::sync::Arc;

use nova_files_api::{ClientGone, ClientId, Command};

use super::Setup;
use crate::jobs::{self, Client};
use crate::preview::{self, Request};
use crate::properties;

/// Answers one of the folder commands for client `id`.
pub(super) fn answer(
    id: ClientId,
    client: &Client,
    command: Command,
    setup: &Setup,
) -> Result<(), ClientGone> {
    match command {
        Command::ListFolder { id, path, order } => jobs::list_folder(client, id, path, order),
        Command::Places => setup.places.listen(id, client.events()),
        Command::ChangeBookmark(change) => setup.places.change(change),
        Command::ListTrash => return jobs::list_trash(client, setup.home.clone()),
        Command::Rows { id, first, count } => {
            client.want_rows(id, first, count, setup.settings.current().thumbnails);
        }
        Command::Search { id, query, limit } => setup.searcher.search(client, id, query, limit),
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
            jobs::open_file(client, Arc::clone(&setup.mime_types), path, activation);
        }
        Command::OpenWith(request) => setup.open_with.answer(client.events(), request),
        // `run` sends the other commands elsewhere.
        _ => {}
    }
    Ok(())
}
