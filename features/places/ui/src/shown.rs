use std::path::{Path, PathBuf};

use gpui::SharedString;
use nova_files_places_api::{Bookmark, Place, PlaceKind};
use nova_files_shell::assets::Icon;
use nova_files_shell::{KnownFolder, KnownFolders};

pub const HOME_LABEL: &str = "Home";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub path: PathBuf,
    pub name: SharedString,
    pub icon: Icon,
    pub pinned: bool,
}

pub fn shown(places: &[Place], pinned: &[Bookmark]) -> Vec<Shown> {
    let standard = places.iter().filter_map(|place| {
        let icon = standard_icon(place.kind)?;
        Some(Shown {
            path: place.path.clone(),
            name: standard_name(place),
            icon,
            pinned: false,
        })
    });
    let pinned = pinned.iter().map(|bookmark| Shown {
        path: bookmark.path.clone(),
        name: bookmark
            .label
            .clone()
            .map_or_else(|| folder_name(&bookmark.path), SharedString::from),
        icon: places
            .iter()
            .find(|place| place.path == bookmark.path)
            .map_or(Icon::Folder, |place| place_icon(place.kind)),
        pinned: true,
    });
    let mut rows: Vec<Shown> = Vec::new();
    for row in standard.chain(pinned) {
        match rows.iter_mut().find(|shown| shown.path == row.path) {
            Some(shown) => shown.pinned |= row.pinned,
            None => rows.push(row),
        }
    }
    rows
}

impl Shown {
    pub fn is_here(&self, here: Option<&Path>) -> bool {
        here == Some(self.path.as_path())
    }
}

pub const fn place_icon(kind: PlaceKind) -> Icon {
    match kind {
        PlaceKind::Home => Icon::Home,
        PlaceKind::Desktop => Icon::Desktop,
        PlaceKind::Documents => Icon::Documents,
        PlaceKind::Downloads => Icon::Downloads,
        PlaceKind::Music => Icon::Music,
        PlaceKind::Pictures => Icon::Pictures,
        PlaceKind::Videos => Icon::Videos,
    }
}

const fn standard_icon(kind: PlaceKind) -> Option<Icon> {
    match kind {
        PlaceKind::Music | PlaceKind::Videos => None,
        PlaceKind::Home
        | PlaceKind::Desktop
        | PlaceKind::Documents
        | PlaceKind::Downloads
        | PlaceKind::Pictures => Some(place_icon(kind)),
    }
}

pub fn known(places: &[Place]) -> KnownFolders {
    KnownFolders::new(places.iter().map(|place| KnownFolder {
        path: place.path.clone(),
        name: standard_name(place),
        icon: place_icon(place.kind),
    }))
}

fn standard_name(place: &Place) -> SharedString {
    match place.kind {
        PlaceKind::Home => SharedString::from(HOME_LABEL),
        PlaceKind::Desktop
        | PlaceKind::Documents
        | PlaceKind::Downloads
        | PlaceKind::Music
        | PlaceKind::Pictures
        | PlaceKind::Videos => folder_name(&place.path),
    }
}

