use std::fs::{self, File};
use std::time::{Duration, Instant};

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

    running.record(Record::Made(Path::new("/a"), Path::new("/b")));

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

#[test]
fn a_failure_the_user_may_answer_is_asked_and_any_other_stops_the_job() {
    let mut running = Running::for_test();
    let control = Arc::clone(&running.shared);
    let path = Path::new("/locked");
    // Gives up after a while, so a job that never asks fails the test
    // instead of hanging it.
    let answering = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(open) = control.control.open() {
                return control
                    .control
                    .answer(&open, Answer::Skip, Scope::ThisOne, || {});
            }
            std::thread::yield_now();
        }
        false
    });

    let skipped = running.retry_or_skip(path, || Err::<(), _>(Failure::PermissionDenied));
    let stopped = running.retry_or_skip(path, || Err::<(), _>(Failure::ReadOnly));
    let done = running.retry_or_skip(path, || Ok::<_, Failure>(7));

    assert!(answering.join().unwrap());
    assert_eq!(skipped, Ok(None));
    assert_eq!(done, Ok(Some(7)));
    assert_eq!(stopped, Err(Stop::because(path, Failure::ReadOnly)));
}
