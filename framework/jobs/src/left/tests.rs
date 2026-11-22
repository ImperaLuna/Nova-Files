use super::*;
use nova_files_test_folder::TestFolder;
use std::os::unix::fs::MetadataExt as _;

fn entry_of(path: &Path) -> PathBuf {
    let there = path.symlink_metadata().unwrap();
    PathBuf::from(format!("{}:{}", there.dev(), there.ino()))
}

fn operation(lines: &[Line]) -> Option<Operation> {
    super::operation(lines, &|_, _, _| None)
}

fn line(word: &str, paths: &[&Path]) -> Line {
    Line {
        word: word.to_owned(),
        paths: paths.iter().map(|path| path.to_path_buf()).collect(),
    }
}

#[test]
fn a_backup_goes_once_a_moved_file_holds_its_name() {
    let folder = TestFolder::new("left-moved");
    let of = folder.file("a", "moved in");
    let backup = folder.file(&format!("{BACKUP_PREFIX}a"), "old");
    let from = folder.path().join("from");

    clean(&[
        line(word::BACKUP, &[&backup, &of]),
        line(word::MOVED, &[&from, &of]),
    ]);

    assert!(!backup.exists());
    assert_eq!(fs::read_to_string(&of).unwrap(), "moved in");
}

#[test]
fn only_the_jobs_hidden_names_are_cleaned_up() {
    let folder = TestFolder::new("left-hidden");
    let part = folder.file("part", "the user's");
    let backup = folder.file("backup", "the user's");
    let of = folder.path().join("of");

    clean(&[
        line(word::PART, &[&part]),
        line(word::BACKUP, &[&backup, &of]),
    ]);

    assert!(part.exists());
    assert!(backup.exists());
    assert!(!of.exists());
}

#[test]
fn an_info_file_whose_entry_never_reached_the_trash_goes() {
    let folder = TestFolder::new("left-trash-info");
    let trash = folder.path().join("Trash");
    fs::create_dir_all(trash.join("info")).unwrap();
    fs::create_dir_all(trash.join("files")).unwrap();
    let info = |name: &str| {
        let path = trash.join(format!("info/{name}.trashinfo"));
        fs::write(&path, "[Trash Info]\n").unwrap();
        path
    };
    let (lone, kept, stranger, theirs) =
        (info("lone"), info("kept"), info("stranger"), info("theirs"));
    let user = folder.file("user", "the user's");
    fs::rename(&user, trash.join("files/kept")).unwrap();
    let user = entry_of(&trash.join("files/kept"));
    fs::write(trash.join("files/stranger"), "theirs").unwrap();
    let elsewhere = PathBuf::from(format!("{}:0", lone.symlink_metadata().unwrap().dev()));

    clean(&[
        line(word::TRASH_INFO, &[&lone, &entry_of(&lone), &user]),
        line(word::TRASH_INFO, &[&kept, &entry_of(&kept), &user]),
        line(word::TRASH_INFO, &[&stranger, &entry_of(&stranger), &user]),
        line(word::TRASH_INFO, &[&theirs, &elsewhere, &user]),
    ]);

    assert!(!lone.exists());
    assert!(kept.exists());
    assert!(!stranger.exists(), "never names another app's entry");
    assert!(theirs.exists());
}

#[test]
fn every_info_file_a_put_back_cut_short_left_goes() {
    let folder = TestFolder::new("left-put-back");
    let trash = folder.path().join("Trash");
    fs::create_dir_all(trash.join("info")).unwrap();
    fs::create_dir_all(trash.join("files")).unwrap();
    let info = |name: &str| {
        let path = trash.join(format!("info/{name}.trashinfo"));
        fs::write(&path, "[Trash Info]\n").unwrap();
        path
    };
    let (first, last) = (info("first"), info("last"));
    let gone = folder.file("gone", "put back");
    let item = entry_of(&gone);
    let trashed = trash.join("files/copy");

    clean(&[
        line(word::TRASH, &[]),
        line(word::TRASHED, &[&folder.path().join("copy"), &trashed]),
        line(word::PUT_BACK, &[]),
        line(word::TRASH_INFO, &[&first, &entry_of(&first), &item]),
        line(word::TRASH_INFO, &[&last, &entry_of(&last), &item]),
    ]);

    assert!(!first.exists());
    assert!(!last.exists());
}

