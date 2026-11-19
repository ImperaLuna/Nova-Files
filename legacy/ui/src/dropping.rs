use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::Modifiers;
use nova_files_api::{Gathering, Operation};
use nova_files_transfer_api::DropKind;

pub const MOVE_TO: &str = "Move to";
pub const COPY_TO: &str = "Copy to";
pub const ADD_TO: &str = "Add to";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropAt {
    Folder(PathBuf),
    Trash,
    Gathered(Gathering),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dropped {
    Job(Operation),
    Gather {
        place: Gathering,
        folders: Vec<PathBuf>,
    },
    ByDrive {
        sources: Vec<PathBuf>,
        into: PathBuf,
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

pub fn allowed(at: &DropAt, sources: &Sources, trashed: impl Fn(&Path) -> bool) -> bool {
    if sources.paths.is_empty() || sources.folders.iter().any(|folder| trashed(folder)) {
        return false;
    }
    match at {
        DropAt::Trash | DropAt::Gathered(_) => true,
        DropAt::Folder(into) => {
            !trashed(into)
                && !sources.folders.contains(into)
                && !into.ancestors().any(|folder| sources.each.contains(folder))
        }
    }
}

pub fn dropped(at: DropAt, sources: Vec<PathBuf>, modifiers: Modifiers) -> Dropped {
    let into = match at {
        DropAt::Trash => return Dropped::Job(Operation::Trash { paths: sources }),
        DropAt::Gathered(place) => {
            return Dropped::Gather {
                place,
                folders: sources,
            };
        }
        DropAt::Folder(into) => into,
    };
    if modifiers.control {
        Dropped::Job(Operation::Copy { sources, into })
    } else if modifiers.shift {
        Dropped::Job(Operation::Move { sources, into })
    } else {
        Dropped::ByDrive { sources, into }
    }
}

pub fn label(
    at: &DropAt,
    name: &str,
    by_drive: Option<DropKind>,
    modifiers: Modifiers,
) -> Option<String> {
    let kind = match at {
        DropAt::Gathered(_) => return Some(format!("{ADD_TO} {name}")),
        DropAt::Trash => DropKind::Move,
        DropAt::Folder(_) if modifiers.control => DropKind::Copy,
        DropAt::Folder(_) if modifiers.shift => DropKind::Move,
        DropAt::Folder(_) => by_drive?,
    };
    let verb = match kind {
        DropKind::Move => MOVE_TO,
        DropKind::Copy => COPY_TO,
    };
    Some(format!("{verb} {name}"))
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
    fn keys_held_pick_copy_or_move_and_otherwise_the_drive_does() {
        let (sources, into) = (vec![PathBuf::from("/a/x")], PathBuf::from("/b"));
        let at = || DropAt::Folder(into.clone());
        let ctrl = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        assert_eq!(
            dropped(at(), sources.clone(), ctrl),
            Dropped::Job(Operation::Copy {
                sources: sources.clone(),
                into: into.clone()
            })
        );
        let both = Modifiers {
            shift: true,
            ..ctrl
        };
        assert!(matches!(
            dropped(at(), sources.clone(), both),
            Dropped::Job(Operation::Copy { .. })
        ));
        assert_eq!(
            dropped(at(), sources.clone(), shift),
            Dropped::Job(Operation::Move {
                sources: sources.clone(),
                into: into.clone()
            })
        );
        assert_eq!(
            dropped(at(), sources.clone(), Modifiers::default()),
            Dropped::ByDrive {
                sources: sources.clone(),
                into
            }
        );
        assert_eq!(
            dropped(DropAt::Trash, sources.clone(), shift),
            Dropped::Job(Operation::Trash { paths: sources })
        );
    }

    #[test]
    fn the_chip_names_the_drop_as_the_keys_or_the_drive_pick_it() {
        let at = folder("/b");
        let none = Modifiers::default();
        let ctrl = Modifiers {
            control: true,
            ..none
        };
        let shift = Modifiers {
            shift: true,
            ..none
        };
        let says = |by_drive, modifiers| label(&at, "Backup", by_drive, modifiers);
        assert_eq!(
            says(Some(DropKind::Move), none).as_deref(),
            Some("Move to Backup")
        );
        assert_eq!(
            says(Some(DropKind::Copy), none).as_deref(),
            Some("Copy to Backup")
        );
        assert_eq!(says(None, none), None);
        assert_eq!(
            says(Some(DropKind::Move), ctrl).as_deref(),
            Some("Copy to Backup")
        );
        assert_eq!(says(None, shift).as_deref(), Some("Move to Backup"));
        let both = Modifiers {
            shift: true,
            ..ctrl
        };
        assert_eq!(says(None, both).as_deref(), Some("Copy to Backup"));
        assert_eq!(
            label(&DropAt::Trash, "Trash", None, ctrl).as_deref(),
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
            dropped(
                at.clone(),
                vec![PathBuf::from("/mnt/lib")],
                Modifiers::control()
            ),
            Dropped::Gather {
                place: Gathering::Games,
                folders: vec![PathBuf::from("/mnt/lib")],
            }
        );
        assert_eq!(
            label(&at, "Games", None, Modifiers::shift()).as_deref(),
            Some("Add to Games")
        );
    }
}
