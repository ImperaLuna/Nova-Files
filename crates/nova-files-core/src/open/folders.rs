use std::path::Path;
use std::{fmt, io};

use super::apps::FOLDER_TYPE;
use super::associate::{self, NotSet};
use super::defaults::{self, Places};
use super::{FLATPAK_INFO, environment_places};

const OWN_DESKTOP_ID: &str = concat!(env!("NOVA_FILES_APP_ID"), ".desktop");

pub fn opens_folders() -> bool {
    opens_folders_in(&environment_places())
}

fn opens_folders_in(places: &Places) -> bool {
    defaults::default_app(&[FOLDER_TYPE], places).is_some_and(|app| app.id == OWN_DESKTOP_ID)
}

pub fn make_default_for_folders() -> Result<(), NotDefault> {
    if Path::new(FLATPAK_INFO).exists() {
        return Err(NotDefault::Sandboxed);
    }
    make_default_in(&environment_places())
}

fn make_default_in(places: &Places) -> Result<(), NotDefault> {
    associate::set_default(FOLDER_TYPE, OWN_DESKTOP_ID, places).map_err(|error| match error {
        NotSet::Invalid => NotDefault::NotInstalled,
        NotSet::NoHome => NotDefault::NoHome,
        NotSet::Failed(error) => NotDefault::Failed(error),
    })
}

#[derive(Debug)]
pub enum NotDefault {
    NotInstalled,
    Sandboxed,
    NoHome,
    Failed(io::Error),
}

impl fmt::Display for NotDefault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInstalled => write!(
                f,
                "no usable {OWN_DESKTOP_ID} in the applications folders of XDG_DATA_HOME or XDG_DATA_DIRS; install Nova Files first"
            ),
            Self::Sandboxed => f.write_str("can't change the default for folders from a Flatpak"),
            Self::NoHome => f.write_str("no config folder: set XDG_CONFIG_HOME or HOME"),
            Self::Failed(error) => write!(f, "can't update mimeapps.list: {error}"),
        }
    }
}

impl std::error::Error for NotDefault {}

#[cfg(test)]
mod tests {
    use super::super::tree::Tree;
    use super::{NotDefault, OWN_DESKTOP_ID, make_default_in, opens_folders_in};

    const ENTRY: &str = "[Desktop Entry]\nType=Application\nExec=app %u\n";

    #[test]
    fn opens_folders_only_while_the_default_for_them() {
        let tree = Tree::new("folders-default");
        tree.write(&format!("share/applications/{OWN_DESKTOP_ID}"), ENTRY)
            .write("share/applications/other.desktop", ENTRY);
        assert!(!opens_folders_in(&tree.places()));

        tree.write(
            "config/mimeapps.list",
            "[Default Applications]\ninode/directory=other.desktop\n",
        );
        assert!(!opens_folders_in(&tree.places()));

        tree.write(
            "config/mimeapps.list",
            &format!("[Default Applications]\ninode/directory={OWN_DESKTOP_ID}\n"),
        );
        assert!(opens_folders_in(&tree.places()));
    }

    #[test]
    fn a_default_whose_entry_is_gone_does_not_count() {
        let tree = Tree::new("folders-uninstalled");
        tree.write(
            "config/mimeapps.list",
            &format!("[Default Applications]\ninode/directory={OWN_DESKTOP_ID}\n"),
        );
        assert!(!opens_folders_in(&tree.places()));
    }

    #[test]
    fn making_it_the_default_makes_it_open_folders() {
        let tree = Tree::new("folders-make-default");
        tree.write(&format!("share/applications/{OWN_DESKTOP_ID}"), ENTRY)
            .write("share/applications/other.desktop", ENTRY)
            .write(
                "config/mimeapps.list",
                "[Default Applications]\ninode/directory=other.desktop\n",
            );

        make_default_in(&tree.places()).unwrap();

        assert!(opens_folders_in(&tree.places()));
    }

    #[test]
    fn it_cant_be_made_the_default_until_installed() {
        let tree = Tree::new("folders-make-default-uninstalled");

        let error = make_default_in(&tree.places()).unwrap_err();

        assert!(matches!(error, NotDefault::NotInstalled), "{error:?}");
        assert!(!opens_folders_in(&tree.places()));
    }
}
