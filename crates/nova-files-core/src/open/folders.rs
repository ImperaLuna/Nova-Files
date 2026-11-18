//! Nova Files as the user's file manager: whether it is the default app
//! for folders (`inode/directory`), read the same way as any other default
//! (see `defaults`).

use super::apps::FOLDER_TYPE;
use super::defaults::{self, Places};
use super::environment_places;

/// Nova Files' desktop ID: its desktop entry is named after the app id.
const OWN_DESKTOP_ID: &str = concat!(env!("NOVA_FILES_APP_ID"), ".desktop");

/// Whether Nova Files is the user's default app for folders.
pub fn opens_folders() -> bool {
    opens_folders_in(&environment_places())
}

fn opens_folders_in(places: &Places) -> bool {
    defaults::default_app(&[FOLDER_TYPE], places).is_some_and(|app| app.id == OWN_DESKTOP_ID)
}

#[cfg(test)]
mod tests {
    use super::super::tree::Tree;
    use super::{OWN_DESKTOP_ID, opens_folders_in};

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
}
