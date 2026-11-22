use std::fs::{self, File};
use std::time::{Duration, Instant};

use super::*;
use crate::journal::hold;
use nova_files_test_folder::TestFolder;

const ASK_WAIT: Duration = Duration::from_secs(5);

fn answer_once(shared: Arc<Job>, answer: Answer, scope: Scope) -> std::thread::JoinHandle<bool> {
    std::thread::spawn(move || {
        let deadline = Instant::now() + ASK_WAIT;
        while Instant::now() < deadline {
            if let Some(open) = shared.control.open() {
                return shared.control.answer(&open, answer, scope, || {});
            }
            std::thread::yield_now();
        }
        false
    })
}

#[test]
fn a_journal_that_cant_be_written_stays_until_the_job_ends_and_stops_replaces() {
    let folder = TestFolder::new("running-journal-fails");
    let path = folder.file("1", "nova-files-job 1\n");
    let file = File::open(&path).unwrap();
    let mut running = Running::with_journal(Journal::over(file, path.clone()));
    let (backup, of) = (Path::new("/b/.nova-backup-1-0"), Path::new("/b/a"));

    running.record(Record::Made(Path::new("/a"), Path::new("/b")));

    assert!(running.journal.is_none());
    assert!(path.exists());
    assert_eq!(
        running.record_backup(backup, of),
        Err(Stop::because(of, Failure::Other))
    );
    running.finish();
    assert!(!path.exists());
}

#[test]
fn a_backup_is_recorded_only_with_a_journal() {
    let folder = TestFolder::new("running-backup");
    let path = folder.file("1", "");
    let file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    let mut journaled = Running::with_journal(Journal::over(file, path.clone()));
    let (backup, of) = (Path::new("/b/.nova-backup-1-0"), Path::new("/b/a"));

    assert_eq!(journaled.record_backup(backup, of), Ok(()));
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "backup /b/.nova-backup-1-0 /b/a\n"
    );
    assert_eq!(
        Running::for_test().record_backup(backup, of),
        Err(Stop::because(of, Failure::Other))
    );
}

#[test]
fn a_question_puts_the_journal_on_the_disk_first() {
    let folder = TestFolder::new("running-ask-settles");
    let path = folder.file("1", "");
    let file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    let mut running = Running::with_journal(Journal::over(file, path));
    let answering = answer_once(Arc::clone(&running.shared), Answer::Skip, Scope::ThisOne);
    let before = running.journal_synced().unwrap();

    let answer = running.ask(Conflict {
        kind: ConflictKind::FileExists,
        source: PathBuf::from("/a/x"),
        target: PathBuf::from("/b/x"),
        number: 0,
        more: 0,
    });

    assert!(answering.join().unwrap());
    assert_eq!(answer, Ok(Answer::Skip));
    assert!(running.journal_synced().unwrap() > before);
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
    let path = Path::new("/locked");
    let answering = answer_once(Arc::clone(&running.shared), Answer::Skip, Scope::ThisOne);

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
    let left = Left {
        job: JobId(1),
        operation: nova_files_api::Operation::Undo { job: JobId(0) },
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

#[test]
fn a_kind_is_answered_once_answered_for_all() {
    let mut running = Running::for_test();
    let answering = answer_once(
        Arc::clone(&running.shared),
        Answer::Skip,
        Scope::AllOfThisKind,
    );
    let clash = |name: &str| Conflict {
        kind: ConflictKind::FileExists,
        source: PathBuf::from("/a").join(name),
        target: PathBuf::from("/b").join(name),
        number: 0,
        more: 1,
    };

    assert!(!running.answered(ConflictKind::FileExists));
    assert_eq!(running.ask(clash("x")), Ok(Answer::Skip));
    assert!(answering.join().unwrap());

    assert!(running.answered(ConflictKind::FileExists));
    assert!(!running.answered(ConflictKind::FolderExists));
    assert_eq!(running.ask(clash("y")), Ok(Answer::Skip));
}
