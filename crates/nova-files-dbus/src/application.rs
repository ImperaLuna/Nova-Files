//! `org.freedesktop.Application`, the freedesktop interface a running app
//! offers so a second launch (or a launcher using D-Bus activation) can
//! hand over to it instead of starting a second process.
//!
//! `Activate` asks for a new window on the home folder, `Open` for one
//! window per `file://` URI, on that folder. `ActivateAction` comes with the
//! feature that needs it.

use std::collections::HashMap;
use std::path::PathBuf;

use nova_files_api::{Connector, uri};
use zbus::fdo;
use zbus::zvariant::OwnedValue;

/// The interface name. The `#[zbus::interface]` attribute needs the
/// literal too; a test checks the two match.
pub const INTERFACE: &str = "org.freedesktop.Application";

pub struct Application<F> {
    connector: Connector,
    on_open: F,
}

impl<F> Application<F> {
    pub const fn new(connector: Connector, on_open: F) -> Self {
        Self { connector, on_open }
    }
}

impl<F: Fn(Option<PathBuf>)> Application<F> {
    /// The core hears of a window first, so it doesn't stop while the
    /// window is still starting; if it has already stopped, the caller gets
    /// an error rather than a window that never appears.
    fn keep_core_up(&self) -> fdo::Result<()> {
        self.connector
            .activity()
            .map_err(|error| fdo::Error::Failed(error.to_string()))
    }
}

#[zbus::interface(name = "org.freedesktop.Application")]
impl<F: Fn(Option<PathBuf>) + Send + Sync + 'static> Application<F> {
    /// Opens a new window on the home folder. `platform_data` (startup
    /// notification ids and the like) is not used yet.
    fn activate(&self, platform_data: HashMap<String, OwnedValue>) -> fdo::Result<()> {
        drop(platform_data);
        self.keep_core_up()?;
        (self.on_open)(None);
        Ok(())
    }

    /// Opens a window on each folder `uris` names. Nothing opens unless
    /// every URI is a local `file://` one; no URIs opens the home folder.
    fn open(
        &self,
        uris: Vec<String>,
        hint: String,
        platform_data: HashMap<String, OwnedValue>,
    ) -> fdo::Result<()> {
        drop((hint, platform_data));
        let folders = uris
            .into_iter()
            .map(|text| {
                uri::path_from_file_uri(&text)
                    .ok_or_else(|| fdo::Error::InvalidArgs(format!("not a local file URI: {text}")))
            })
            .collect::<fdo::Result<Vec<PathBuf>>>()?;
        self.keep_core_up()?;
        if folders.is_empty() {
            (self.on_open)(None);
        }
        for folder in folders {
            (self.on_open)(Some(folder));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use zbus::object_server::Interface as _;

    use super::{Application, INTERFACE};

    #[test]
    fn interface_name_matches_the_constant() {
        assert_eq!(
            Application::<fn(Option<std::path::PathBuf>)>::name().as_str(),
            INTERFACE
        );
    }
}
