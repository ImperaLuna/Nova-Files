use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use nova_files_api::ConflictKind;

use super::*;

const SHORT: Duration = Duration::from_millis(50);
const LONG: Duration = Duration::from_secs(5);

#[test]
fn runs_until_cancelled() {
    let control = Control::default();
    assert_eq!(control.checkpoint(), Ok(()));
    control.cancel();
    assert_eq!(control.checkpoint(), Err(Cancelled));
    assert!(!control.pause());
    assert!(!control.resume());
}

#[test]
fn a_held_job_waits_until_resumed() {
    let control = Arc::new(Control::default());
    assert!(!control.paused());
    assert!(control.pause());
    assert!(control.paused());
    assert!(!control.pause());
    let (passed, reached) = mpsc::channel();
    let job = Arc::clone(&control);
    thread::spawn(move || passed.send(job.checkpoint()).unwrap());

    assert!(reached.recv_timeout(SHORT).is_err());
    assert!(control.resume());
    assert_eq!(reached.recv_timeout(LONG), Ok(Ok(())));
    assert!(!control.resume());
}

#[test]
fn a_held_job_can_be_cancelled() {
    let control = Arc::new(Control::default());
    control.pause();
    let (passed, reached) = mpsc::channel();
    let job = Arc::clone(&control);
    thread::spawn(move || passed.send(job.checkpoint()).unwrap());

    assert!(reached.recv_timeout(SHORT).is_err());
    control.cancel();
    assert_eq!(reached.recv_timeout(LONG), Ok(Err(Cancelled)));
}

fn conflict(kind: ConflictKind) -> Conflict {
    Conflict {
        kind,
        source: "/a".into(),
        target: "/b".into(),
        number: 1,
    }
}

#[test]
fn takes_only_answers_the_open_conflict_accepts() {
    let control = Control::default();
    let folder = conflict(ConflictKind::FolderExists);
    assert!(
        !control.answer(&folder, Answer::Skip, Scope::ThisOne, || {}),
        "nothing open"
    );
    control.raise(folder.clone());
    assert_eq!(control.open(), Some(folder.clone()));

    assert!(!control.answer(&folder, Answer::Replace, Scope::ThisOne, || {}));
    let mut other = folder.clone();
    other.target = "/c".into();
    assert!(
        !control.answer(&other, Answer::Merge, Scope::ThisOne, || {}),
        "not the one open"
    );
    let mut earlier = folder.clone();
    earlier.number = 0;
    assert!(
        !control.answer(&earlier, Answer::Merge, Scope::ThisOne, || {}),
        "a late answer to an earlier one that looked the same"
    );
    assert!(control.answer(&folder, Answer::Merge, Scope::AllOfThisKind, || {}));
    assert_eq!(control.open(), None);
    assert!(
        !control.answer(&folder, Answer::Skip, Scope::ThisOne, || {}),
        "answered once"
    );
    assert_eq!(
        control.answer_to(),
        Ok((Answer::Merge, Scope::AllOfThisKind))
    );
}

#[test]
fn a_raised_conflict_waits_for_its_answer_or_a_cancel() {
    let control = Arc::new(Control::default());
    control.raise(conflict(ConflictKind::NoSpace));
    let (passed, reached) = mpsc::channel();
    let job = Arc::clone(&control);
    thread::spawn(move || passed.send(job.answer_to()).unwrap());

    assert!(reached.recv_timeout(SHORT).is_err());
    let mut heard = 0;
    assert!(control.answer(
        &conflict(ConflictKind::NoSpace),
        Answer::Retry,
        Scope::ThisOne,
        || heard += 1
    ));
    assert_eq!(heard, 1, "told once, as it was taken");
    assert_eq!(
        reached.recv_timeout(LONG),
        Ok(Ok((Answer::Retry, Scope::ThisOne)))
    );

    control.raise(conflict(ConflictKind::SourceGone));
    let job = Arc::clone(&control);
    let (passed, reached) = mpsc::channel();
    thread::spawn(move || passed.send(job.answer_to()).unwrap());
    control.cancel();
    assert_eq!(reached.recv_timeout(LONG), Ok(Err(Cancelled)));
    assert_eq!(control.open(), None);
}
