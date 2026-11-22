use super::*;

fn paths(names: &[&str]) -> Vec<PathBuf> {
    names
        .iter()
        .map(|name| Path::new("/home/a").join(name))
        .collect()
}

fn tally(replaced: u64, skipped: u64) -> Tally {
    Tally { replaced, skipped }
}

#[test]
fn a_copy_names_where_it_went_only_when_that_is_not_on_screen() {
    let here = Path::new("/home/a/Projects");
    let copy = CopyJob {
        sources: paths(&["x", "y"]),
        into: here.to_path_buf(),
    };
    assert_eq!(
        CopyWords
            .done(&copy, Some(here), Tally::default())
            .as_deref(),
        Some("Copied 2 items")
    );
    assert_eq!(
        CopyWords.done(&copy, None, Tally::default()).as_deref(),
        Some("Copied 2 items to Projects")
    );
}

#[test]
fn a_move_always_names_where_it_went() {
    let into = PathBuf::from("/home/a/Music");
    let moved = MoveJob {
        sources: paths(&["song.ogg"]),
        into: into.clone(),
    };
    assert_eq!(
        MoveWords
            .done(&moved, Some(&into), Tally::default())
            .as_deref(),
        Some("Moved song.ogg to Music")
    );
}

#[test]
fn a_note_says_what_the_answers_to_clashes_did() {
    let into = PathBuf::from("/home/a/Documents");
    let copy = CopyJob {
        sources: paths(&["a", "b", "c", "d", "e"]),
        into: into.clone(),
    };
    let moved = MoveJob {
        sources: paths(&["a"]),
        into: into.clone(),
    };

    assert_eq!(
        CopyWords.done(&copy, Some(&into), tally(2, 1)).as_deref(),
        Some("Copied 4 items, replaced 2, skipped 1")
    );
    assert_eq!(
        CopyWords.done(&copy, None, tally(3, 0)).as_deref(),
        Some("Copied 5 items to Documents, replaced 3")
    );
    assert_eq!(
        MoveWords.done(&moved, None, tally(1, 0)).as_deref(),
        Some("Moved a to Documents, replaced 1")
    );
    assert_eq!(
        MoveWords.done(&moved, None, tally(0, 1)).as_deref(),
        Some("Nothing moved, skipped 1")
    );
    assert_eq!(
        CopyWords.done(&copy, None, tally(0, 5)).as_deref(),
        Some("Nothing copied, skipped 5")
    );
}

#[test]
fn a_copy_onto_itself_keeps_both_without_asking_and_a_move_asks() {
    let copy = CopyJob {
        sources: paths(&["a"]),
        into: "/home/a".into(),
    };
    let moved = MoveJob {
        sources: paths(&["a"]),
        into: "/home/a".into(),
    };
    assert_eq!(
        CopyWords.answers_itself(&copy, &ConflictKind::SameFile),
        Some(Answer::KeepBoth)
    );
    assert_eq!(
        CopyWords.answers_itself(&copy, &ConflictKind::FileExists),
        None
    );
    assert_eq!(
        MoveWords.answers_itself(&moved, &ConflictKind::SameFile),
        None
    );
}

#[test]
fn a_transfer_is_titled_by_what_goes_where() {
    let copy = CopyJob {
        sources: paths(&["film.mkv"]),
        into: "/home/a/Documents".into(),
    };
    assert_eq!(
        CopyWords.title(&copy).as_deref(),
        Some("film.mkv to Documents")
    );
    assert_eq!(
        CopyWords.doing(&copy),
        Doing {
            verb: "Copying",
            after_items: ""
        }
    );
}

#[test]
fn a_copy_or_move_says_an_odd_item_cannot_be_copied() {
    let copy = CopyJob {
        sources: paths(&["socket"]),
        into: PathBuf::from("/home/b"),
    };
    let moving = MoveJob {
        sources: paths(&["socket"]),
        into: PathBuf::from("/home/b"),
    };
    assert_eq!(
        CopyWords.why(&copy, Failure::Unsupported),
        Some("it can't be copied")
    );
    assert_eq!(
        MoveWords.why(&moving, Failure::Unsupported),
        Some("it can't be copied")
    );
}
