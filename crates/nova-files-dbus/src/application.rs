//! `org.freedesktop.Application`, the freedesktop interface a running app
//! offers so a second launch (or a launcher using D-Bus activation) can
//! hand over to it instead of starting a second process.
//!
//! `Activate` asks for a new window on the Start page, `Open` for one
//! window per `file://` URI, on that folder. `ActivateAction`'s one action,
//! [`SERVE_FILE_MANAGER`], has the running instance ask for apps' "Show in
//! folder" name, when D-Bus started a second process for it.

use std::collections::HashMap;
use std::path::PathBuf;

use nova_files_api::{Connector, WindowRequest, uri};
use zbus::fdo;
use zbus::zvariant::OwnedValue;

use crate::file_manager;

/// The interface name. The `#[zbus::interface]` attribute needs the
/// literal too; a test checks the two match.
pub const INTERFACE: &str = "org.freedesktop.Application";
/// The action that has the running instance ask for
/// `org.freedesktop.FileManager1`.
pub const SERVE_FILE_MANAGER: &str = "serve-file-manager";

pub struct Application<F> {
    connector: Connector,
    on_open: F,
}

impl<F> Application<F> {
    pub const fn new(connector: Connector, on_open: F) -> Self {
        Self { connector, on_open }
    }
}

/// The local paths `uris` name.
///
/// # Errors
/// When any isn't a local `file://` URI.
pub fn local_paths(uris: Vec<String>) -> fdo::Result<Vec<PathBuf>> {
    uris.into_iter()
        .map(|text| {
            uri::path_from_file_uri(&text)
                .ok_or_else(|| fdo::Error::InvalidArgs(format!("not a local file URI: {text}")))
        })
        .collect()
}

/// The core hears of a window first, so it doesn't stop while the window
/// is still starting; if it has already stopped, the caller gets an error
/// rather than a window that never appears.
pub fn keep_core_up(connector: &Connector) -> fdo::Result<()> {
    connector
        .activity()
        .map_err(|error| fdo::Error::Failed(error.to_string()))
}

#[zbus::interface(name = "org.freedesktop.Application")]
impl<F: Fn(WindowRequest) + Send + Sync + 'static> Application<F> {
    /// Opens a new window on the Start page. `platform_data` (startup
    /// notification ids and the like) is not used yet.
    fn activate(&self, platform_data: HashMap<String, OwnedValue>) -> fdo::Result<()> {
        drop(platform_data);
        keep_core_up(&self.connector)?;
        (self.on_open)(WindowRequest::default());
        Ok(())
    }

    /// Opens a window on each folder `uris` names. Nothing opens unless
    /// every URI is a local `file://` one; no URIs opens the home folder.
    fn open(
        &self,
        uris: Vec<String>,
        platform_data: HashMap<String, OwnedValue>,
    ) -> fdo::Result<()> {
        drop(platform_data);
        let folders = local_paths(uris)?;
        keep_core_up(&self.connector)?;
        if folders.is_empty() {
            (self.on_open)(WindowRequest::default());
        }
        for folder in folders {
            (self.on_open)(WindowRequest::on(folder));
        }
        Ok(())
    }

    /// Runs `name`: only [`SERVE_FILE_MANAGER`], which asks for
    /// `org.freedesktop.FileManager1`, never taking it from its owner.
    async fn activate_action(
        &self,
        #[zbus(connection)] connection: &zbus::Connection,
        name: String,
        parameter: Vec<OwnedValue>,
        platform_data: HashMap<String, OwnedValue>,
    ) -> fdo::Result<()> {
        drop((parameter, platform_data));
        if name != SERVE_FILE_MANAGER {
            return Err(fdo::Error::InvalidArgs(format!("no action {name}")));
        }
        file_manager::queue_for_name(connection)
            .await
            .map(drop)
            .map_err(|error| fdo::Error::Failed(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use zbus::object_server::Interface as _;

    use super::{Application, INTERFACE};

    #[test]
    fn interface_name_matches_the_constant() {
        assert_eq!(
            Application::<fn(nova_files_api::WindowRequest)>::name().as_str(),
            INTERFACE
        );
    }
}
