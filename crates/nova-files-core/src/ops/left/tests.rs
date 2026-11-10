use super::*;
use crate::ops::test_folder::TestFolder;
use std::os::unix::fs::MetadataExt as _;

/// The journal field naming the entry at `path`.
fn entry_of(path: &Path) -> PathBuf {
    let there = path.symlink_metadata().unwrap();
    PathBuf::from(format!("{}:{}", there.dev(), there.ino()))
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
    // Got there, though the crash came before it was recorded.
    let user = folder.file("user", "the user's");
    fs::rename(&user, trash.join("files/kept")).unwrap();
    let user = entry_of(&trash.join("files/kept"));
    // Another app's entry, without its info file, holds the name.
    fs::write(trash.join("files/stranger"), "theirs").unwrap();
    let elsewhere = PathBuf::from(format!("{}:0", lone.symlink_metadata().unwrap().dev()));

    clean(&[
        line(word::TRASH_INFO, &[&lone, &entry_of(&lone), &user]),
        line(word::TRASH_INFO, &[&kept, &entry_of(&kept), &user]),
        line(word::TRASH_INFO, &[&stranger, &entry_of(&stranger), &user]),
        // Another app's info file took the name after the crash.
        line(word::TRASH_INFO, &[&theirs, &elsewhere, &user]),
    ]);

    assert!(!lone.exists());
    assert!(kept.exists());
    assert!(!stranger.exists(), "never names another app's entry");
    assert!(theirs.exists());
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
fn a_trash_job_resumes_with_the_paths_it_was_given() {
    let a = Path::new("/a");
    let b = Path::new("/b");

    let resumed = operation(&[
        line(word::TRASH, &[]),
        line(word::SOURCE, &[a]),
        line(word::SOURCE, &[b]),
        line(word::SOURCES_DONE, &[]),
    ]);

    assert_eq!(
        resumed,
        Some(Operation::Trash {
            paths: vec![a.into(), b.into()]
        })
    );
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
        // Settled by the job, its inode since another info file's.
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
