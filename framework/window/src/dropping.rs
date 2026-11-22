use std::collections::HashSet;
use std::path::{Path, PathBuf};

use nova_files_shell::{Allow, PageRef, RulesBook};

pub const ADD_TO: &str = "Add to";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropAt {
    Folder(PathBuf),
    Page(PageRef),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dropped {
    Page { page: PageRef, paths: Vec<PathBuf> },
}

#[derive(Debug, Default)]
pub struct Sources {
    pub paths: Vec<PathBuf>,
    each: HashSet<PathBuf>,
    folders: HashSet<PathBuf>,
}

impl Sources {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        let each = paths.iter().cloned().collect();
        let folders = paths
            .iter()
            .filter_map(|path| path.parent().map(Path::to_path_buf))
            .collect();
        Self {
            paths,
            each,
            folders,
        }
    }
}

pub fn shell_place(at: &DropAt) -> nova_files_shell::Location {
    match at {
        DropAt::Folder(folder) => nova_files_shell::Location::Folder(folder.clone()),
        DropAt::Page(page) => nova_files_shell::Location::Page(page.clone()),
    }
}

pub fn allowed(at: &DropAt, sources: &Sources, rules: &RulesBook) -> bool {
    let taken_out = |folder: &PathBuf| rules.for_folder(folder).allows(Allow::TakeOut);
    if sources.paths.is_empty() || !sources.folders.iter().all(taken_out) {
        return false;
    }
    match at {
        DropAt::Page(_) => true,
        DropAt::Folder(into) => {
            (rules.takes_drops(into) || rules.for_folder(into).allows(Allow::Change))
                && !sources.folders.contains(into)
                && !into.ancestors().any(|folder| sources.each.contains(folder))
        }
    }
}

pub fn dropped(at: DropAt, sources: Vec<PathBuf>) -> Option<Dropped> {
    Some(match at {
        DropAt::Page(page) => Dropped::Page {
            page,
            paths: sources,
        },
        DropAt::Folder(_) => return None,
    })
}

pub fn label(at: &DropAt, name: &str) -> Option<String> {
    match at {
        DropAt::Page(_) => Some(format!("{ADD_TO} {name}")),
        DropAt::Folder(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use gpui::SharedString;
    use nova_files_shell::{FolderRules, PageKind};

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Shelf;

    impl PageKind for Shelf {
        fn title(&self) -> SharedString {
            "Games".into()
        }
    }

    const TRASH: &str = "/home/u/.local/share/Trash/files";
    const ARCHIVE: &str = "/a/bundle.zip";

    fn paths(list: &[&str]) -> Sources {
        Sources::new(list.iter().map(PathBuf::from).collect())
    }

    fn folder(path: &str) -> DropAt {
        DropAt::Folder(PathBuf::from(path))
    }

    fn rules() -> RulesBook {
        let rules = RulesBook::default();
        rules.claim("trash", TRASH.into(), FolderRules::allowing(&[]));
        rules.take_drops("trash", TRASH.into());
        let inside = FolderRules::allowing(&[Allow::TakeOut, Allow::Restyle]);
        rules.claim("archives", ARCHIVE.into(), inside);
        rules
    }

    #[test]
    fn files_drop_into_any_other_folder() {
        let rules = rules();
        assert!(allowed(&folder("/b"), &paths(&["/a/x", "/a/y"]), &rules));
        assert!(allowed(&folder("/a/x2"), &paths(&["/a/x"]), &rules));
        assert!(allowed(&folder(TRASH), &paths(&["/a/x"]), &rules));
    }

    #[test]
    fn a_drop_that_changes_nothing_isnt_offered() {
        let rules = rules();
        assert!(!allowed(&folder("/b"), &Sources::default(), &rules));
        assert!(!allowed(&folder("/a"), &paths(&["/b/y", "/a/x"]), &rules));
        assert!(!allowed(&folder("/a/x"), &paths(&["/a/x"]), &rules));
        assert!(!allowed(&folder("/a/x/in"), &paths(&["/a/x"]), &rules));
    }

    #[test]
    fn nothing_goes_in_or_out_of_the_trash_by_dragging() {
        let rules = rules();
        let trashed = paths(&[&format!("{TRASH}/x")]);
        assert!(!allowed(&folder("/b"), &trashed, &rules));
        assert!(!allowed(&folder(TRASH), &trashed, &rules));
        assert!(!allowed(
            &folder(&format!("{TRASH}/x2")),
            &paths(&["/a/x"]),
            &rules
        ));
    }

    #[test]
    fn nothing_drops_into_an_opened_archive() {
        let rules = rules();
        assert!(!allowed(&folder(ARCHIVE), &paths(&["/b/x"]), &rules));
        assert!(!allowed(
            &folder(&format!("{ARCHIVE}/docs")),
            &paths(&["/b/x"]),
            &rules
        ));
        assert!(allowed(
            &folder("/b"),
            &paths(&[&format!("{ARCHIVE}/x")]),
            &rules
        ));
    }

    #[test]
    fn the_window_drops_only_on_places_and_leaves_folders_to_the_features() {
        let sources = vec![PathBuf::from("/a/x")];
        let trash = folder(TRASH);
        assert_eq!(dropped(folder("/b"), sources.clone()), None);
        assert_eq!(label(&folder("/b"), "Backup"), None);
        assert_eq!(dropped(trash.clone(), sources), None);
        assert_eq!(label(&trash, "Trash"), None);
        assert_eq!(
            shell_place(&trash),
            nova_files_shell::Location::Folder(TRASH.into())
        );
    }

    #[test]
    fn folders_dropped_on_a_place_are_added_whatever_the_keys() {
        let page = PageRef::new(Shelf);
        let at = DropAt::Page(page.clone());
        let rules = rules();
        assert!(allowed(&at, &paths(&["/mnt/lib"]), &rules));
        assert!(!allowed(&at, &paths(&[&format!("{TRASH}/old")]), &rules));
        assert_eq!(
            dropped(at.clone(), vec![PathBuf::from("/mnt/lib")]),
            Some(Dropped::Page {
                page,
                paths: vec![PathBuf::from("/mnt/lib")],
            })
        );
        assert_eq!(label(&at, "Games").as_deref(), Some("Add to Games"));
    }
}
