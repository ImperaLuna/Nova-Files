//! The user's default app for a MIME type (freedesktop "Association
//! between MIME types and applications" spec).
//!
//! `mimeapps.list` files are read in the spec's order: the config home,
//! then each config folder, then `applications` in the data home and each
//! data folder; in each place the desktop's own `<desktop>-mimeapps.list`
//! for every name in `XDG_CURRENT_DESKTOP` comes before the plain one. The
//! first `Default Applications` entry that is installed wins. Without one,
//! `Added Associations` follow in the same order, then each data folder's
//! `mimeinfo.cache`. A `Removed Associations` entry hides the app from its
//! own list and every lower one, defaults included.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use super::entry::{self, Entry};

pub const LIST: &str = "mimeapps.list";
pub const DESKTOP_LIST_SUFFIX: &str = "-mimeapps.list";
const APPLICATIONS: &str = "applications";
const CACHE: &str = "mimeinfo.cache";
pub const DEFAULTS: &str = "Default Applications";
pub const ADDED: &str = "Added Associations";
pub const REMOVED: &str = "Removed Associations";
const CACHE_GROUP: &str = "MIME Cache";
pub const ID_SEPARATOR: char = ';';
/// A desktop ID names `applications/a/b.desktop` as `a-b.desktop`.
const ID_FOLDER_SEPARATOR: char = '-';
/// Big systems have a cache of a few hundred KB.
/// 4 MiB.
pub const MAX_LIST_BYTES: u64 = 4_194_304;

/// Where to look.
#[derive(Debug, Clone, Default)]
pub struct Places {
    /// The config home, then the config folders.
    pub config: Vec<PathBuf>,
    /// The config home alone, where this user's choices are saved; `None`
    /// without a home folder.
    pub config_home: Option<PathBuf>,
    /// The data home, then the data folders.
    pub data: Vec<PathBuf>,
    /// `XDG_CURRENT_DESKTOP`'s names, lower case, in order.
    pub desktops: Vec<String>,
    /// `PATH`, for `TryExec`.
    pub search_path: Option<OsString>,
}

/// An app that opens files, its desktop ID and the desktop entry it came
/// from.
#[derive(Debug, PartialEq, Eq)]
pub struct App {
    pub id: String,
    pub entry: Entry,
    pub file: PathBuf,
}

/// The association lists as read once: every `mimeapps.list`, highest
/// first, and every `mimeinfo.cache`.
pub struct Associations {
    lists: Vec<String>,
    caches: Vec<String>,
}

impl Associations {
    /// Reads the lists and caches in `places`.
    pub fn read(places: &Places) -> Self {
        Self {
            lists: list_files(places)
                .iter()
                .filter_map(|file| read_capped(file))
                .collect(),
            caches: places
                .data
                .iter()
                .filter_map(|dir| read_capped(&dir.join(APPLICATIONS).join(CACHE)))
                .collect(),
        }
    }

    /// The default app for the first of `types` (a type, then the types
    /// it is a kind of) that has one.
    pub fn default_app(&self, types: &[&str], places: &Places) -> Option<App> {
        types.iter().find_map(|mime| self.default_for(mime, places))
    }

    fn default_for(&self, mime: &str, places: &Places) -> Option<App> {
        let install = |id: &str| installed(id, places);
        listed(&self.lists, DEFAULTS, mime)
            .into_iter()
            .chain(listed(&self.lists, ADDED, mime))
            .chain(self.cached(mime))
            .find_map(install)
    }

    /// Every app ID associated with `mime`, best first: the defaults, then
    /// the added associations, list by list, then the caches. Each shows
    /// once; removed ones are left out the way [`Self::default_app`] skips
    /// them. Not checked for being installed.
    pub fn ids_for(&self, mime: &str) -> Vec<&str> {
        let mut seen = HashSet::new();
        listed(&self.lists, DEFAULTS, mime)
            .into_iter()
            .chain(listed(&self.lists, ADDED, mime))
            .chain(self.cached(mime))
            .filter(|id| seen.insert(*id))
            .collect()
    }

