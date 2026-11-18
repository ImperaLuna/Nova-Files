use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

pub const ALLOWED: [&str; 7] = [
    ".config",
    ".local/bin",
    ".local/share/applications",
    ".local/share/fonts",
    ".themes",
    ".icons",
    ".local/share/Steam/steamapps/common",
];

const SKIPPED: [&str; 2] = ["node_modules", "__pycache__"];

const CACHE_ENDINGS: [&str; 2] = ["cache", "caches"];

const MARKERS: [&str; 2] = ["Local State", "profiles.ini"];

pub const CACHE_TAG: &str = "CACHEDIR.TAG";
pub const CACHE_TAG_SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

const WINE_PREFIX: [&str; 2] = ["drive_c", "system.reg"];

const HIDDEN: u8 = b'.';

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Zone {
    Home,
    Open { allowed_place: bool },
    Toward(PathBuf),
}

pub const fn keeps_file(zone: &Zone) -> bool {
    !matches!(zone, Zone::Toward(_))
}

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

pub fn zone_of(relative: &Path) -> Option<Zone> {
    relative
        .iter()
        .try_fold(Zone::Home, |zone, name| enters(&zone, name))
}

pub fn keeps(zone: &Zone, name: &OsStr, is_folder: bool) -> bool {
    if is_folder {
        enters(zone, name).is_some()
    } else {
        keeps_file(zone)
    }
}

pub fn is_marker(name: &OsStr) -> bool {
    name == CACHE_TAG
        || MARKERS.iter().any(|marker| name == *marker)
        || WINE_PREFIX.iter().any(|file| name == *file)
}

pub fn marked(names: &[OsString]) -> bool {
    let holds = |wanted: &str| names.iter().any(|name| name == wanted);
    MARKERS.iter().any(|marker| holds(marker)) || WINE_PREFIX.iter().all(|file| holds(file))
}

pub fn is_cache_signature(start: &[u8]) -> bool {
    start == CACHE_TAG_SIGNATURE
}

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
    fn a_folder_s_zone_follows_its_path() {
        assert_eq!(zone_of(Path::new("")), Some(Zone::Home));
        assert_eq!(zone_of(Path::new("Documents/Taxes")), Some(OPEN));
        assert_eq!(zone_of(Path::new(".config/app")), Some(ALLOWED_PLACE));
        assert_eq!(zone_of(Path::new(".ssh/keys")), None);
    }

    #[test]
    fn keeps_asks_the_rule_for_its_kind() {
        let toward = Zone::Toward(PathBuf::from(".local"));
        assert!(keeps(&OPEN, OsStr::new("a.txt"), false));
        assert!(!keeps(&toward, OsStr::new("a.txt"), false));
        assert!(keeps(&toward, OsStr::new("bin"), true));
        assert!(!keeps(&OPEN, OsStr::new(".git"), true));
    }

    #[test]
    fn markers_are_the_files_that_can_mark_a_folder() {
        for name in [
            "CACHEDIR.TAG",
            "Local State",
            "profiles.ini",
            "drive_c",
            "system.reg",
        ] {
            assert!(is_marker(OsStr::new(name)), "{name}");
        }
        assert!(!is_marker(OsStr::new("notes.txt")));
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
        assert!(!marked(&names(&["CACHEDIR.TAG"])));
        assert!(marked(&names(&["Default", "Local State"])));
        assert!(marked(&names(&["profiles.ini"])));
        assert!(marked(&names(&["drive_c", "system.reg", "user.reg"])));
        assert!(!marked(&names(&["drive_c"])));
        assert!(!marked(&names(&["system.reg"])));
        assert!(!marked(&names(&["notes.txt"])));
        assert!(!marked(&[]));
    }

    #[test]
    fn only_the_signature_marks_a_cache() {
        assert!(is_cache_signature(CACHE_TAG_SIGNATURE));
        assert!(!is_cache_signature(b""));
        assert!(!is_cache_signature(&CACHE_TAG_SIGNATURE[1..]));
    }
}
