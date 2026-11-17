//! The icon theme the user chose, from the settings files toolkits keep
//! in the config folder: GTK 3's and GTK 4's `settings.ini`
//! (`gtk-icon-theme-name`), then KDE's `kdeglobals` (`Theme` under
//! `[Icons]`). The first file that names one wins. A name that could
//! reach outside the icons folder (`..`, a `/`) is no name.

use std::path::Path;

use super::ini;
use crate::capped;

/// Each file, the section it keeps the name in, and its key.
const SOURCES: [(&str, &str, &str); 3] = [
    ("gtk-3.0/settings.ini", "Settings", "gtk-icon-theme-name"),
    ("gtk-4.0/settings.ini", "Settings", "gtk-icon-theme-name"),
    ("kdeglobals", "Icons", "Theme"),
];
/// Settings files are a few KB.
const MAX_FILE_BYTES: u64 = 256 * 1024;
const QUOTES: [char; 2] = ['"', '\''];
const SEPARATOR: char = '/';
const NOT_NAMES: [&str; 2] = [".", ".."];

/// The theme named in the settings files under `config`, if any.
pub fn chosen(config: &Path) -> Option<String> {
    SOURCES.iter().find_map(|(file, section, key)| {
        let text = capped::text(&config.join(file), MAX_FILE_BYTES)?;
        ini::value(&text, section, key).and_then(checked)
    })
}

/// `name` if it is a plain folder name.
pub fn checked(name: &str) -> Option<String> {
    let name = name.trim().trim_matches(QUOTES).trim();
    (!name.is_empty() && !name.contains(SEPARATOR) && !NOT_NAMES.contains(&name))
        .then(|| name.to_owned())
}