pub fn folder_name(path: &Path) -> SharedString {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(kind: PlaceKind, path: &str) -> Place {
        Place {
            kind,
            path: PathBuf::from(path),
        }
    }

    fn pin(path: &str, label: Option<&str>) -> Bookmark {
        Bookmark {
            path: PathBuf::from(path),
            label: label.map(str::to_owned),
        }
    }

    fn names(rows: &[Shown]) -> Vec<&str> {
        rows.iter().map(|row| row.name.as_ref()).collect()
    }

    #[test]
    fn standard_folders_come_first_then_the_pinned_ones_in_order() {
        let rows = shown(
            &[
                place(PlaceKind::Home, "/home/me"),
                place(PlaceKind::Documents, "/home/me/Dokumente"),
                place(PlaceKind::Music, "/home/me/Music"),
                place(PlaceKind::Videos, "/home/me/Videos"),
                place(PlaceKind::Pictures, "/home/me/Pictures"),
            ],
            &[
                pin("/home/me/Work", None),
                pin("/mnt/games", Some("Games SSD")),
            ],
        );
        assert_eq!(
            names(&rows),
            ["Home", "Dokumente", "Pictures", "Work", "Games SSD"]
        );
        assert_eq!(
            rows.iter().map(|row| row.pinned).collect::<Vec<_>>(),
            [false, false, false, true, true]
        );
        assert_eq!(rows[3].icon, Icon::Folder);
    }

    #[test]
    fn a_pin_at_a_known_folder_draws_that_folder_s_icon() {
        let rows = shown(
            &[
                place(PlaceKind::Music, "/home/me/Music"),
                place(PlaceKind::Videos, "/home/me/Videos"),
            ],
            &[
                pin("/home/me/Music", None),
                pin("/home/me/Videos", Some("Films")),
                pin("/home/me/Public", None),
            ],
        );
        let icons: Vec<Icon> = rows.iter().map(|row| row.icon).collect();
        assert_eq!(icons, [Icon::Music, Icon::Videos, Icon::Folder]);
    }

    #[test]
    fn a_pinned_folder_already_listed_shows_once() {
        let rows = shown(
            &[place(PlaceKind::Downloads, "/home/me/Downloads")],
            &[
                pin("/home/me/Downloads", Some("Mine")),
                pin("/srv", None),
                pin("/srv", Some("Again")),
            ],
        );
        assert_eq!(names(&rows), ["Downloads", "srv"]);
        assert!(rows[0].pinned, "pinned too, so it can be unpinned");
        assert_eq!(rows[0].icon, Icon::Downloads);
    }

    #[test]
    fn each_standard_folder_has_its_own_icon() {
        let kinds = [
            PlaceKind::Home,
            PlaceKind::Desktop,
            PlaceKind::Documents,
            PlaceKind::Downloads,
            PlaceKind::Pictures,
        ];
        let icons: Vec<Icon> = kinds.into_iter().filter_map(standard_icon).collect();
        assert_eq!(icons.len(), kinds.len());
        for (index, icon) in icons.iter().enumerate() {
            assert!(!icons[..index].contains(icon), "{icon:?} drawn twice");
            assert_ne!(*icon, Icon::Folder);
        }
    }

    #[test]
    fn every_standard_folder_is_known_with_its_icon() {
        let known = known(&[
            place(PlaceKind::Music, "/home/me/Music"),
            place(PlaceKind::Videos, "/home/me/Videos"),
        ]);
        assert_eq!(known.icon_at(Path::new("/home/me/Music")), Icon::Music);
        assert_eq!(known.icon_at(Path::new("/home/me/Videos")), Icon::Videos);
        assert_eq!(known.icon_at(Path::new("/home/me/Work")), Icon::Folder);
    }

    #[test]
    fn the_root_is_named_by_its_path() {
        assert_eq!(folder_name(Path::new("/")), "/");
        assert_eq!(shown(&[], &[pin("/", None)])[0].name, "/");
    }

    #[test]
    fn a_place_is_here_only_while_its_folder_is_on_screen() {
        let rows = shown(&[place(PlaceKind::Home, "/home/me")], &[]);
        assert!(rows[0].is_here(Some(Path::new("/home/me"))));
        assert!(!rows[0].is_here(Some(Path::new("/home/me/Work"))));
        assert!(!rows[0].is_here(None));
    }

    #[test]
    fn every_folder_kind_has_an_icon_of_its_own() {
        let kinds = [
            PlaceKind::Home,
            PlaceKind::Desktop,
            PlaceKind::Documents,
            PlaceKind::Downloads,
            PlaceKind::Music,
            PlaceKind::Pictures,
            PlaceKind::Videos,
        ];
        let icons: Vec<Icon> = kinds.into_iter().map(place_icon).collect();
        for (index, icon) in icons.iter().enumerate() {
            assert!(!icons[..index].contains(icon), "{icon:?} drawn twice");
        }
        assert_eq!(place_icon(PlaceKind::Home), Icon::Home);
    }
}
