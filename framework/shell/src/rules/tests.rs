use std::path::PathBuf;

use super::{Allow, FolderRules, RulesBook, Views};

#[test]
fn a_folder_nobody_claims_allows_everything() {
    let rules = RulesBook::default().for_folder(&PathBuf::from("/home/me"));
    assert!(rules.meets(&[Allow::Change, Allow::TakeOut, Allow::Restyle]));
    assert_eq!(rules.views(), Views::Any);
}

#[test]
fn a_claim_covers_the_folders_under_it() {
    let book = RulesBook::default();
    book.claim(
        "archives",
        PathBuf::from("/a.zip"),
        FolderRules::allowing(&[Allow::TakeOut]),
    );
    let inside = book.for_folder(&PathBuf::from("/a.zip/docs"));
    assert!(inside.meets(&[Allow::TakeOut]));
    assert!(!inside.meets(&[Allow::TakeOut, Allow::Change]));
    assert_eq!(inside.views(), Views::DetailsOnly);
    assert!(book.for_folder(&PathBuf::from("/b")).allows(Allow::Change));
}

#[test]
fn two_claims_allow_only_what_both_allow() {
    let book = RulesBook::default();
    book.claim(
        "outer",
        PathBuf::from("/x"),
        FolderRules::allowing(&[Allow::TakeOut, Allow::Restyle]),
    );
    book.claim(
        "inner",
        PathBuf::from("/x/y"),
        FolderRules::allowing(&[Allow::Change, Allow::Restyle]),
    );
    let both = book.for_folder(&PathBuf::from("/x/y"));
    assert!(both.allows(Allow::Restyle));
    assert!(!both.allows(Allow::TakeOut));
    assert!(!both.allows(Allow::Change));
}

#[test]
fn needing_nothing_is_met_anywhere() {
    assert!(FolderRules::allowing(&[]).meets(&[]));
}
