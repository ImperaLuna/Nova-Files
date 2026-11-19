use std::path::Path;

use super::ini;
use nova_files_disk::capped;

const SOURCES: [(&str, &str, &str); 3] = [
    ("gtk-3.0/settings.ini", "Settings", "gtk-icon-theme-name"),
    ("gtk-4.0/settings.ini", "Settings", "gtk-icon-theme-name"),
    ("kdeglobals", "Icons", "Theme"),
];
const MAX_FILE_BYTES: u64 = 256 * 1024;
const QUOTES: [char; 2] = ['"', '\''];
const SEPARATOR: char = '/';
const NOT_NAMES: [&str; 2] = [".", ".."];

pub fn chosen(config: &Path) -> Option<String> {
    SOURCES.iter().find_map(|(file, section, key)| {
        let text = capped::text(&config.join(file), MAX_FILE_BYTES)?;
        ini::value(&text, section, key).and_then(checked)
    })
}

pub fn checked(name: &str) -> Option<String> {
    let name = name.trim().trim_matches(QUOTES).trim();
    (!name.is_empty() && !name.contains(SEPARATOR) && !NOT_NAMES.contains(&name))
        .then(|| name.to_owned())
}
