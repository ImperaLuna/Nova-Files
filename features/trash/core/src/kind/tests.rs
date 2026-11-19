use super::*;
use nova_files_api::JobId;
use nova_files_jobs::test_folder::TestFolder;
use std::fs;

fn none() -> Trashes {
    Trashes {
        home: None,
        drives: false,
    }
}

#[test]
fn nothing_done_or_gone_since_is_nothing_to_undo() {
    let folder = TestFolder::new("undo-nothing");
    let source = folder.path().join("a");
    let trash_op = Operation::Trash {
        paths: vec![source.clone()],
    };

    assert_eq!(undo_of(&trash_op, &[], &none()), None);
    let gone = folder.path().join("files/a");
    assert_eq!(undo_of(&trash_op, &[(source, gone)], &none()), None);
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
    fs::create_dir(folder.path().join("files")).unwrap();
    let elsewhere = folder.file("files/b", "b");
    let steps = [(source.clone(), item.clone()), (source, elsewhere)];

    let undo = undo_of(&trash_op, &steps, &trashes);

    assert_eq!(
        undo,
        Some(Undo::Restore(vec![(
            item.clone(),
            identity::of(&item).unwrap()
        )]))
    );
    assert_eq!(undo_of(&trash_op, &steps[..1], &none()), None);
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
        assert_eq!(undo_of(&operation, &steps, &none()), None, "{operation:?}");
    }
}
