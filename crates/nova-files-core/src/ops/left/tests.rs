use super::*;
use crate::ops::test_folder::TestFolder;

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
