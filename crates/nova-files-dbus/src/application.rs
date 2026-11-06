//! `org.freedesktop.Application`, the freedesktop interface a running app
//! offers so a second launch (or a launcher using D-Bus activation) can
//! hand over to it instead of starting a second process.
//!
//! Only `Activate` is served for now: it asks for a new window. `Open` and
//! `ActivateAction` come with the features that need them.

use std::collections::HashMap;

use nova_files_api::Connector;
use zbus::fdo;
use zbus::zvariant::OwnedValue;

/// The interface name. The `#[zbus::interface]` attribute needs the
/// literal too; a test checks the two match.
pub const INTERFACE: &str = "org.freedesktop.Application";

pub struct Application<F> {
    connector: Connector,
    on_activate: F,
}

impl<F> Application<F> {
    pub const fn new(connector: Connector, on_activate: F) -> Self {
        Self {
            connector,
            on_activate,
        }
    }
}

#[zbus::interface(name = "org.freedesktop.Application")]
impl<F: Fn() + Send + Sync + 'static> Application<F> {
    /// Opens a new window. `platform_data` (startup notification ids and
    /// the like) is not used yet.
    ///
    /// The core hears of it first, so it doesn't stop while the window is
    /// still starting; if it has already stopped, the caller gets an error
    /// rather than a window that never appears.
    fn activate(&self, platform_data: HashMap<String, OwnedValue>) -> fdo::Result<()> {
        drop(platform_data);
        self.connector
            .activity()
            .map_err(|error| fdo::Error::Failed(error.to_string()))?;
        (self.on_activate)();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use zbus::object_server::Interface as _;

    use super::{Application, INTERFACE};

    #[test]
    fn interface_name_matches_the_constant() {
        assert_eq!(Application::<fn()>::name().as_str(), INTERFACE);
    }
}
