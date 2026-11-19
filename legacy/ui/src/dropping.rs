use std::collections::HashSet;
use std::path::{Path, PathBuf};

use nova_files_api::Operation;
use nova_files_gathered_api::Gathering;
use nova_files_shell::PageRef;

pub const MOVE_TO: &str = "Move to";
pub const ADD_TO: &str = "Add to";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropAt {
    Folder(PathBuf),
    Trash,
    Gathered(Gathering),
    Page(PageRef),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dropped {
    Job(Operation),
    Gather {
        place: Gathering,
        folders: Vec<PathBuf>,
    },
    Page {
        page: PageRef,
        paths: Vec<PathBuf>,
    },
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

pub fn shell_place(at: &DropAt) -> Option<nova_files_shell::Location> {
    match at {
        DropAt::Folder(folder) => Some(nova_files_shell::Location::Folder(folder.clone())),
        DropAt::Page(page) => Some(nova_files_shell::Location::Page(page.clone())),
        DropAt::Trash | DropAt::Gathered(_) => None,
    }
}

pub fn allowed(at: &DropAt, sources: &Sources, trashed: impl Fn(&Path) -> bool) -> bool {
    if sources.paths.is_empty() || sources.folders.iter().any(|folder| trashed(folder)) {
        return false;
    }
    match at {
        DropAt::Trash | DropAt::Gathered(_) | DropAt::Page(_) => true,
        DropAt::Folder(into) => {
            !trashed(into)
                && !sources.folders.contains(into)
                && !into.ancestors().any(|folder| sources.each.contains(folder))
        }
    }
}

pub fn dropped(at: DropAt, sources: Vec<PathBuf>) -> Option<Dropped> {
    Some(match at {
        DropAt::Trash => Dropped::Job(Operation::Trash { paths: sources }),
        DropAt::Gathered(place) => Dropped::Gather {
            place,
            folders: sources,
        },
        DropAt::Page(page) => Dropped::Page {
            page,
            paths: sources,
        },
        DropAt::Folder(_) => return None,
    })
}

pub fn label(at: &DropAt, name: &str) -> Option<String> {
    match at {
        DropAt::Gathered(_) | DropAt::Page(_) => Some(format!("{ADD_TO} {name}")),
        DropAt::Trash => Some(format!("{MOVE_TO} {name}")),
        DropAt::Folder(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(allowed(&DropAt::Trash, &paths(&["/a/x"]), trash));
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
        assert!(!allowed(&DropAt::Trash, &trashed, trash));
        assert!(!allowed(&folder(TRASH), &paths(&["/a/x"]), trash));
    }

    #[test]
    fn the_window_drops_only_on_the_trash_places_and_pages() {
        let sources = vec![PathBuf::from("/a/x")];
        assert_eq!(dropped(folder("/b"), sources.clone()), None);
        assert_eq!(label(&folder("/b"), "Backup"), None);
        assert_eq!(
            dropped(DropAt::Trash, sources.clone()),
            Some(Dropped::Job(Operation::Trash { paths: sources }))
        );
        assert_eq!(
            label(&DropAt::Trash, "Trash").as_deref(),
            Some("Move to Trash")
        );
    }

    #[test]
    fn folders_dropped_on_a_place_are_added_whatever_the_keys() {
        let at = DropAt::Gathered(Gathering::Games);
        let trash = |folder: &Path| folder.starts_with(TRASH);
        assert!(allowed(&at, &paths(&["/mnt/lib"]), trash));
        assert!(!allowed(&at, &paths(&[&format!("{TRASH}/old")]), trash));
        assert_eq!(
            dropped(at.clone(), vec![PathBuf::from("/mnt/lib")]),
            Some(Dropped::Gather {
                place: Gathering::Games,
                folders: vec![PathBuf::from("/mnt/lib")],
            })
        );
        assert_eq!(label(&at, "Games").as_deref(), Some("Add to Games"));
    }
}
