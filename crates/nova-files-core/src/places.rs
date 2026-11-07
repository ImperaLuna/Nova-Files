//! The standard folders for the places list: Home, then the user's
//! Documents, Pictures and Downloads folders, each shown only if it exists.
//!
//! Where those folders are comes from the system's `user-dirs.dirs` file
//! (the freedesktop xdg-user-dirs format), which every desktop writes and
//! which follows the user's language. A folder the file doesn't mention is
//! looked for under its usual name in Home; one it disables (`"$HOME/"`) or
//! puts outside Home is left out.

use std::env;
use std::fs;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use nova_files_api::{Place, PlaceKind};

/// The places, in list order, with the `user-dirs.dirs` key and the folder
/// name used when that file doesn't say.
const STANDARD: [(PlaceKind, &str, &str); 3] = [
    (PlaceKind::Documents, "XDG_DOCUMENTS_DIR", "Documents"),
    (PlaceKind::Pictures, "XDG_PICTURES_DIR", "Pictures"),
    (PlaceKind::Downloads, "XDG_DOWNLOAD_DIR", "Downloads"),
];

const USER_DIRS_FILE: &str = "user-dirs.dirs";
const CONFIG_HOME_VAR: &str = "XDG_CONFIG_HOME";
const CONFIG_IN_HOME: &str = ".config";
/// `user-dirs.dirs` is a few lines; anything bigger isn't one.
const USER_DIRS_MAX_BYTES: u64 = 64 * 1024;
const HOME_PREFIX: &str = "$HOME/";

/// The places that exist on this system, in list order. Empty without a
/// home folder.
pub fn places() -> Vec<Place> {
    let Some(home) = env::home_dir() else {
        return Vec::new();
    };
    let user_dirs = read_user_dirs(&config_home(&home).join(USER_DIRS_FILE));
    places_in(&home, &user_dirs)
}

/// The places under `home`, given the text of `user-dirs.dirs` (empty when
/// there is none).
fn places_in(home: &Path, user_dirs: &str) -> Vec<Place> {
    let home_place = Place {
        kind: PlaceKind::Home,
        path: home.to_owned(),
    };
    let standard = STANDARD.iter().filter_map(|&(kind, key, default)| {
        let path = match user_dir(home, user_dirs, key) {
            Setting::Unset => home.join(default),
            Setting::Folder(path) => path,
            Setting::Refused => return None,
        };
        Some(Place { kind, path })
    });
    std::iter::once(home_place)
        .chain(standard)
        .filter(|place| fs::metadata(&place.path).is_ok_and(|found| found.is_dir()))
        .collect()
}

fn config_home(home: &Path) -> PathBuf {
    // The spec says a relative XDG_CONFIG_HOME is invalid and is ignored.
    let from_env = env::var_os(CONFIG_HOME_VAR)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    from_env.unwrap_or_else(|| home.join(CONFIG_IN_HOME))
}

/// The file's text, or empty when it is missing, unreadable or too big.
fn read_user_dirs(path: &Path) -> String {
    let mut text = String::new();
    let read = fs::File::open(path)
        .and_then(|file| file.take(USER_DIRS_MAX_BYTES + 1).read_to_string(&mut text));
    match read {
        Ok(length) if u64::try_from(length).is_ok_and(|length| length <= USER_DIRS_MAX_BYTES) => {
            text
        }
        _ => String::new(),
    }
}

/// What `user-dirs.dirs` says about one folder.
#[derive(Debug, PartialEq, Eq)]
enum Setting {
    /// Nothing: the usual name under Home applies.
    Unset,
    /// A folder inside Home.
    Folder(PathBuf),
    /// Disabled, outside Home, or not in the format.
    Refused,
}

/// What `user_dirs` says about `key`. The format is `KEY="$HOME/Folder"`,
/// one per line; the file is shell, so the last line for a key wins.
fn user_dir(home: &Path, user_dirs: &str, key: &str) -> Setting {
    let value = user_dirs.lines().rev().find_map(|line| {
        let (name, value) = line.trim().split_once('=')?;
        (name == key).then_some(value)
    });
    let Some(value) = value else {
        return Setting::Unset;
    };
    let relative = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .and_then(|value| value.strip_prefix(HOME_PREFIX))
        .map(Path::new);
    match relative {
        Some(relative)
            if !relative.as_os_str().is_empty()
                && relative
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))) =>
        {
            Setting::Folder(home.join(relative))
        }
        _ => Setting::Refused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/someone";

    fn dir(text: &str, key: &str) -> Setting {
        user_dir(Path::new(HOME), text, key)
    }

    #[test]
    fn reads_a_folder_inside_home() {
        let text = "# comment\nXDG_DOCUMENTS_DIR=\"$HOME/Dokumente\"\n";
        assert_eq!(
            dir(text, "XDG_DOCUMENTS_DIR"),
            Setting::Folder(PathBuf::from("/home/someone/Dokumente"))
        );
    }

    #[test]
    fn last_line_for_a_key_wins() {
        let text = "XDG_DOCUMENTS_DIR=\"$HOME/Old\"\nXDG_DOCUMENTS_DIR=\"$HOME/New\"\n";
        assert_eq!(
            dir(text, "XDG_DOCUMENTS_DIR"),
            Setting::Folder(PathBuf::from("/home/someone/New"))
        );
    }

    #[test]
    fn refuses_folders_outside_home() {
        assert_eq!(
            dir("XDG_DOCUMENTS_DIR=\"/etc\"", "XDG_DOCUMENTS_DIR"),
            Setting::Refused
        );
        assert_eq!(
            dir("XDG_DOCUMENTS_DIR=\"$HOME/../x\"", "XDG_DOCUMENTS_DIR"),
            Setting::Refused
        );
        // `$HOME/` alone means "no such folder" in the format.
        assert_eq!(
            dir("XDG_DOCUMENTS_DIR=\"$HOME/\"", "XDG_DOCUMENTS_DIR"),
            Setting::Refused
        );
    }

    /// A temp home folder, removed when dropped.
    struct TempHome(PathBuf);

    impl Drop for TempHome {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    #[test]
    fn lists_home_then_existing_folders_in_order() {
        let home =
            TempHome(env::temp_dir().join(format!("nova-files-places-{}", std::process::id())));
        fs::create_dir_all(home.0.join("Bilder")).unwrap();
        fs::create_dir_all(home.0.join("Documents")).unwrap();
        fs::create_dir_all(home.0.join("Downloads")).unwrap();
        let user_dirs = "XDG_PICTURES_DIR=\"$HOME/Bilder\"\nXDG_DOWNLOAD_DIR=\"$HOME/\"\n";

        let places = places_in(&home.0, user_dirs);

        let found: Vec<(PlaceKind, PathBuf)> = places
            .into_iter()
            .map(|place| (place.kind, place.path))
            .collect();
        assert_eq!(
            found,
            [
                (PlaceKind::Home, home.0.clone()),
                (PlaceKind::Documents, home.0.join("Documents")),
                (PlaceKind::Pictures, home.0.join("Bilder")),
            ]
        );
    }

    #[test]
    fn missing_key_is_unset() {
        assert_eq!(
            dir("XDG_MUSIC_DIR=\"$HOME/Music\"", "XDG_DOCUMENTS_DIR"),
            Setting::Unset
        );
    }
}
