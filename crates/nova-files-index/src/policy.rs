//! What the crawl indexes: the home folder, without hidden folders, except a
//! few hidden places worth finding, and without caches and other folders
//! nobody searches.
//!
//! Each folder is crawled in a [`Zone`] that says which of these rules
//! apply inside it. The crawled folder is [`Zone::Home`]: its dotfiles are
//! indexed and its hidden folders are entered only on the way to one of
//! [`ALLOWED`]. Inside an allowed place the zone is open again, minus
//! cache folders. Anywhere, a hidden folder outside the allowed places is
//! skipped, as are `node_modules`, `__pycache__`, and folders that say what
//! they are by a file inside them ([`marked`]): a cache (`CACHEDIR.TAG`), a
//! browser profile, or a Wine or Proton prefix. Hidden files are indexed
//! wherever their folder is.
//!
//! Places never indexed (`~/.ssh`, `~/.gnupg`, `~/.mozilla`, `~/.var`
//! for Flatpak data, `~/.wine`) need no rule of their own: they are hidden
//! folders that aren't allowed.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

/// The hidden places that are indexed, relative to the crawled folder.
/// Everything inside them is, under [`Zone::Open`]'s rules.
pub const ALLOWED: [&str; 7] = [
    ".config",
    ".local/bin",
    ".local/share/applications",
    ".local/share/fonts",
    ".themes",
    ".icons",
    // Installed Steam games. Their Proton prefixes are in
    // steamapps/compatdata, which stays out.
    ".local/share/Steam/steamapps/common",
];

/// Folders skipped wherever they are: rebuilt by tools, never searched.
const SKIPPED: [&str; 2] = ["node_modules", "__pycache__"];

/// Inside an allowed place, a folder whose name ends in one of these, in
/// any case, is a cache: `Cache`, `GPUCache`, `Code Cache`, `ShaderCache`.
const CACHE_ENDINGS: [&str; 2] = ["cache", "caches"];

/// A folder holding one of these files is skipped. `CACHEDIR.TAG` is the
/// cache-folder standard (bford.info/cachedir), set by Cargo, pip and many
/// more; only its presence is checked. `Local State` sits in every
/// Chromium-family browser's profile folder, `profiles.ini` in every
/// Mozilla one.
const MARKERS: [&str; 3] = ["CACHEDIR.TAG", "Local State", "profiles.ini"];

/// A folder holding all of these is a Wine or Proton prefix.
const WINE_PREFIX: [&str; 2] = ["drive_c", "system.reg"];

const HIDDEN: u8 = b'.';

/// Which rules apply to the entries of a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Zone {
    /// The crawled folder itself.
    Home,
    /// A folder whose entries are indexed unless a rule skips them.
    /// `allowed_place` is set inside one of [`ALLOWED`], where cache
    /// folders are skipped too.
    Open { allowed_place: bool },
    /// A hidden folder on the way to an allowed place, at this path
    /// relative to the crawled folder. Only the folders on the way are
    /// indexed.
    Toward(PathBuf),
}

/// Whether a file (or link) named `name` in a folder of `zone` is indexed.
pub const fn keeps_file(zone: &Zone) -> bool {
    !matches!(zone, Zone::Toward(_))
}

/// The zone of the folder `name` inside a folder of `zone`, or `None`
/// when the folder isn't indexed.
pub fn enters(zone: &Zone, name: &OsStr) -> Option<Zone> {
    if SKIPPED.iter().any(|skipped| name == *skipped) {
        return None;
    }
    let hidden = name.as_bytes().first() == Some(&HIDDEN);
    match zone {
        Zone::Home if hidden => toward(Path::new(name)),
        Zone::Home => Some(Zone::Open {
            allowed_place: false,
        }),
        Zone::Open { allowed_place } => {
            let skipped = hidden || (*allowed_place && is_cache(name));
            (!skipped).then(|| zone.clone())
        }
        Zone::Toward(path) => toward(&path.join(name)),
    }
}

/// Whether a folder holding `names` says it isn't worth indexing.
pub fn marked(names: &[OsString]) -> bool {
    let holds = |wanted: &str| names.iter().any(|name| name == wanted);
    MARKERS.iter().any(|marker| holds(marker)) || WINE_PREFIX.iter().all(|file| holds(file))
}

