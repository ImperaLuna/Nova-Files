use std::collections::HashMap;
use std::path::PathBuf;

use nova_files_api::{Connector, WindowRequest, uri};
use zbus::fdo;
use zbus::zvariant::OwnedValue;

use crate::file_manager;

pub const INTERFACE: &str = "org.freedesktop.Application";
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

pub fn local_paths(uris: Vec<String>) -> fdo::Result<Vec<PathBuf>> {
    uris.into_iter()
        .map(|text| {
            uri::path_from_file_uri(&text)
                .ok_or_else(|| fdo::Error::InvalidArgs(format!("not a local file URI: {text}")))
        })
        .collect()
}

pub fn keep_core_up(connector: &Connector) -> fdo::Result<()> {
    connector
        .activity()
        .map_err(|error| fdo::Error::Failed(error.to_string()))
}

#[zbus::interface(name = "org.freedesktop.Application")]
impl<F: Fn(WindowRequest) + Send + Sync + 'static> Application<F> {
    fn activate(&self, platform_data: HashMap<String, OwnedValue>) -> fdo::Result<()> {
        drop(platform_data);
        keep_core_up(&self.connector)?;
        (self.on_open)(WindowRequest::default());
        Ok(())
    }

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
