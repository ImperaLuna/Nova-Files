//! The file picker: the xdg-desktop-portal `FileChooser` backend, so other
//! apps' Open and Save dialogs open Nova Files. Empty until the picker
//! lands (FIL-36); the layout is fixed here.
//!
//! xdg-desktop-portal finds a backend by its own bus name, so the running
//! instance asks for [`BUS_NAME`] beside the application id and serves this
//! interface at [`OBJECT_PATH`], the path every portal backend uses. Its
//! interface name and methods are the portal's, not ours, so it carries no
//! version of its own. A pick request takes a hold on the core (see
//! `holds`) until the user answers, the asking app calls `Request.Close`
//! or the caller leaves the bus.

/// The backend's bus name, set once in `.cargo/config.toml`. A `.portal`
/// file names it, and `portals.conf` picks it by the `.portal` file's name.
pub const BUS_NAME: &str = env!("NOVA_FILES_PORTAL_BUS_NAME");

/// Where xdg-desktop-portal calls every backend.
pub const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";

/// The interface name. The `#[zbus::interface]` attribute needs the literal
/// too; a test checks the two match.
pub const INTERFACE: &str = "org.freedesktop.impl.portal.FileChooser";

pub struct FileChooser;

#[zbus::interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl FileChooser {}

#[cfg(test)]
mod tests {
    use zbus::object_server::Interface as _;

    use super::{FileChooser, INTERFACE};

    #[test]
    fn interface_name_matches_the_constant() {
        assert_eq!(FileChooser::name().as_str(), INTERFACE);
    }
}