/// The zone of the hidden folder at `path`: open inside an allowed place,
/// on the way while it leads to one, otherwise skipped.
fn toward(path: &Path) -> Option<Zone> {
    if ALLOWED.iter().any(|allowed| path == Path::new(allowed)) {
        Some(Zone::Open {
            allowed_place: true,
        })
    } else if ALLOWED
        .iter()
        .any(|allowed| Path::new(allowed).starts_with(path))
    {
        Some(Zone::Toward(path.to_owned()))
    } else {
        None
    }
}

fn is_cache(name: &OsStr) -> bool {
    let name = name.to_string_lossy().to_lowercase();
    CACHE_ENDINGS.iter().any(|ending| name.ends_with(ending))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPEN: Zone = Zone::Open {
        allowed_place: false,
    };
    const ALLOWED_PLACE: Zone = Zone::Open {
        allowed_place: true,
    };

    fn enter(zone: &Zone, names: &[&str]) -> Option<Zone> {
        names
            .iter()
            .try_fold(zone.clone(), |zone, name| enters(&zone, OsStr::new(name)))
    }

    #[test]
    fn plain_folders_are_open() {
        assert_eq!(enter(&Zone::Home, &["Documents", "Taxes"]), Some(OPEN));
    }

    #[test]
    fn hidden_folders_are_skipped_outside_the_allowed_places() {
        assert_eq!(enter(&Zone::Home, &[".ssh"]), None);
        assert_eq!(enter(&Zone::Home, &[".gnupg"]), None);
        assert_eq!(enter(&Zone::Home, &[".mozilla"]), None);
        assert_eq!(enter(&Zone::Home, &["Projects", "app", ".git"]), None);
        assert_eq!(enter(&Zone::Home, &[".local", "state"]), None);
        assert_eq!(enter(&Zone::Home, &[".local", "share", "Trash"]), None);
    }

    #[test]
    fn allowed_places_are_open_and_their_way_is_followed() {
        for allowed in ALLOWED {
            let names: Vec<&str> = allowed.split('/').collect();
            assert_eq!(enter(&Zone::Home, &names), Some(ALLOWED_PLACE), "{allowed}");
        }
        assert_eq!(
            enter(&Zone::Home, &[".local", "share"]),
            Some(Zone::Toward(PathBuf::from(".local/share")))
        );
        assert_eq!(
            enter(
                &Zone::Home,
                &[".local", "share", "Steam", "steamapps", "compatdata"]
            ),
            None
        );
    }

    #[test]
    fn only_folders_on_the_way_are_kept_there() {
        assert!(keeps_file(&Zone::Home));
        assert!(keeps_file(&OPEN));
        assert!(keeps_file(&ALLOWED_PLACE));
        assert!(!keeps_file(&Zone::Toward(PathBuf::from(".local"))));
    }

    #[test]
    fn caches_are_skipped_inside_allowed_places_only() {
        for cache in ["Cache", "GPUCache", "Code Cache", "ShaderCache", "Caches"] {
            assert_eq!(enter(&ALLOWED_PLACE, &[cache]), None, "{cache}");
        }
        assert_eq!(
            enter(&ALLOWED_PLACE, &["Code", "User"]),
            Some(ALLOWED_PLACE)
        );
        assert_eq!(enter(&OPEN, &["Cache"]), Some(OPEN));
    }

    #[test]
    fn tool_folders_are_skipped_anywhere() {
        for zone in [Zone::Home, OPEN, ALLOWED_PLACE] {
            assert_eq!(enter(&zone, &["node_modules"]), None);
            assert_eq!(enter(&zone, &["__pycache__"]), None);
        }
    }

    #[test]
    fn marker_files_skip_their_folder() {
        let names = |list: &[&str]| list.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(marked(&names(&["CACHEDIR.TAG", "data"])));
        assert!(marked(&names(&["Default", "Local State"])));
        assert!(marked(&names(&["profiles.ini"])));
        assert!(marked(&names(&["drive_c", "system.reg", "user.reg"])));
        assert!(!marked(&names(&["drive_c"])));
        assert!(!marked(&names(&["system.reg"])));
        assert!(!marked(&names(&["notes.txt"])));
        assert!(!marked(&[]));
    }
}
