use super::*;
use crate::{Details, FileId, Flags, Kind};

fn added(path: &str) -> Change {
    Change::Added {
        path: PathBuf::from(path),
        details: Details {
            kind: Kind::File,
            flags: Flags::default(),
            id: FileId {
                device: 0,
                inode: 0,
            },
            size: 0,
            modified: None,
            created: None,
        },
    }
}

fn removed(path: &str) -> Change {
    Change::Removed {
        path: PathBuf::from(path),
    }
}

fn moved(from: &str, to: &str) -> Change {
    Change::Moved {
        from: PathBuf::from(from),
        to: PathBuf::from(to),
    }
}

fn after(changes: &[Change]) -> Overlay {
    let mut overlay = Overlay::new();
    for change in changes {
        overlay.apply(change);
    }
    overlay
}

fn place(overlay: &Overlay, path: &str) -> Option<PathBuf> {
    overlay.place(PathBuf::from(path))
}

fn added_paths(overlay: &Overlay) -> Vec<&Path> {
    overlay.added().collect()
}

#[test]
fn starts_empty_and_leaves_paths_alone() {
    let overlay = Overlay::new();
    assert!(overlay.is_empty());
    assert_eq!(place(&overlay, "/h/a"), Some(PathBuf::from("/h/a")));
    assert_eq!(added_paths(&overlay), Vec::<&Path>::new());
}

#[test]
fn details_and_rescans_change_nothing() {
    let Change::Added { details, .. } = added("/h/a") else {
        unreachable!()
    };
    let overlay = after(&[
        Change::Changed {
            path: PathBuf::from("/h/a"),
            details,
        },
        Change::Rescan {
            folder: PathBuf::from("/h"),
        },
    ]);
    assert!(overlay.is_empty());
}

#[test]
fn an_added_entry_is_kept_by_path() {
    let overlay = after(&[added("/h/new")]);
    assert!(!overlay.is_empty());
    assert_eq!(added_paths(&overlay), [Path::new("/h/new")]);
}

#[test]
fn a_removal_takes_the_entry_and_everything_inside() {
    let overlay = after(&[added("/h/d/new"), added("/h/dx"), removed("/h/d")]);
    assert!(!overlay.is_empty());
    assert_eq!(place(&overlay, "/h/d"), None);
    assert_eq!(place(&overlay, "/h/d/old"), None);
    assert_eq!(place(&overlay, "/h/dx"), Some(PathBuf::from("/h/dx")));
    assert_eq!(added_paths(&overlay), [Path::new("/h/dx")]);
}

#[test]
fn a_move_carries_everything_inside() {
    let overlay = after(&[added("/h/d/new"), moved("/h/d", "/h/e/d")]);
    assert_eq!(place(&overlay, "/h/d"), Some(PathBuf::from("/h/e/d")));
    assert_eq!(
        place(&overlay, "/h/d/old"),
        Some(PathBuf::from("/h/e/d/old"))
    );
    assert_eq!(place(&overlay, "/h/dx"), Some(PathBuf::from("/h/dx")));
    assert_eq!(added_paths(&overlay), [Path::new("/h/e/d/new")]);
}

#[test]
fn a_renamed_entry_is_searched_under_its_new_name() {
    let overlay = after(&[moved("/h/d", "/h/e")]);
    // The row's name is out of date, so the entry joins the added ones.
    assert_eq!(place(&overlay, "/h/d"), None);
    assert_eq!(place(&overlay, "/h/d/old"), Some(PathBuf::from("/h/e/old")));
    assert_eq!(added_paths(&overlay), [Path::new("/h/e")]);
}

#[test]
fn a_rename_replaces_what_was_there() {
    let overlay = after(&[moved("/h/a", "/h/b")]);
    assert_eq!(place(&overlay, "/h/b"), None);
}

#[test]
fn steps_apply_in_order() {
    let overlay = after(&[moved("/h/d", "/h/e/d"), removed("/h/e"), added("/h/e")]);
    assert_eq!(place(&overlay, "/h/d/old"), None);
    assert_eq!(added_paths(&overlay), [Path::new("/h/e")]);
    let overlay = after(&[removed("/h/e"), moved("/h/d", "/h/e/d")]);
    assert_eq!(
        place(&overlay, "/h/d/old"),
        Some(PathBuf::from("/h/e/d/old"))
    );
}

#[test]
fn a_removal_takes_only_what_is_inside() {
    let overlay = after(&[
        added("/h/d/x"),
        added("/h/d x"),
        added("/h/d.txt"),
        added("/h/c"),
        removed("/h/d"),
    ]);
    assert_eq!(
        added_paths(&overlay),
        [
            Path::new("/h/c"),
            Path::new("/h/d x"),
            Path::new("/h/d.txt")
        ]
    );
}

#[test]
fn only_a_move_can_put_paths_elsewhere() {
    let overlay = after(&[added("/h/a"), removed("/h/b")]);
    assert!(!overlay.moves());
    let overlay = after(&[removed("/h/b"), moved("/h/c", "/h/d/c")]);
    assert!(overlay.moves());
}

#[test]
fn removals_between_moves_keep_their_place_in_order() {
    let mut changes: Vec<Change> = (0..1_000)
        .map(|n| removed(&format!("/h/gone/{n}")))
        .collect();
    changes.push(moved("/h/d", "/h/gone/7/d"));
    changes.push(removed("/h/e"));
    changes.push(moved("/h/f", "/h/e/f"));
    let overlay = after(&changes);
    assert_eq!(place(&overlay, "/h/gone/3/x"), None);
    // Moved in after its folder went: there again.
    assert_eq!(
        place(&overlay, "/h/d/x"),
        Some(PathBuf::from("/h/gone/7/d/x"))
    );
    assert_eq!(place(&overlay, "/h/e/old"), None);
    assert_eq!(place(&overlay, "/h/f/x"), Some(PathBuf::from("/h/e/f/x")));
    assert_eq!(place(&overlay, "/h/kept"), Some(PathBuf::from("/h/kept")));
}

#[test]
fn rebase_swaps_only_a_leading_folder() {
    let (from, to) = (Path::new("/h/d"), Path::new("/x"));
    assert_eq!(
        rebase(Path::new("/h/d"), from, to),
        Some(PathBuf::from("/x"))
    );
    assert_eq!(
        rebase(Path::new("/h/d/f"), from, to),
        Some(PathBuf::from("/x/f"))
    );
    assert_eq!(rebase(Path::new("/h/dd"), from, to), None);
}
