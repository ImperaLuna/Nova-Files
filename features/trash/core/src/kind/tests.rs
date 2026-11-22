use super::*;
use nova_files_api::JobId;
use nova_files_jobs::mover::Moved;
use nova_files_test_folder::TestFolder;
use std::fs;

struct NoFiles;

impl Mover for NoFiles {
    fn move_pairs(&self, _: &mut Running, _: &[(PathBuf, PathBuf)]) -> Moved {
        (Ok(()), Vec::new())
    }

    fn delete(&self, _: &mut Running, _: &[PathBuf]) -> Moved {
        (Ok(()), Vec::new())
    }
}

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
    let trash_op = trashing(vec![source.clone()]);

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
    let trash_op = trashing(vec![source.clone()]);
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
fn deletes_from_the_trash_and_undos_are_never_undone() {
    let folder = TestFolder::new("undo-never");
    let a = folder.file("a", "a");
    let steps = [(a.clone(), a.clone())];

    for operation in [
        deleting(vec![a]),
        emptying(),
        Operation::Undo { job: JobId(1) },
    ] {
        assert_eq!(undo_of(&operation, &steps, &none()), None, "{operation:?}");
    }
}

#[test]
fn trash_jobs_left_by_a_crash_resume_with_what_they_were_given() {
    let jobs = TrashJobs {
        trashes: none(),
        files: NoFiles,
    };
    let paths = vec![PathBuf::from("/a"), PathBuf::from("/b")];
    let resumed = |name| jobs.resume(name, None, paths.clone());

    assert_eq!(resumed(word::TRASH), Some(trashing(paths.clone())));
    assert_eq!(resumed(word::RESTORE), Some(restoring(paths.clone())));
    assert_eq!(
        resumed(word::DELETE_FROM_TRASH),
        Some(deleting(paths.clone()))
    );
    assert_eq!(resumed(word::EMPTY_TRASH), Some(emptying()));
    assert_eq!(resumed("copy"), None);
    assert_eq!(jobs.resume(word::TRASH, Some("/into".into()), paths), None);
}
