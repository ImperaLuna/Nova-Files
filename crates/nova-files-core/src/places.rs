//! The standard folders for the places list: Home, then the user's
//! Documents, Pictures and Downloads folders, each shown only if it exists.
//!
//! Where those folders are comes from the system's `user-dirs.dirs` file
//! (the freedesktop xdg-user-dirs format), which every desktop writes and
//! which follows the user's language. A folder the file doesn't mention is
//! looked for under its usual name in Home; one it disables (`"$HOME/"`) or
//! puts outside Home is left out.

use std::env;
use std::ffi::OsString;
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
const STATE_HOME_VAR: &str = "XDG_STATE_HOME";
const STATE_IN_HOME: &str = ".local/state";
const DATA_HOME_VAR: &str = "XDG_DATA_HOME";
const DATA_IN_HOME: &str = ".local/share";
const CACHE_HOME_VAR: &str = "XDG_CACHE_HOME";
const CACHE_IN_HOME: &str = ".cache";
/// `user-dirs.dirs` is a few lines; anything bigger isn't one.
const USER_DIRS_MAX_BYTES: u64 = 64 * 1024;
const HOME_PREFIX: &str = "$HOME/";
const ESCAPE: char = '\\';

/// The user's home folder, its config folder, where the places come from,
/// its state folder and its data folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Home {
    pub folder: PathBuf,
    /// Holds `user-dirs.dirs`.
    pub config: PathBuf,
    /// Holds the UI's window state.
    pub state: PathBuf,
    /// Holds the home trash.
    pub data: PathBuf,
    /// Whether the trashes at the top of other drives count, found from
    /// the system's mount table. Tests leave them out, so they never
    /// touch the real ones.
    pub drive_trashes: bool,
}

impl Home {
    /// The home, config, state and data folders the environment names
    /// (`HOME`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `XDG_DATA_HOME`), or
    /// `None` without a home folder.
    pub fn from_env() -> Option<Self> {
        let folder = env::home_dir()?;
        let config = xdg_home(&folder, env::var_os(CONFIG_HOME_VAR), CONFIG_IN_HOME);
        let state = xdg_home(&folder, env::var_os(STATE_HOME_VAR), STATE_IN_HOME);
        let data = xdg_home(&folder, env::var_os(DATA_HOME_VAR), DATA_IN_HOME);
        Some(Self {
            folder,
            config,
            state,
            data,
            drive_trashes: true,
        })
    }
}

/// The user's cache folder: `$XDG_CACHE_HOME`, or `~/.cache` when that is
/// unset or relative. `None` without either.
pub fn cache_home() -> Option<PathBuf> {
    cache_home_from(env::var_os(CACHE_HOME_VAR), env::home_dir())
}

fn cache_home_from(variable: Option<OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    variable
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| home.map(|home| home.join(CACHE_IN_HOME)))
}

/// The places that exist under `home`, in list order. Empty without a
/// home folder.
pub fn places(home: Option<&Home>) -> Vec<Place> {
    let Some(home) = home else {
        return Vec::new();
    };
    let user_dirs = read_user_dirs(&home.config.join(USER_DIRS_FILE));
    places_in(&home.folder, &user_dirs)
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

/// An XDG folder: its variable's value (`from_env`) when it is absolute,
/// which the spec requires, else `in_home` in Home.
fn xdg_home(home: &Path, from_env: Option<OsString>, in_home: &str) -> PathBuf {
    from_env
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(in_home))
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
        .map(unescape);
    match relative.as_deref().map(Path::new) {
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

/// `text` with the shell's backslash escapes inside double quotes taken
/// off (`\$` is `$`), as xdg-user-dirs writes them. Nothing is run.
fn unescape(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(next) = chars.next() {
        if next == ESCAPE
            && let Some(escaped) = chars.next()
        {
            plain.push(escaped);
        } else {
            plain.push(next);
        }
    }
    plain
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
    fn decodes_escaped_characters() {
        let text = r#"XDG_DOCUMENTS_DIR="$HOME/\$Docs \"x\" \\y""#;
        assert_eq!(
            dir(text, "XDG_DOCUMENTS_DIR"),
            Setting::Folder(PathBuf::from(r#"/home/someone/$Docs "x" \y"#))
        );
        // A backslash with nothing after it stays.
        let lone = format!("a{ESCAPE}");
        assert_eq!(unescape(&lone), lone);
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
    fn xdg_folders_come_from_an_absolute_env_value_only() {
        let home = Path::new(HOME);
        assert_eq!(
            xdg_home(home, Some(OsString::from("/etc/xdg-me")), CONFIG_IN_HOME),
            PathBuf::from("/etc/xdg-me")
        );
        assert_eq!(
            xdg_home(home, Some(OsString::from("relative")), CONFIG_IN_HOME),
            PathBuf::from("/home/someone/.config")
        );
        assert_eq!(
            xdg_home(home, None, STATE_IN_HOME),
            PathBuf::from("/home/someone/.local/state")
        );
    }

    #[test]
    fn reads_user_dirs_up_to_the_size_cap() {
        let folder =
            TempHome(env::temp_dir().join(format!("nova-files-user-dirs-{}", std::process::id())));
        fs::create_dir_all(&folder.0).unwrap();
        let small = folder.0.join("small");
        let exact = folder.0.join("exact");
        let big = folder.0.join("big");
        let cap = usize::try_from(USER_DIRS_MAX_BYTES).unwrap();
        fs::write(&small, "XDG_DOCUMENTS_DIR=\"$HOME/Docs\"\n").unwrap();
        fs::write(&exact, "a".repeat(cap)).unwrap();
        fs::write(&big, "a".repeat(cap + 1)).unwrap();

        assert_eq!(read_user_dirs(&small), "XDG_DOCUMENTS_DIR=\"$HOME/Docs\"\n");
        assert_eq!(read_user_dirs(&exact).len(), cap);
        assert_eq!(read_user_dirs(&big), "");
        assert_eq!(read_user_dirs(&folder.0.join("missing")), "");
        assert_eq!(USER_DIRS_MAX_BYTES, 65_536);
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

    #[test]
    fn cache_home_prefers_an_absolute_variable() {
        let home = Some(PathBuf::from("/home/me"));
        assert_eq!(
            cache_home_from(Some("/var/cache/me".into()), home.clone()),
            Some(PathBuf::from("/var/cache/me"))
        );
        assert_eq!(
            cache_home_from(Some("relative".into()), home.clone()),
            Some(PathBuf::from("/home/me/.cache"))
        );
        assert_eq!(
            cache_home_from(None, home),
            Some(PathBuf::from("/home/me/.cache"))
        );
        assert_eq!(cache_home_from(None, None), None);
    }

    #[test]
    fn cache_home_reads_the_environment() {
        assert_eq!(
            cache_home(),
            cache_home_from(env::var_os(CACHE_HOME_VAR), env::home_dir())
        );
        assert!(cache_home().is_some_and(|cache| cache.is_absolute()));
    }
}
