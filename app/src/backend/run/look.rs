use nova_files_api::{ClientGone, Command, Event};

use super::Setup;
use crate::backend::jobs::Client;

pub(super) fn answer(client: &Client, command: Command, setup: &Setup) -> Result<(), ClientGone> {
    match command {
        Command::WindowState => {
            return client.deliver(Event::WindowState(setup.window_state.bytes()));
        }
        Command::SaveWindowState(bytes) => setup.window_state.save(&bytes),
        _ => {}
    }
    Ok(())
}