    /// The cache's apps for `mime` that no list removed.
    fn cached(&self, mime: &str) -> Vec<&str> {
        let removed: HashSet<&str> = self
            .lists
            .iter()
            .flat_map(|text| ids(text, REMOVED, mime))
            .collect();
        self.caches
            .iter()
            .flat_map(|text| ids(text, CACHE_GROUP, mime))
            .filter(|id| !removed.contains(id))
            .collect()
    }
}

/// The default app for the first of `types` (a type, then the types it
/// is a kind of) that has one.
pub fn default_app(types: &[&str], places: &Places) -> Option<App> {
    Associations::read(places).default_app(types, places)
}

/// The apps `group` lists for `mime`, list by list, skipping any that
/// list or a higher one removed.
fn listed<'a>(lists: &'a [String], group: &str, mime: &str) -> Vec<&'a str> {
    let mut removed = HashSet::new();
    let mut found = Vec::new();
    for text in lists {
        removed.extend(ids(text, REMOVED, mime));
        found.extend(
            ids(text, group, mime)
                .into_iter()
                .filter(|id| !removed.contains(id)),
        );
    }
    found
}

/// Every `mimeapps.list` place, highest precedence first.
fn list_files(places: &Places) -> Vec<PathBuf> {
    let in_dir = |dir: &Path| {
        places
            .desktops
            .iter()
            .map(|desktop| dir.join(format!("{desktop}{DESKTOP_LIST_SUFFIX}")))
            .chain([dir.join(LIST)])
            .collect::<Vec<_>>()
    };
    let data_apps: Vec<PathBuf> = places
        .data
        .iter()
        .map(|dir| dir.join(APPLICATIONS))
        .collect();
    places
        .config
        .iter()
        .chain(&data_apps)
        .flat_map(|dir| in_dir(dir))
        .collect()
}

/// The desktop IDs `group` lists for `mime` in an ini-style file.
fn ids<'a>(text: &'a str, group: &str, mime: &str) -> Vec<&'a str> {
    let mut in_group = false;
    for line in text.lines().map(str::trim) {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            in_group = name == group;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if in_group && key.trim_end() == mime {
            return value
                .split(ID_SEPARATOR)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .collect();
        }
    }
    Vec::new()
}

/// The app desktop ID `id` names, from the first data folder that has it.
/// A hidden or broken entry there hides the ones below it.
pub fn installed(id: &str, places: &Places) -> Option<App> {
    let file = places
        .data
        .iter()
        .find_map(|dir| find_entry(&dir.join(APPLICATIONS), id))?;
    let entry = entry::read(&file, places.search_path.as_deref())?;
    Some(App {
        id: id.to_owned(),
        entry,
        file,
    })
}

/// `id` under `apps`, as a file or, where its dashes may stand for
/// folders, inside them (`a-b.desktop` is also `a/b.desktop`).
fn find_entry(apps: &Path, id: &str) -> Option<PathBuf> {
    if id.contains('/') || id.starts_with('.') {
        return None;
    }
    let direct = apps.join(id);
    if direct.is_file() {
        return Some(direct);
    }
    id.match_indices(ID_FOLDER_SEPARATOR).find_map(|(at, _)| {
        let (folder, rest) = (&id[..at], &id[at + 1..]);
        let inside = apps.join(folder);
        (!folder.is_empty() && inside.is_dir())
            .then(|| find_entry(&inside, rest))
            .flatten()
    })
}

