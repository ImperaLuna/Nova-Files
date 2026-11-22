use super::*;

fn paths(names: &[&str]) -> Vec<PathBuf> {
    names
        .iter()
        .map(|name| Path::new("/home/a").join(name))
        .collect()
}

#[test]
fn each_trash_job_says_what_it_did() {
    let none = Tally::default();
    let skipped = Tally {
        replaced: 0,
        skipped: 1,
    };
    let trash = TrashJob {
        paths: paths(&["notes.txt"]),
    };
    assert_eq!(
        TrashWords.done(&trash, None, skipped).as_deref(),
        Some("Moved notes.txt to Trash")
    );
    let restore = RestoreJob {
        items: paths(&["a", "b"]),
    };
    assert_eq!(
        RestoreWords.done(&restore, None, none).as_deref(),
        Some("Restored 2 items")
    );
    let delete = DeleteFromTrashJob {
        items: paths(&["a"]),
    };
    assert_eq!(
        DeleteFromTrashWords.done(&delete, None, none).as_deref(),
        Some("Deleted 1 item")
    );
    assert_eq!(
        EmptyTrashWords.done(&EmptyTrashJob, None, none).as_deref(),
        Some("Trash emptied")
    );
}

#[test]
fn each_trash_job_names_what_it_works_on() {
    let trash = TrashJob {
        paths: paths(&["a", "b"]),
    };
    assert_eq!(TrashWords.doing(&trash), doing("Moving", " to Trash"));
    assert_eq!(TrashWords.sources(&trash).len(), 2);
    assert_eq!(EmptyTrashWords.sources(&EmptyTrashJob), &[] as &[PathBuf]);
    assert_eq!(
        EmptyTrashWords.failed(&EmptyTrashJob),
        failed("empty Trash", false)
    );
    assert_eq!(TrashWords.failed(&trash), failed("move to Trash", true));
}
