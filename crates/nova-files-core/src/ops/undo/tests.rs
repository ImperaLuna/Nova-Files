use super::*;
use crate::ops::TestFolder;

fn done() -> Outcome {
    Outcome::Done
}

/// No trash at all.
fn none() -> Trashes {
    Trashes {
        home: None,
        drives: false,
    }
}

#[test]
fn a_copy_sends_its_copies_still_there_to_the_trash() {
    let folder = TestFolder::new("undo-copy");
    let copy = folder.file("a (2)", "a");
    let source = folder.path().join("a");
    let copy_op = Operation::Copy {
        sources: vec![source.clone()],
        into: folder.path().to_path_buf(),
    };
    let steps = vec![
        (source, copy.clone()),
        (folder.path().join("b"), folder.path().join("gone")),
    ];

    let undo = undo_of(&copy_op, &steps, &done(), &none());

    assert_eq!(
        undo,
        Some(Undo::Trash(vec![(copy.clone(), entry(&copy).unwrap())]))
    );
}

#[test]
fn a_move_goes_back_even_when_it_stopped_part_way() {
    let folder = TestFolder::new("undo-move");
    let moved = folder.file("b", "b");
    let from = folder.path().join("from/b");
    let move_op = Operation::Move {
        sources: vec![from.clone(), folder.path().join("from/c")],
        into: folder.path().to_path_buf(),
    };
    let cancelled = Outcome::Cancelled;

    let undo = undo_of(
        &move_op,
        &[(from.clone(), moved.clone())],
        &cancelled,
        &none(),
    );

    assert_eq!(
        undo,
        Some(Undo::MoveBack(vec![(
            moved.clone(),
            from,
            entry(&moved).unwrap()
        )]))
    );
}

#[test]
fn nothing_done_or_gone_since_is_nothing_to_undo() {
    let folder = TestFolder::new("undo-nothing");
    let source = folder.path().join("a");
    let trash_op = Operation::Trash {
        paths: vec![source.clone()],
    };

    assert_eq!(undo_of(&trash_op, &[], &done(), &none()), None);
    let gone = folder.path().join("files/a");
    assert_eq!(
        undo_of(&trash_op, &[(source, gone)], &done(), &none()),
        None
    );
}

#[test]
fn a_trashed_entry_comes_back_only_from_a_trash_it_can_be_restored_from() {
    let folder = TestFolder::new("undo-trash-where");
    fs::create_dir_all(folder.path().join("Trash/info")).unwrap();
    fs::create_dir_all(folder.path().join("Trash/files")).unwrap();
    let source = folder.path().join("a");
    let trash_op = Operation::Trash {
        paths: vec![source.clone()],
    };
    let trashes = Trashes {
        home: Some(folder.path().join("Trash")),
        drives: false,
    };
    let item = folder
        .path()
        .join("Trash/files")
        .canonicalize()
        .unwrap()
        .join("a");
    fs::write(&item, "a").unwrap();
    // In a files folder, though of no trash.
    fs::create_dir(folder.path().join("files")).unwrap();
    let elsewhere = folder.file("files/b", "b");
    let steps = [(source.clone(), item.clone()), (source, elsewhere)];

    let undo = undo_of(&trash_op, &steps, &done(), &trashes);

    assert_eq!(
        undo,
        Some(Undo::Restore(vec![(item.clone(), entry(&item).unwrap())]))
    );
    assert_eq!(undo_of(&trash_op, &steps[..1], &done(), &none()), None);
}

#[test]
fn a_rename_to_its_own_name_is_nothing_to_undo() {
    let folder = TestFolder::new("undo-same-name");
    let path = folder.file("a", "a");
    let rename_op = Operation::Rename {
        path,
        name: "a".into(),
    };

    assert_eq!(undo_of(&rename_op, &[], &done(), &none()), None);
}

#[test]
fn a_rename_and_a_new_folder_are_taken_back_only_once_done() {
    let folder = TestFolder::new("undo-one-step");
    let renamed = folder.file("new", "x");
    let rename_op = Operation::Rename {
        path: folder.path().join("old"),
        name: "new".into(),
    };
    let made = folder.path().join("made");
    fs::create_dir(&made).unwrap();
    let folder_op = Operation::NewFolder {
        inside: folder.path().to_path_buf(),
        name: "made".into(),
    };
    let failed = Outcome::Failed {
        path: made.clone(),
        reason: Failure::NameTaken,
    };

    assert_eq!(
        undo_of(&rename_op, &[], &done(), &none()),
        Some(Undo::Rename(
            renamed.clone(),
            entry(&renamed).unwrap(),
            "old".into()
        ))
    );
    assert_eq!(
        undo_of(&folder_op, &[], &done(), &none()),
        Some(Undo::RemoveFolder(made.clone(), entry(&made).unwrap()))
    );
    assert_eq!(undo_of(&rename_op, &[], &failed, &none()), None);
    assert_eq!(undo_of(&folder_op, &[], &failed, &none()), None);
}

#[test]
fn deletes_and_undos_are_never_undone() {
    let folder = TestFolder::new("undo-never");
    let a = folder.file("a", "a");
    let steps = [(a.clone(), a.clone())];

    for operation in [
        Operation::Delete {
            paths: vec![a.clone()],
        },
        Operation::DeleteFromTrash { items: vec![a] },
        Operation::EmptyTrash,
        Operation::Undo { job: JobId(1) },
    ] {
        assert_eq!(
            undo_of(&operation, &steps, &done(), &none()),
            None,
            "{operation:?}"
        );
    }
}

#[test]
fn the_history_keeps_the_last_jobs_and_gives_each_once() {
    let history = History::default();
    let audience = Audience::default();
    let undoable = |job| Undoable {
        job: JobId(job),
        operation: Operation::EmptyTrash,
        undo: Undo::RemoveFolder(PathBuf::from("/a"), entry(Path::new("/")).unwrap()),
    };
    // One put back after a failed undo goes where its age puts it.
    for job in (0..=u64::try_from(KEPT).unwrap()).filter(|job| *job != 3) {
        history.add(undoable(job), &audience);
    }
    history.add(undoable(3), &audience);

    let mut told = Vec::new();
    history.told(|events| told = events);

    let told_of = |job| Event::Undoable {
        job: JobId(job),
        operation: Operation::EmptyTrash,
    };
    assert_eq!(told.len(), KEPT);
    assert_eq!(
        told[0],
        Event::Undoable {
            job: JobId(1),
            operation: Operation::EmptyTrash
        }
    );
    assert_eq!(history.take(JobId(0), &audience), None, "pushed out");
    assert_eq!(told[2], told_of(3));
    assert_eq!(history.take(JobId(3), &audience), Some(undoable(3)));
    assert_eq!(history.take(JobId(3), &audience), None, "taken once");
}
