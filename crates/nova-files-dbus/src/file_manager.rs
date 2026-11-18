use std::path::PathBuf;

use enumflags2::BitFlags;
use nova_files_api::{Connector, WindowRequest};
use zbus::fdo::{self, RequestNameFlags, RequestNameReply};

use crate::application::{keep_core_up, local_paths};

pub const BUS_NAME: &str = "org.freedesktop.FileManager1";
pub const OBJECT_PATH: &str = "/org/freedesktop/FileManager1";
pub const INTERFACE: &str = "org.freedesktop.FileManager1";
pub const MAX_URIS: usize = 256;

pub struct FileManager<F> {
    connector: Connector,
    on_open: F,
}

impl<F> FileManager<F> {
    pub const fn new(connector: Connector, on_open: F) -> Self {
        Self { connector, on_open }
    }
}

impl<F: Fn(WindowRequest)> FileManager<F> {
    fn open(&self, requests: Vec<WindowRequest>) -> fdo::Result<()> {
        if requests.is_empty() {
            return Ok(());
        }
        keep_core_up(&self.connector)?;
        for request in requests {
            (self.on_open)(request);
        }
        Ok(())
    }
}

#[zbus::interface(name = "org.freedesktop.FileManager1")]
impl<F: Fn(WindowRequest) + Send + Sync + 'static> FileManager<F> {
    fn show_folders(&self, uris: Vec<String>, startup_id: String) -> fdo::Result<()> {
        drop(startup_id);
        let folders = paths(uris)?;
        self.open(folders.into_iter().map(WindowRequest::on).collect())
    }

    fn show_items(&self, uris: Vec<String>, startup_id: String) -> fdo::Result<()> {
        drop(startup_id);
        self.open(showing(paths(uris)?)?)
    }

    fn show_item_properties(&self, uris: Vec<String>, startup_id: String) -> fdo::Result<()> {
        self.show_items(uris, startup_id)
    }
}

pub async fn queue_for_name(connection: &zbus::Connection) -> zbus::Result<RequestNameReply> {
    connection
        .request_name_with_flags(BUS_NAME, BitFlags::<RequestNameFlags>::EMPTY)
        .await
}

fn paths(uris: Vec<String>) -> fdo::Result<Vec<PathBuf>> {
    if uris.len() > MAX_URIS {
        return Err(fdo::Error::InvalidArgs(format!(
            "{} URIs, more than {MAX_URIS}",
            uris.len()
        )));
    }
    local_paths(uris)
}

fn showing(items: Vec<PathBuf>) -> fdo::Result<Vec<WindowRequest>> {
    let mut windows: Vec<WindowRequest> = Vec::new();
    for item in items {
        let shown = WindowRequest::showing(&item).ok_or_else(|| {
            fdo::Error::InvalidArgs(format!("no folder holds {}", item.display()))
        })?;
        match windows
            .iter_mut()
            .find(|window| window.folder == shown.folder)
        {
            Some(window) => {
                for name in shown.select {
                    if !window.select.contains(&name) {
                        window.select.push(name);
                    }
                }
            }
            None => windows.push(shown),
        }
    }
    Ok(windows)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use nova_files_api::WindowRequest;
    use zbus::object_server::Interface as _;

    use super::{FileManager, INTERFACE, showing};

    #[test]
    fn interface_name_matches_the_constant() {
        assert_eq!(FileManager::<fn(WindowRequest)>::name().as_str(), INTERFACE);
    }

    #[test]
    fn items_in_one_folder_share_a_window_and_repeats_show_once() {
        let items = ["/a/x", "/b/y", "/a/z", "/a/x"].map(PathBuf::from).to_vec();

        let windows = showing(items).unwrap();

        let folders: Vec<_> = windows.iter().map(|w| w.folder.clone().unwrap()).collect();
        assert_eq!(folders, [PathBuf::from("/a"), PathBuf::from("/b")]);
        assert_eq!(windows[0].select, ["x", "z"]);
        assert_eq!(windows[1].select, ["y"]);
    }

    #[test]
    fn the_root_has_no_folder_to_show_it_in() {
        assert!(showing(vec![PathBuf::from("/")]).is_err());
    }
}
