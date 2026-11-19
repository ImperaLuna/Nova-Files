use nova_files_api::{Answer, Conflict, ConflictKind, Failure};

use super::words::{failure, name_of};

const ORDER: [Answer; 5] = [
    Answer::Skip,
    Answer::KeepBoth,
    Answer::Merge,
    Answer::Replace,
    Answer::Retry,
];

const ALREADY_HERE: &str = " is already here";
const NO_TRASH: &str = "its drive has no trash";
const IN_TRASH: &str = "it is in the Trash already";
const PASSWORD_NEEDED: &str = "needs a password";
const PASSWORD_WRONG: &str = "wrong password";

pub fn words(conflict: &Conflict) -> (String, String) {
    let reason = match conflict.kind {
        ConflictKind::FileExists
        | ConflictKind::FolderExists
        | ConflictKind::KindMismatch
        | ConflictKind::SameFile => return (name_of(&conflict.target), ALREADY_HERE.to_owned()),
        ConflictKind::NoSpace => failure(Failure::NoSpace),
        ConflictKind::PermissionDenied => failure(Failure::PermissionDenied),
        ConflictKind::SourceGone => failure(Failure::NotFound),
        ConflictKind::SourceChanged => failure(Failure::SourceChanged),
        ConflictKind::Unsupported => failure(Failure::Unsupported),
        ConflictKind::NoTrash => NO_TRASH,
        ConflictKind::InTrash => IN_TRASH,
        ConflictKind::PasswordNeeded => PASSWORD_NEEDED,
        ConflictKind::PasswordWrong => PASSWORD_WRONG,
    };
    (name_of(&conflict.source), format!(": {reason}"))
}

pub fn answers(kind: ConflictKind) -> Vec<Answer> {
    ORDER
        .into_iter()
        .filter(|&answer| kind.accepts(answer))
        .collect()
}

pub const fn label(answer: Answer) -> &'static str {
    match answer {
        Answer::Skip => "Skip",
        Answer::KeepBoth => "Keep both",
        Answer::Merge => "Merge",
        Answer::Replace => "Replace",
        Answer::Retry => "Retry",
    }
}

pub const fn selector(answer: Answer) -> &'static str {
    match answer {
        Answer::Skip => "island-skip",
        Answer::KeepBoth => "island-keep-both",
        Answer::Merge => "island-merge",
        Answer::Replace => "island-replace",
        Answer::Retry => "island-retry",
    }
}

pub fn for_all(conflict: &Conflict) -> Option<String> {
    (conflict.more > 0).then(|| format!("For all {}", conflict.more.saturating_add(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conflict(kind: ConflictKind, more: u64) -> Conflict {
        Conflict {
            kind,
            source: "/a/notes.txt".into(),
            target: "/b/notes.txt".into(),
            number: 1,
            more,
        }
    }

    #[test]
    fn a_taken_name_is_already_here() {
        let clash = conflict(ConflictKind::FileExists, 0);
        assert_eq!(
            words(&clash),
            ("notes.txt".to_owned(), " is already here".to_owned())
        );
        let full = conflict(ConflictKind::NoSpace, 0);
        assert_eq!(
            words(&full),
            ("notes.txt".to_owned(), ": the drive is full".to_owned())
        );
        let no_trash = conflict(ConflictKind::NoTrash, 0);
        assert_eq!(words(&no_trash).1, ": its drive has no trash");
        let in_trash = conflict(ConflictKind::InTrash, 0);
        assert_eq!(words(&in_trash).1, ": it is in the Trash already");
    }

    #[test]
    fn each_kind_offers_what_it_takes_in_order() {
        use Answer::{KeepBoth, Merge, Replace, Retry, Skip};
        assert_eq!(answers(ConflictKind::FileExists), [Skip, KeepBoth, Replace]);
        assert_eq!(answers(ConflictKind::FolderExists), [Skip, KeepBoth, Merge]);
        assert_eq!(answers(ConflictKind::SameFile), [Skip, KeepBoth]);
        assert_eq!(answers(ConflictKind::NoSpace), [Skip, Retry]);
        assert_eq!(answers(ConflictKind::SourceGone), [Skip]);
        assert_eq!(answers(ConflictKind::InTrash), [Skip]);
        assert_eq!(label(KeepBoth), "Keep both");
        assert_eq!(selector(Replace), "island-replace");
    }

    #[test]
    fn for_all_counts_this_one_and_shows_only_when_more_are_coming() {
        assert_eq!(for_all(&conflict(ConflictKind::FileExists, 0)), None);
        assert_eq!(
            for_all(&conflict(ConflictKind::FileExists, 3)).as_deref(),
            Some("For all 4")
        );
    }
}