fn read_capped(path: &Path) -> Option<String> {
    let mut text = String::new();
    File::open(path)
        .ok()?
        .take(MAX_LIST_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

#[cfg(test)]
mod tests {

    use super::*;

    use crate::open::tree::Tree;

    trait Defaults {
        fn app(&self, path: &str) -> &Self;
        fn all_places(&self) -> Places;
        fn default_for(&self, types: &[&str]) -> Option<String>;
    }

    impl Defaults for Tree {
        fn app(&self, path: &str) -> &Self {
            self.write(path, "[Desktop Entry]\nType=Application\nExec=app %f\n")
        }

        fn all_places(&self) -> Places {
            self.places_in(&["config", "etc"], &["local", "share"], &["nova", "gnome"])
        }

        /// Where the default for `types` comes from, relative to the tree.
        fn default_for(&self, types: &[&str]) -> Option<String> {
            default_app(types, &self.all_places()).map(|app| {
                app.file
                    .strip_prefix(&self.0)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
        }
    }

    #[test]
    fn lists_are_read_in_the_specs_order() {
        let tree = Tree::new("defaults-order");
        let places = tree.all_places();
        let names: Vec<String> = list_files(&places)
            .iter()
            .map(|file| {
                file.strip_prefix(&tree.0)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(
            names,
            [
                "config/nova-mimeapps.list",
                "config/gnome-mimeapps.list",
                "config/mimeapps.list",
                "etc/nova-mimeapps.list",
                "etc/gnome-mimeapps.list",
                "etc/mimeapps.list",
                "local/applications/nova-mimeapps.list",
                "local/applications/gnome-mimeapps.list",
                "local/applications/mimeapps.list",
                "share/applications/nova-mimeapps.list",
                "share/applications/gnome-mimeapps.list",
                "share/applications/mimeapps.list",
            ]
        );
    }

    #[test]
    fn the_first_installed_default_wins() {
        let tree = Tree::new("defaults-defaults");
        tree.app("share/applications/b.desktop")
            .app("share/applications/c.desktop")
            .write(
                "config/mimeapps.list",
                "[Added Associations]\nimage/png=c.desktop\n[Default Applications]\nimage/png=missing.desktop; b.desktop;\n",
            )
            .write("etc/mimeapps.list", "[Default Applications]\nimage/png=c.desktop\n");
        assert_eq!(
            tree.default_for(&["image/png"]).as_deref(),
            Some("share/applications/b.desktop")
        );
    }

    #[test]
    fn the_desktops_own_list_comes_first() {
        let tree = Tree::new("defaults-desktop");
        tree.app("share/applications/b.desktop")
            .app("share/applications/c.desktop")
            .write(
                "config/mimeapps.list",
                "[Default Applications]\nimage/png=b.desktop\n",
            )
            .write(
                "config/gnome-mimeapps.list",
                "[Default Applications]\nimage/png=c.desktop\n",
            );
        assert_eq!(
            tree.default_for(&["image/png"]).as_deref(),
            Some("share/applications/c.desktop")
        );
    }

    #[test]
    fn without_a_default_added_then_cached_apps_count_unless_removed() {
        let tree = Tree::new("defaults-added");
        tree.app("share/applications/added.desktop")
            .app("share/applications/cached.desktop")
            .app("share/applications/other.desktop")
            .write(
                "config/mimeapps.list",
                "[Removed Associations]\nimage/png=added.desktop;other.desktop\n",
            )
            .write(
                "etc/mimeapps.list",
                "[Added Associations]\nimage/png=added.desktop\n",
            )
            .write(
                "share/applications/mimeinfo.cache",
                "[MIME Cache]\nimage/png=other.desktop;cached.desktop;\n",
            );
        assert_eq!(
            tree.default_for(&["image/png"]).as_deref(),
            Some("share/applications/cached.desktop")
        );
        tree.write(
            "local/applications/mimeapps.list",
            "[Added Associations]\nimage/png=cached.desktop;\n",
        )
        .write("config/mimeapps.list", "");
        assert_eq!(
            tree.default_for(&["image/png"]).as_deref(),
            Some("share/applications/added.desktop")
        );
    }

    #[test]
    fn a_terminal_default_is_still_the_default() {
        let tree = Tree::new("defaults-terminal-default");
        tree.write(
            "share/applications/vim.desktop",
            "[Desktop Entry]\nType=Application\nExec=vim %f\nTerminal=true\n",
        )
        .app("share/applications/gui.desktop")
        .write(
            "config/mimeapps.list",
            "[Default Applications]\ntext/plain=vim.desktop\n",
        )
        .write(
            "share/applications/mimeinfo.cache",
            "[MIME Cache]\ntext/plain=gui.desktop;\n",
        );
        assert_eq!(
            tree.default_for(&["text/plain"]).as_deref(),
            Some("share/applications/vim.desktop")
        );
    }

    #[test]
    fn a_removed_app_is_no_lower_lists_default() {
        let tree = Tree::new("defaults-removed-default");
        tree.app("share/applications/a.desktop")
            .app("share/applications/b.desktop")
            .write(
                "config/mimeapps.list",
                "[Removed Associations]\nimage/png=a.desktop\n[Added Associations]\nimage/png=b.desktop\n",
            )
            .write(
                "etc/mimeapps.list",
                "[Default Applications]\nimage/png=a.desktop\n",
            );
        assert_eq!(
            tree.default_for(&["image/png"]).as_deref(),
            Some("share/applications/b.desktop")
        );
    }

    #[test]
    fn a_removal_only_hides_lower_lists() {
        let tree = Tree::new("defaults-removal");
        tree.app("share/applications/a.desktop")
            .write(
                "config/mimeapps.list",
                "[Added Associations]\nimage/png=a.desktop\n",
            )
            .write(
                "etc/mimeapps.list",
                "[Removed Associations]\nimage/png=a.desktop\n",
            );
        assert_eq!(
            tree.default_for(&["image/png"]).as_deref(),
            Some("share/applications/a.desktop")
        );
    }

    #[test]
    fn the_first_type_with_an_app_wins() {
        let tree = Tree::new("defaults-types");
        tree.app("share/applications/text.desktop").write(
            "config/mimeapps.list",
            "[Default Applications]\ntext/plain=text.desktop\n",
        );
        assert_eq!(
            tree.default_for(&["text/rust", "text/plain"]).as_deref(),
            Some("share/applications/text.desktop")
        );
        assert_eq!(tree.default_for(&["image/png"]), None);
        assert_eq!(tree.default_for(&[]), None);
    }

    #[test]
    fn a_higher_data_folder_hides_the_same_id_below() {
        let tree = Tree::new("defaults-shadow");
        tree.app("share/applications/a.desktop")
            .write(
                "local/applications/a.desktop",
                "[Desktop Entry]\nType=Application\nExec=a %f\nHidden=true\n",
            )
            .write(
                "config/mimeapps.list",
                "[Default Applications]\nimage/png=a.desktop\n",
            );
        assert_eq!(tree.default_for(&["image/png"]), None);
    }

    #[test]
    fn dashes_in_an_id_may_be_folders() {
        let tree = Tree::new("defaults-folders");
        tree.app("share/applications/kde/org-viewer.desktop")
            .app("share/applications/org-other.desktop")
            .app("share/applications/org/one.desktop")
            .write(
                "config/mimeapps.list",
                "[Default Applications]\nimage/png=kde-org-viewer.desktop\nimage/gif=org-other.desktop\nimage/bmp=org-one.desktop\n",
            );
        assert_eq!(
            tree.default_for(&["image/png"]).as_deref(),
            Some("share/applications/kde/org-viewer.desktop")
        );
        assert_eq!(
            tree.default_for(&["image/gif"]).as_deref(),
            Some("share/applications/org-other.desktop")
        );
        assert_eq!(
            tree.default_for(&["image/bmp"]).as_deref(),
            Some("share/applications/org/one.desktop")
        );
    }

    #[test]
    fn an_id_cannot_leave_the_applications_folder() {
        let tree = Tree::new("defaults-escape");
        tree.app("share/evil.desktop")
            .app("share/applications/sub/x.desktop")
            .app("share/applications/x.desktop").write(
            "config/mimeapps.list",
            "[Default Applications]\nimage/png=../evil.desktop\nimage/gif=sub/x.desktop\nimage/bmp=-x.desktop\n",
        );
        assert_eq!(tree.default_for(&["image/png"]), None);
        assert_eq!(tree.default_for(&["image/gif"]), None);
        assert_eq!(tree.default_for(&["image/bmp"]), None);
    }

    #[test]
    fn ids_come_from_the_named_group_and_key_only() {
        let text = "[Default Applications]\nimage/png = a.desktop ; ;b.desktop\nimage/pngx=c.desktop\n[Other]\nimage/gif=d.desktop\n";
        assert_eq!(ids(text, DEFAULTS, "image/png"), ["a.desktop", "b.desktop"]);
        assert_eq!(ids(text, DEFAULTS, "image/gif"), Vec::<&str>::new());
        assert_eq!(ids(text, "Other", "image/gif"), ["d.desktop"]);
        assert_eq!(
            ids("image/png=a.desktop\n", DEFAULTS, "image/png"),
            Vec::<&str>::new()
        );
    }
}
