//! The path bar's crumbs, worked out from a folder's path alone: one per
//! level, from a root to the folder on screen. The root is Home for a
//! folder under the home folder and Computer (`/`) for any other. Each
//! crumb names the folder it goes to. A place's folder takes the place's
//! icon; any other, the folder icon. Only the root and the folder on
//! screen show theirs.

use std::path::{Component, Path, PathBuf};

use gpui::SharedString;
use nova_files_api::{Place, PlaceKind};

use crate::assets::Icon;
use crate::window::ROOT_FOLDER;

const HOME_LABEL: &str = "Home";
const COMPUTER_LABEL: &str = "Computer";

/// One level of the path bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crumb {
    /// The folder a click goes to.
    pub path: PathBuf,
    pub label: SharedString,
    pub icon: Icon,
    /// Whether the icon shows: on the root and the folder on screen.
    pub shows_icon: bool,
    /// Whether this is the folder on screen, the last crumb.
    pub here: bool,
}

impl Crumb {
    const fn new(path: PathBuf, label: SharedString, icon: Icon) -> Self {
        Self {
            path,
            label,
            icon,
            shows_icon: false,
            here: false,
        }
    }
}

/// The part of `folder` below `home`, if `folder` is home or in it. A
/// home that is relative or the root itself is no home: every path would
/// be in it.
pub fn under_home<'a>(folder: &'a Path, home: &Path) -> Option<&'a Path> {
    if !home.has_root() || home.parent().is_none() {
        return None;
    }
    folder.strip_prefix(home).ok()
}

/// The crumbs from a root down to `folder`, with `home` as the home
/// folder; a folder in `places` is drawn with `place_icon` of its kind.
pub fn crumbs(
    folder: &Path,
    home: &Path,
    places: &[Place],
    place_icon: fn(PlaceKind) -> Icon,
) -> Vec<Crumb> {
    let (root, mut crumbs) = if under_home(folder, home).is_some() {
        let crumb = Crumb::new(home.to_path_buf(), HOME_LABEL.into(), Icon::Home);
        (home, vec![crumb])
    } else {
        let root = Path::new(ROOT_FOLDER);
        let crumb = Crumb::new(root.to_path_buf(), COMPUTER_LABEL.into(), Icon::Drive);
        (root, vec![crumb])
    };
    let mut path = root.to_path_buf();
    let below = folder.strip_prefix(root).unwrap_or(folder);
    for part in below.components() {
        let Component::Normal(name) = part else {
            continue;
        };
        path.push(name);
        let icon = places
            .iter()
            .find(|place| place.path == path)
            .map_or(Icon::Folder, |place| place_icon(place.kind));
        let label = name.to_string_lossy().into_owned().into();
        crumbs.push(Crumb::new(path.clone(), label, icon));
    }
    if let Some(root) = crumbs.first_mut() {
        root.shows_icon = true;
    }
    if let Some(here) = crumbs.last_mut() {
        here.shows_icon = true;
        here.here = true;
    }
    crumbs
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/me";

    /// A place's icon, as the sidebar would draw it.
    const fn place_icon(kind: PlaceKind) -> Icon {
        match kind {
            PlaceKind::Documents => Icon::Documents,
            _ => Icon::Pictures,
        }
    }

    fn of(folder: &Path) -> Vec<Crumb> {
        crumbs(folder, Path::new(HOME), &[], place_icon)
    }

    fn labels(crumbs: &[Crumb]) -> Vec<&str> {
        crumbs.iter().map(|crumb| crumb.label.as_ref()).collect()
    }

    #[test]
    fn a_folder_under_home_starts_at_home() {
        let found = of(Path::new("/home/me/Projects/nova"));
        assert_eq!(labels(&found), ["Home", "Projects", "nova"]);
        assert_eq!(found[0].icon, Icon::Home);
        assert_eq!(found[0].path, PathBuf::from(HOME));
        assert_eq!(found[2].path, PathBuf::from("/home/me/Projects/nova"));
        assert_eq!(found[2].icon, Icon::Folder);
    }

    #[test]
    fn only_the_root_and_the_folder_on_screen_show_their_icons() {
        let found = of(Path::new("/home/me/Projects/nova"));
        let shown: Vec<bool> = found.iter().map(|crumb| crumb.shows_icon).collect();
        assert_eq!(shown, [true, false, true]);
        let here: Vec<bool> = found.iter().map(|crumb| crumb.here).collect();
        assert_eq!(here, [false, false, true]);
    }

    #[test]
    fn home_itself_is_one_crumb_that_is_both_root_and_here() {
        let found = of(Path::new(HOME));
        assert_eq!(labels(&found), ["Home"]);
        assert!(found[0].shows_icon && found[0].here);
    }

    #[test]
    fn a_folder_elsewhere_starts_at_computer() {
        let found = of(Path::new("/etc/nixos"));
        assert_eq!(labels(&found), ["Computer", "etc", "nixos"]);
        assert_eq!(found[0].path, PathBuf::from("/"));
        assert_eq!(found[0].icon, Icon::Drive);
        assert_eq!(found[1].path, PathBuf::from("/etc"));
        assert_eq!(labels(&of(Path::new("/"))), ["Computer"]);
    }

    #[test]
    fn a_name_only_sharing_homes_start_is_not_under_home() {
        let found = of(Path::new("/home/meg"));
        assert_eq!(labels(&found), ["Computer", "home", "meg"]);
    }

    #[test]
    fn a_home_at_the_root_is_no_home() {
        let found = crumbs(Path::new("/etc"), Path::new("/"), &[], place_icon);
        assert_eq!(labels(&found), ["Computer", "etc"]);
        assert_eq!(under_home(Path::new("/etc"), Path::new("/")), None);
        assert_eq!(
            under_home(Path::new("/home/me/a"), Path::new("/home/me")),
            Some(Path::new("a"))
        );
    }

    #[test]
    fn a_relative_home_is_never_a_root() {
        let found = crumbs(Path::new("me/a"), Path::new("me"), &[], place_icon);
        assert_eq!(labels(&found), ["Computer", "me", "a"]);
    }

    #[test]
    fn a_places_folder_takes_its_icon() {
        let places = [Place {
            kind: PlaceKind::Documents,
            path: PathBuf::from("/home/me/Dokumente"),
        }];
        let folder = Path::new("/home/me/Dokumente/a");
        let found = crumbs(folder, Path::new(HOME), &places, place_icon);
        assert_eq!(found[1].icon, Icon::Documents);
        assert_eq!(found[2].icon, Icon::Folder);
    }

    #[test]
    fn a_name_that_is_not_utf8_still_shows() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt as _;
        let folder = Path::new("/tmp").join(OsStr::from_bytes(b"caf\xe9"));
        let found = of(&folder);
        assert_eq!(labels(&found), ["Computer", "tmp", "caf\u{FFFD}"]);
        assert_eq!(found[2].path, folder);
    }
}
