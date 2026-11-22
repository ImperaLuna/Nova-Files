use std::collections::HashSet;
use std::path::{Path, PathBuf};

use nova_files_shell::PageRef;

pub const ADD_TO: &str = "Add to";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropAt {
    Folder(PathBuf),
    Trash(PathBuf),
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
        DropAt::Folder(folder) | DropAt::Trash(folder) => {
            nova_files_shell::Location::Folder(folder.clone())
        }
        DropAt::Page(page) => nova_files_shell::Location::Page(page.clone()),
    }
}

pub fn allowed(at: &DropAt, sources: &Sources, trashed: impl Fn(&Path) -> bool) -> bool {
    if sources.paths.is_empty() || sources.folders.iter().any(|folder| trashed(folder)) {
        return false;
    }
    match at {
        DropAt::Trash(_) | DropAt::Page(_) => true,
        DropAt::Folder(into) => {
            !trashed(into)
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
        DropAt::Folder(_) | DropAt::Trash(_) => return None,
    })
}

pub fn label(at: &DropAt, name: &str) -> Option<String> {
    match at {
        DropAt::Page(_) => Some(format!("{ADD_TO} {name}")),
        DropAt::Folder(_) | DropAt::Trash(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use gpui::SharedString;
    use nova_files_shell::PageKind;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Shelf;

    impl PageKind for Shelf {
        fn title(&self) -> SharedString {
            "Games".into()
        }
    }

    const TRASH: &str = "/home/u/.local/share/Trash/files";

    fn paths(list: &[&str]) -> Sources {
        Sources::new(list.iter().map(PathBuf::from).collect())
    }

    fn folder(path: &str) -> DropAt {
        DropAt::Folder(PathBuf::from(path))
    }

    #[test]
    fn files_drop_into_any_other_folder() {
        let trash = |folder: &Path| folder.starts_with(TRASH);
        assert!(allowed(&folder("/b"), &paths(&["/a/x", "/a/y"]), trash));
        assert!(allowed(&folder("/a/x2"), &paths(&["/a/x"]), trash));
        assert!(allowed(
            &DropAt::Trash(TRASH.into()),
            &paths(&["/a/x"]),
            trash
        ));
    }

    #[test]
    fn a_drop_that_changes_nothing_isnt_offered() {
        let trash = |folder: &Path| folder.starts_with(TRASH);
        assert!(!allowed(&folder("/b"), &Sources::default(), trash));
        assert!(!allowed(&folder("/a"), &paths(&["/b/y", "/a/x"]), trash));
        assert!(!allowed(&folder("/a/x"), &paths(&["/a/x"]), trash));
        assert!(!allowed(&folder("/a/x/in"), &paths(&["/a/x"]), trash));
    }

    #[test]
    fn nothing_goes_in_or_out_of_the_trash_by_dragging() {
        let trash = |folder: &Path| folder.starts_with(TRASH);
        let trashed = paths(&[&format!("{TRASH}/x")]);
        assert!(!allowed(&folder("/b"), &trashed, trash));
        assert!(!allowed(&DropAt::Trash(TRASH.into()), &trashed, trash));
        assert!(!allowed(&folder(TRASH), &paths(&["/a/x"]), trash));
    }

    #[test]
    fn the_window_drops_only_on_places_and_leaves_folders_to_the_features() {
        let sources = vec![PathBuf::from("/a/x")];
        let trash = DropAt::Trash(TRASH.into());
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
        let trash = |folder: &Path| folder.starts_with(TRASH);
        assert!(allowed(&at, &paths(&["/mnt/lib"]), trash));
        assert!(!allowed(&at, &paths(&[&format!("{TRASH}/old")]), trash));
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
