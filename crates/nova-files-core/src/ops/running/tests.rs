use std::fs::{self, File};
use std::time::{Duration, Instant};

use super::*;
use crate::ops::journal::hold;
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

#[test]
fn a_journal_that_cant_be_opened_again_still_goes_when_the_job_ends() {
    let folder = TestFolder::new("running-reopen-fails");
    let path = folder.file("1", "nova-files-job 1\n");
    // Opened to read only: it can't be cut back to its whole lines.
    let left = Left {
        job: JobId(1),
        operation: nova_files_api::Operation::EmptyTrash,
        done: Done::default(),
        journal: path.clone(),
        file: File::open(&path).unwrap(),
        whole: 0,
    };
    hold(&left.file).unwrap();
    let mut running = Running::for_test();

    running.resuming(left, 2);
    assert!(running.journal.is_none());
    let other_core = hold(&File::open(&path).unwrap()).unwrap_err();
    assert_eq!(other_core.kind(), io::ErrorKind::WouldBlock, "still held");
    running.finish();

    assert!(!path.exists());
}
