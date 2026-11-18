//! The commands about how the app looks and is set: settings, the theme,
//! the desktop's look and icon theme, the saved window state and the
//! folders' views. `run`
//! sends only these here.

use nova_files_api::{ClientGone, ClientId, Command, Event};

use super::Setup;
use crate::jobs::Client;

/// Answers one of the look and settings commands for client `id`.
pub(super) fn answer(
    id: ClientId,
    client: &Client,
    command: Command,
    setup: &Setup,
) -> Result<(), ClientGone> {
    match command {
        Command::WindowState => {
            return client.deliver(Event::WindowState(setup.window_state.bytes()));
        }
        Command::SaveWindowState(bytes) => setup.window_state.save(bytes),
        Command::FolderViews => setup.folder_views.listen(id, client.events()),
        Command::SetFolderView { folder, view } => setup.folder_views.set(folder, view),
        Command::Settings => setup.settings.listen(id, client.events()),
        Command::ChangeSetting(change) => setup.settings.change(change),
        Command::Theme => setup.theme.listen(id, client.events()),
        Command::DesktopLook(look) => setup.theme.follow_desktop(look),
        Command::DesktopIconThemes(names) => setup.file_icons.follow_desktop(&names),
        // `run` sends the other commands elsewhere.
        _ => {}
    }
    Ok(())
}
