use std::ffi::OsString;

use nova_files_api::JobId;

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
fn the_root_goes_by_a_slash() {
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
        finished_note(&copy, &Outcome::Done, Some(here), Tally::default()).as_deref(),
        Some("Copied 2 items")
    );
    assert_eq!(
        finished_note(&copy, &Outcome::Done, None, Tally::default()).as_deref(),
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
            finished_note(&operation, &Outcome::Done, None, Tally::default()).as_deref(),
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
    assert_eq!(
        finished_note(&trash, &Outcome::Cancelled, None, Tally::default()),
        None
    );
    let failed = Outcome::Failed {
        path: PathBuf::from("/home/a/locked"),
        reason: Failure::PermissionDenied,
    };
    assert_eq!(
        finished_note(&trash, &failed, None, Tally::default()).as_deref(),
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
        finished_note(&new_folder, &read_only, None, Tally::default()).as_deref(),
        Some("Couldn't make New folder: the drive is read-only")
    );
    assert_eq!(
        finished_note(&Operation::EmptyTrash, &failed, None, Tally::default()).as_deref(),
        Some("Couldn't empty Trash: permission denied")
    );
}

#[test]
fn only_a_copy_or_move_says_an_item_cannot_be_copied() {
    let socket = Outcome::Failed {
        path: PathBuf::from("/home/a/socket"),
        reason: Failure::Unsupported,
    };
    let copy = Operation::Copy {
        sources: paths(&["socket"]),
        into: PathBuf::from("/home/b"),
    };
    let delete = Operation::DeleteFromTrash {
        items: paths(&["socket"]),
    };
    assert_eq!(
        finished_note(&copy, &socket, None, Tally::default()).as_deref(),
        Some("Couldn't copy socket: it can't be copied")
    );
    assert_eq!(
        finished_note(&delete, &socket, None, Tally::default()).as_deref(),
        Some("Couldn't delete socket: Nova Files can't do that with it")
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
        Failure::NoRecord,
        Failure::Other,
    ];
    let words: std::collections::HashSet<_> = reasons.into_iter().map(failure).collect();
    assert_eq!(words.len(), reasons.len());
}

#[test]
fn a_path_under_home_reads_from_the_tilde() {
    let home = Path::new("/home/a");
    assert_eq!(short_path(Path::new("/home/a"), home), "~");
    assert_eq!(
        short_path(Path::new("/home/a/Documents/x"), home),
        "~/Documents/x"
    );
    assert_eq!(short_path(Path::new("/home/ab"), home), "/home/ab");
    assert_eq!(short_path(Path::new("/srv"), home), "/srv");
}

#[test]
fn a_listing_counts_its_folders_and_files() {
    use nova_files_api::EntryId;
    let entry = |id, name: &str, kind| Entry {
        id: EntryId(id),
        name: OsString::from(name),
        kind,
    };
    let entries = [
        entry(0, "a", EntryKind::Folder),
        entry(1, "b", EntryKind::File),
        entry(2, "c", EntryKind::File),
    ];
    assert_eq!(count_of(&entries), "1 folder, 2 files");
}

#[test]
fn a_note_says_what_the_answers_to_clashes_did() {
    let into = PathBuf::from("/home/a/Documents");
    let copy = Operation::Copy {
        sources: paths(&["a", "b", "c", "d", "e"]),
        into: into.clone(),
    };
    let moved = Operation::Move {
        sources: paths(&["a"]),
        into: into.clone(),
    };
    let tally = |replaced, skipped| Tally { replaced, skipped };
    let note = |operation: &Operation, here: Option<&Path>, answered| {
        finished_note(operation, &Outcome::Done, here, answered)
    };

    assert_eq!(
        note(&copy, Some(&into), tally(2, 1)).as_deref(),
        Some("Copied 4 items, replaced 2, skipped 1")
    );
    assert_eq!(
        note(&copy, None, tally(3, 0)).as_deref(),
        Some("Copied 5 items to Documents, replaced 3")
    );
    assert_eq!(
        note(&moved, None, tally(1, 0)).as_deref(),
        Some("Moved a to Documents, replaced 1")
    );
    assert_eq!(
        note(&moved, None, tally(0, 1)).as_deref(),
        Some("Nothing moved, skipped 1")
    );
    assert_eq!(
        note(&copy, None, tally(0, 5)).as_deref(),
        Some("Nothing copied, skipped 5")
    );
    let trash = Operation::Trash {
        paths: paths(&["a"]),
    };
    assert_eq!(
        note(&trash, None, tally(0, 1)).as_deref(),
        Some("Moved a to Trash")
    );
    let extract = Operation::Extract {
        archive: "/home/a/photos.zip".into(),
        into,
        to: nova_files_api::ExtractTo::Here,
    };
    assert_eq!(
        note(&extract, None, tally(0, 0)).as_deref(),
        Some("Extracted photos.zip")
    );
    assert_eq!(
        note(&extract, None, tally(0, 1)).as_deref(),
        Some("Extracted photos.zip, skipped 1")
    );
    assert_eq!(
        note(&extract, None, tally(1, 0)).as_deref(),
        Some("Extracted photos.zip, replaced 1")
    );
}