#[test]
fn a_recorded_path_that_is_no_info_file_is_left_alone() {
    let folder = TestFolder::new("left-trash-other");
    let other = folder.file("notes.trashinfo", "the user's");
    let outside = folder.file("notes", "the user's");

    clean(&[
        line(
            word::TRASH_INFO,
            &[&other, &entry_of(&other), &entry_of(&other)],
        ),
        line(
            word::TRASH_INFO,
            &[&outside, &entry_of(&outside), &entry_of(&outside)],
        ),
    ]);

    assert!(other.exists());
    assert!(outside.exists());
}

#[test]
fn a_trash_job_remembers_what_it_trashed() {
    let a = Path::new("/a");

    let done = clean(&[line(word::TRASHED, &[a, Path::new("/t/files/a")])]);

    assert!(done.trashed.contains(a));
}

#[test]
fn a_trash_job_cleans_up_only_its_last_info_file() {
    let folder = TestFolder::new("left-trash-last");
    let trash = folder.path().join("Trash");
    fs::create_dir_all(trash.join("info")).unwrap();
    fs::create_dir_all(trash.join("files")).unwrap();
    let info = |name: &str| {
        let path = trash.join(format!("info/{name}.trashinfo"));
        fs::write(&path, "[Trash Info]\n").unwrap();
        path
    };
    let (earlier, done, last) = (info("earlier"), info("done"), info("last"));
    let never = PathBuf::from("0:0");
    let trashed = |name: &str| line(word::TRASHED, &[Path::new("/x"), &trash.join(name)]);

    clean(&[
        line(word::TRASH, &[]),
        line(word::TRASH_INFO, &[&earlier, &entry_of(&earlier), &never]),
        line(word::TRASH_INFO, &[&done, &entry_of(&done), &never]),
        trashed("files/done"),
        line(word::TRASH_INFO, &[&last, &entry_of(&last), &never]),
    ]);

    assert!(earlier.exists() && done.exists());
    assert!(!last.exists(), "in flight when the crash came");
    let finished = info("finished");
    clean(&[
        line(word::TRASH, &[]),
        line(word::TRASH_INFO, &[&finished, &entry_of(&finished), &never]),
        trashed("files/finished"),
    ]);
    assert!(finished.exists());
}

#[test]
fn an_undo_moving_entries_back_is_never_resumed() {
    let a = Path::new("/a");
    let b = Path::new("/b");

    assert_eq!(
        operation(&[line(word::UNDO, &[]), line(word::MOVED, &[a, b])]),
        None
    );
    assert_eq!(
        operation(&[line(word::UNDO, &[]), line(word::SOURCES_DONE, &[])]),
        None
    );
}

#[test]
fn a_job_the_shared_part_does_not_know_is_rebuilt_by_its_feature() {
    #[derive(Debug, PartialEq, Eq)]
    struct Pack(Vec<PathBuf>, Option<PathBuf>);
    let a = Path::new("/a");
    let into = Path::new("/into");
    let lines = [
        line("pack", &[]),
        line(word::INTO, &[into]),
        line(word::SOURCE, &[a]),
        line(word::SOURCES_DONE, &[]),
    ];
    let rebuild = |word: &str, into: Option<PathBuf>, sources: Vec<PathBuf>| {
        (word == "pack").then(|| Operation::feature(Pack(sources, into)))
    };

    assert_eq!(
        super::operation(&lines, &rebuild),
        Some(Operation::feature(Pack(vec![a.into()], Some(into.into()))))
    );
    assert_eq!(operation(&lines), None);
}

#[test]
fn a_journal_no_feature_here_knows_is_left_as_it_was() {
    let folder = TestFolder::new("left-foreign");
    let source = folder.file("a", "kept");
    let journals = folder.path().join("jobs");
    fs::create_dir(&journals).unwrap();
    let journal = journals.join("4");
    let backup = folder.path().join(".nova-backup-a");
    fs::write(
        &journal,
        format!(
            "nova-files-job 1\npack\nsource {}\nbackup {} {}\n",
            source.display(),
            backup.display(),
            source.display()
        ),
    )
    .unwrap();
    let written = fs::read(&journal).unwrap();

    let found = recover(&journals, &|_, _, _| None);

    assert!(found.is_empty());
    assert_eq!(fs::read(&journal).unwrap(), written);
    assert_eq!(fs::read_to_string(&source).unwrap(), "kept");
}
