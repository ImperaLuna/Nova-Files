use super::*;

static LOCKED: Question = Question::new("locked", &[Answer::Retry]);
static TWIN: Question = Question::new("locked", &[Answer::Retry]);
static BARE: Question = Question::new("bare", &[]);

#[test]
fn a_question_is_its_own_static_whatever_its_name() {
    assert_eq!(ConflictKind::Asked(&LOCKED), ConflictKind::Asked(&LOCKED));
    assert_ne!(ConflictKind::Asked(&LOCKED), ConflictKind::Asked(&TWIN));
    assert_eq!(format!("{LOCKED:?}"), "locked");
}

#[test]
fn a_question_takes_skip_and_only_the_answers_it_names() {
    let kind = ConflictKind::Asked(&LOCKED);
    assert!(kind.accepts(Answer::Skip));
    assert!(kind.accepts(Answer::Retry));
    for answer in [Answer::Replace, Answer::Merge, Answer::KeepBoth] {
        assert!(!kind.accepts(answer));
    }
    let bare = ConflictKind::Asked(&BARE);
    assert!(bare.accepts(Answer::Skip));
    assert!(!bare.accepts(Answer::Retry));
}

#[test]
fn a_feature_job_opens_only_as_its_own_type() {
    #[derive(Debug, PartialEq, Eq)]
    struct Pack(u8);
    let operation = Operation::feature(Pack(1));
    assert_eq!(operation.get::<Pack>(), Some(&Pack(1)));
    assert_eq!(operation.get::<u8>(), None);
    assert_eq!(Operation::EmptyTrash.get::<Pack>(), None);
}
