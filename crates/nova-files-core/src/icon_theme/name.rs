//! The icon theme the user chose, from the settings files toolkits keep
//! in the config folder: GTK 3's and GTK 4's `settings.ini`
//! (`gtk-icon-theme-name`), then KDE's `kdeglobals` (`Theme` under
//! `[Icons]`). The first file that names one wins. A name that could
//! reach outside the icons folder (`..`, a `/`) is no name.

use std::fs::File;
use std::io::Read as _;
use std::path::Path;

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
const SECTION_OPEN: char = '[';
const SECTION_CLOSE: char = ']';
const ASSIGN: char = '=';

/// The theme named in the settings files under `config`, if any.
pub fn chosen(config: &Path) -> Option<String> {
    SOURCES.iter().find_map(|(file, section, key)| {
        let text = read_capped(&config.join(file))?;
        value(&text, section, key).and_then(checked)
    })
}

/// `name` if it is a plain folder name.
pub fn checked(name: &str) -> Option<String> {
    let name = name.trim().trim_matches(QUOTES).trim();
    (!name.is_empty() && !name.contains(SEPARATOR) && !NOT_NAMES.contains(&name))
        .then(|| name.to_owned())
}

/// The value of `key` in `[section]` of an ini file's `text`.
fn value<'a>(text: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if let Some(name) = line
            .strip_prefix(SECTION_OPEN)
            .and_then(|rest| rest.strip_suffix(SECTION_CLOSE))
        {
            inside = name == section;
        } else if inside
            && let Some((found, value)) = line.split_once(ASSIGN)
            && found.trim() == key
        {
            return Some(value);
        }
    }
    None
}

fn read_capped(path: &Path) -> Option<String> {
    let mut text = String::new();
    File::open(path)
        .ok()?
        .take(MAX_FILE_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}
