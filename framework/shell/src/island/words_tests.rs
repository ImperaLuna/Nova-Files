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
fn each_done_job_has_its_note() {
    let into = PathBuf::from("/home/a/Music");
    let notes = [
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
            finished_note(
                &operation,
                &Outcome::Done,
                None,
                Tally::default(),
                &Words::default()
            )
            .as_deref(),
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
        finished_note(
            &trash,
            &Outcome::Cancelled,
            None,
            Tally::default(),
            &Words::default()
        ),
        None
    );
    let failed = Outcome::Failed {
        path: PathBuf::from("/home/a/locked"),
        reason: Failure::PermissionDenied,
    };
    assert_eq!(
        finished_note(&trash, &failed, None, Tally::default(), &Words::default()).as_deref(),
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
        finished_note(
            &new_folder,
            &read_only,
            None,
            Tally::default(),
            &Words::default()
        )
        .as_deref(),
        Some("Couldn't make New folder: the drive is read-only")
    );
    assert_eq!(
        finished_note(
            &Operation::EmptyTrash,
            &failed,
            None,
            Tally::default(),
            &Words::default()
        )
        .as_deref(),
        Some("Couldn't empty Trash: permission denied")
    );
}

#[test]
fn a_feature_may_say_why_in_its_own_words() {
    let socket = Outcome::Failed {
        path: PathBuf::from("/home/a/socket"),
        reason: Failure::Unsupported,
    };
    let fetching = crate::island::test_jobs::fetch(&["socket"], "/b");
    let delete = Operation::DeleteFromTrash {
        items: paths(&["socket"]),
    };
    assert_eq!(
        finished_note(
            &fetching,
            &socket,
            None,
            Tally::default(),
            &crate::island::test_jobs::words()
        )
        .as_deref(),
        Some("Couldn't fetch socket: it can't be copied")
    );
    assert_eq!(
        finished_note(&delete, &socket, None, Tally::default(), &Words::default()).as_deref(),
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
fn a_trash_note_leaves_out_the_answers() {
    let trash = Operation::Trash {
        paths: paths(&["a"]),
    };
    let skipped = Tally {
        replaced: 0,
        skipped: 1,
    };
    assert_eq!(
        finished_note(&trash, &Outcome::Done, None, skipped, &Words::default()).as_deref(),
        Some("Moved a to Trash")
    );
}
