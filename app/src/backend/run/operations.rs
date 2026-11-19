use std::sync::Arc;

use nova_files_api::{ClientGone, ClientId, Command};

use super::Setup;
use crate::backend::jobs::Client;
use nova_files_transfer_api as transfer_api;
use nova_files_transfer_core::{answer_drop_would, start_drop};

pub(super) fn answer(
    id: ClientId,
    client: &Client,
    command: Command,
    setup: &Setup,
) -> Result<(), ClientGone> {
    setup.jobs.command(id, client.events(), command)
}

pub(super) fn drop(id: ClientId, client: &Client, command: transfer_api::Command, setup: &Setup) {
    match command {
        transfer_api::Command::Drop { tag, sources, into } => {
            setup.jobs.audience().join(id, Arc::clone(client.events()));
            start_drop(&setup.jobs, client.events(), tag, sources, into);
        }
        transfer_api::Command::DropWould { id, sources, into } => {
            answer_drop_would(client.events(), id, sources, into);
        }
    }
}
