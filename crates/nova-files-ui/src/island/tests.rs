use std::ffi::OsString;

use super::*;

fn paths(names: &[&str]) -> Vec<PathBuf> {
    names
        .iter()
        .map(|name| Path::new("/home/a").join(name))
        .collect()
}

#[test]
fn counts_say_one_or_many() {
    assert_eq!(count(0, ITEM, ITEMS), "0 items");
    assert_eq!(count(1, ITEM, ITEMS), "1 item");
    assert_eq!(count(2, ITEM, ITEMS), "2 items");
}

#[test]
fn the_quiet_count_names_folders_then_files_and_leaves_out_none() {
    assert_eq!(quiet_count(4, 9), "4 folders, 9 files");
    assert_eq!(quiet_count(1, 0), "1 folder");
    assert_eq!(quiet_count(0, 1), "1 file");
    assert_eq!(quiet_count(0, 0), "");
}

#[test]
fn the_selection_line_adds_the_size_only_when_there_is_one() {
    assert_eq!(
        selection_line(3, Some(12_000_000)),
        ("3".into(), " selected · 12 MB".into())
    );
    assert_eq!(selection_line(1, Some(0)), ("1".into(), " selected".into()));
    assert_eq!(selection_line(2, None), ("2".into(), " selected".into()));
}

#[test]
fn a_lone_path_goes_by_its_name_and_the_root_by_a_slash() {
    assert_eq!(what(&paths(&["notes.txt"])), "notes.txt");
    assert_eq!(what(&paths(&["a", "b", "c"])), "3 items");
    assert_eq!(name_of(Path::new("/")), "/");
}

#[test]
fn a_copy_names_where_it_went_only_when_that_is_not_on_screen() {
    let here = Path::new("/home/a/Projects");
    let copy = Operation::Copy {
        sources: paths(&["x", "y"]),
        into: here.to_path_buf(),
    };
    assert_eq!(
        finished_note(&copy, &Outcome::Done, Some(here)).as_deref(),
        Some("Copied 2 items")
    );
    assert_eq!(
        finished_note(&copy, &Outcome::Done, None).as_deref(),
        Some("Copied 2 items to Projects")
    );
}

#[test]
fn each_done_job_has_its_note() {
    let into = PathBuf::from("/home/a/Music");
    let notes = [
        (
            Operation::Move {
                sources: paths(&["song.ogg"]),
                into: into.clone(),
            },
            Some("Moved song.ogg to Music"),
        ),
        (
            Operation::Trash {
                paths: paths(&["a", "b", "c"]),
            },
            Some("Moved 3 items to Trash"),
        ),
        (
            Operation::Restore {
                items: paths(&["a"]),
            },
            Some("Restored 1 item"),
        ),
        (
            Operation::Delete {
                paths: paths(&["old"]),
            },
            Some("Deleted old"),
        ),
        (
            Operation::DeleteFromTrash {
                items: paths(&["a", "b"]),
            },
            Some("Deleted 2 items"),
        ),
        (Operation::EmptyTrash, Some("Trash emptied")),
        (
            Operation::Rename {
                path: into.clone(),
                name: OsString::from("Songs"),
            },
            Some("Renamed to Songs"),
        ),
        (
            Operation::NewFolder {
                inside: into,
                name: OsString::from("New folder"),
            },
            None,
        ),
        (Operation::Undo { job: JobId(1) }, None),
    ];
    for (operation, note) in notes {
        assert_eq!(
            finished_note(&operation, &Outcome::Done, None).as_deref(),
            note,
            "{operation:?}"
        );
    }
}

#[test]
fn a_cancel_says_nothing_and_a_failure_says_why() {
    let trash = Operation::Trash {
        paths: paths(&["a"]),
    };
    assert_eq!(finished_note(&trash, &Outcome::Cancelled, None), None);
    let failed = Outcome::Failed {
        path: PathBuf::from("/home/a/locked"),
        reason: Failure::PermissionDenied,
    };
    assert_eq!(
        finished_note(&trash, &failed, None).as_deref(),
        Some("Couldn't move to Trash locked: permission denied")
    );
    let new_folder = Operation::NewFolder {
        inside: PathBuf::from("/mnt/cd"),
        name: OsString::from("New folder"),
    };
    let read_only = Outcome::Failed {
        path: PathBuf::from("/mnt/cd/New folder"),
        reason: Failure::ReadOnly,
    };
    assert_eq!(
        finished_note(&new_folder, &read_only, None).as_deref(),
        Some("Couldn't make New folder: the drive is read-only")
    );
    assert_eq!(
        finished_note(&Operation::EmptyTrash, &failed, None).as_deref(),
        Some("Couldn't empty Trash: permission denied")
    );
}

#[test]
fn every_failure_reads_differently() {
    let reasons = [
        Failure::NameTaken,
        Failure::InvalidName,
        Failure::NotFound,
        Failure::PermissionDenied,
        Failure::NoSpace,
        Failure::ReadOnly,
        Failure::IntoItself,
        Failure::Unsupported,
        Failure::SourceChanged,
        Failure::Other,
    ];
    let words: std::collections::HashSet<_> = reasons.into_iter().map(failure).collect();
    assert_eq!(words.len(), reasons.len());
}
