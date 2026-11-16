//! What dropping files does, as other file managers do it: a drop on a
//! folder moves files on that folder's drive and copies files from
//! another (the core looks at the drives, see `Command::DropFiles`); Ctrl
//! held always copies, Shift held always moves.
//!
//! A drop that would change nothing, or can't be done, isn't offered: a
//! folder onto itself or into a folder inside it, files onto the folder
//! they are already in, and anything dropped out of the trash or into its
//! folder of trashed entries, which only Restore changes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::Modifiers;
use nova_files_api::Operation;

/// Where files can be dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropAt {
    /// Into a folder.
    Folder(PathBuf),
}

/// What a drop asks the core for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dropped {
    /// A job of its own: a copy or move the keys held chose.
    Job(Operation),
    /// A copy or move the core picks by drive.
    ByDrive {
        sources: Vec<PathBuf>,
        into: PathBuf,
    },
}

/// Files being dragged, sorted for asking where they may drop: as many
/// targets ask on every frame of a drag, each asks in the time it takes
/// to walk up its own path, however many files are dragged.
#[derive(Debug, Default)]
pub struct Sources {
    pub paths: Vec<PathBuf>,
    /// `paths`, to look up.
    each: HashSet<PathBuf>,
    /// The folders they are in.
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

/// Whether `sources` may be dropped at `at`, where `trash` is the folder
/// of the trash's entries.
pub fn allowed(at: &DropAt, sources: &Sources, trash: &Path) -> bool {
    if sources.paths.is_empty()
        || sources
            .folders
            .iter()
            .any(|folder| folder.starts_with(trash))
    {
        return false;
    }
    let DropAt::Folder(into) = at;
    !into.starts_with(trash)
        && !sources.folders.contains(into)
        && !into.ancestors().any(|folder| sources.each.contains(folder))
}

/// What dropping `sources` at `at` with `modifiers` held does.
pub fn dropped(at: DropAt, sources: Vec<PathBuf>, modifiers: Modifiers) -> Dropped {
    let DropAt::Folder(into) = at;
    if modifiers.control {
        Dropped::Job(Operation::Copy { sources, into })
    } else if modifiers.shift {
        Dropped::Job(Operation::Move { sources, into })
    } else {
        Dropped::ByDrive { sources, into }
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
        let trash = Path::new(TRASH);
        assert!(allowed(&folder("/b"), &paths(&["/a/x", "/a/y"]), trash));
        assert!(allowed(&folder("/a/x2"), &paths(&["/a/x"]), trash));
    }

    #[test]
    fn a_drop_that_changes_nothing_isnt_offered() {
        let trash = Path::new(TRASH);
        // Nothing dragged.
        assert!(!allowed(&folder("/b"), &Sources::default(), trash));
        // Where they already are.
        assert!(!allowed(&folder("/a"), &paths(&["/b/y", "/a/x"]), trash));
        // A folder onto itself, or into what it holds.
        assert!(!allowed(&folder("/a/x"), &paths(&["/a/x"]), trash));
        assert!(!allowed(&folder("/a/x/in"), &paths(&["/a/x"]), trash));
    }

    #[test]
    fn nothing_goes_in_or_out_of_the_trash_by_dragging() {
        let trash = Path::new(TRASH);
        let trashed = paths(&[&format!("{TRASH}/x")]);
        assert!(!allowed(&folder("/b"), &trashed, trash));
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
        // Ctrl wins when both are held.
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
            Dropped::ByDrive { sources, into }
        );
    }
}
