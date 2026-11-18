use super::*;

#[test]
fn stars_and_question_marks() {
    assert!(glob_matches("*.pdf", "report.pdf", true));
    assert!(glob_matches("*.pdf", ".pdf", true));
    assert!(!glob_matches("*.pdf", "report.pdfx", true));
    assert!(glob_matches("*.tar.gz", "backup.tar.gz", true));
    assert!(glob_matches("a*b*c", "aXbYbZc", true));
    assert!(!glob_matches("a*b*c", "aXbYbZ", true));
    assert!(glob_matches("README*", "README", true));
    assert!(glob_matches("?.c", "x.c", true));
    assert!(!glob_matches("?.c", "xy.c", true));
    assert!(!glob_matches("?", "", true));
    assert!(glob_matches("*", "", true));
    assert!(glob_matches("Makefile", "Makefile", true));
    assert!(!glob_matches("Makefile", "Makefile.am", true));
}

#[test]
fn sets_ranges_and_negation() {
    assert!(glob_matches("*.[ch]", "main.c", true));
    assert!(glob_matches("*.[ch]", "main.h", true));
    assert!(!glob_matches("*.[ch]", "main.o", true));
    assert!(glob_matches("*.[1-9]", "ls.1", true));
    assert!(!glob_matches("*.[1-9]", "ls.0", true));
    assert!(glob_matches("[!.]*", "notes", true));
    assert!(!glob_matches("[!.]*", ".hidden", true));
    assert!(glob_matches("[Mm]akefile", "makefile", true));
    assert!(glob_matches("a[b", "a[b", true));
    assert!(!glob_matches("a[b", "ab", true));
}

#[test]
fn case_folds_unless_asked_not_to() {
    assert!(glob_matches("*.png", "PHOTO.PNG", false));
    assert!(!glob_matches("*.png", "PHOTO.PNG", true));
    assert!(glob_matches("*.C", "main.C", true));
    assert!(!glob_matches("*.C", "main.c", true));
}

#[test]
fn a_negated_set_can_hold_a_closing_bracket() {
    assert!(glob_matches("[!]]*.txt", "notes.txt", true));
    assert!(!glob_matches("[!]]*.txt", "]notes.txt", true));
    assert!(glob_matches("[]]x", "]x", true));
}
