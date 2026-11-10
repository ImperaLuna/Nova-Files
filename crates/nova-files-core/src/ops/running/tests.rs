use std::fs::{self, File};

use super::*;
use crate::ops::test_folder::TestFolder;

#[test]
fn a_journal_that_cant_be_written_goes() {
    let folder = TestFolder::new("running-journal-fails");
    let path = folder.file("1", "");
    // Opened to read only: every write fails.
    let file = File::open(&path).unwrap();
    let mut running = Running::for_test();
    running.journal = Some(Journal::over(file, path.clone()));

    running.record(Record::Made(Path::new("/a")));

    assert!(running.journal.is_none());
    assert!(!path.exists());
}

#[test]
fn settling_syncs_the_journal() {
    let folder = TestFolder::new("running-settle");
    let path = folder.file("1", "");
    let file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    let mut running = Running::for_test();
    running.journal = Some(Journal::over(file, path));
    let before = running.journal.as_ref().unwrap().synced();

    running.settle();

    assert!(running.journal.as_ref().unwrap().synced() > before);
}
