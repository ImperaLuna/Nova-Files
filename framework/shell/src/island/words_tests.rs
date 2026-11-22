use std::ffi::OsString;

use nova_files_api::JobId;

use crate::island::test_jobs::{fetch, sweep, words};

use super::*;

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
    let notes = [
        (sweep(&["a"]), Some("Swept")),
        (fetch(&["a"], "/b"), None),
        (Operation::feature(7_u8), None),
        (Operation::Undo { job: JobId(1) }, None),
    ];
    for (operation, note) in notes {
        assert_eq!(
            finished_note(&operation, &Outcome::Done, None, Tally::default(), &words()).as_deref(),
            note,
            "{operation:?}"
        );
    }
}

#[test]
fn a_cancel_says_nothing_and_a_failure_says_why() {
    let trash = sweep(&["a"]);
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
        finished_note(&trash, &failed, None, Tally::default(), &words()).as_deref(),
        Some("Couldn't sweep locked: permission denied")
    );
    let new_folder = fetch(&["New folder"], "/mnt/cd");
    let read_only = Outcome::Failed {
        path: PathBuf::from("/mnt/cd/New folder"),
        reason: Failure::ReadOnly,
    };
    assert_eq!(
        finished_note(&new_folder, &read_only, None, Tally::default(), &words()).as_deref(),
        Some("Couldn't fetch New folder: the drive is read-only")
    );
    assert_eq!(
        finished_note(&sweep(&[]), &failed, None, Tally::default(), &words()).as_deref(),
        Some("Couldn't sweep: permission denied")
    );
}

#[test]
fn a_feature_may_say_why_in_its_own_words() {
    let socket = Outcome::Failed {
        path: PathBuf::from("/home/a/socket"),
        reason: Failure::Unsupported,
    };
    let fetching = fetch(&["socket"], "/b");
    let sweeping = sweep(&["socket"]);
    assert_eq!(
        finished_note(&fetching, &socket, None, Tally::default(), &words()).as_deref(),
        Some("Couldn't fetch socket: it can't be copied")
    );
    assert_eq!(
        finished_note(&sweeping, &socket, None, Tally::default(), &words()).as_deref(),
        Some("Couldn't sweep socket: Nova Files can't do that with it")
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
