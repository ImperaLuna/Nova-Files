use std::ffi::OsStr;
use std::path::Path;

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
    // An unclosed `[` is a plain character.
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

/// A database that knows `.md` as Markdown, a kind of plain text.
#[derive(Debug)]
struct Types;

impl FileTypes for Types {
    fn of(&self, name: &OsStr) -> Option<&str> {
        match Path::new(name).extension()?.to_str()? {
            "md" => Some("text/markdown"),
            "png" => Some("image/png"),
            _ => None,
        }
    }

    fn is_a(&self, mime: &str, wanted: &str) -> bool {
        mime == wanted || (mime == "text/markdown" && wanted == "text/plain")
    }
}

fn filter(rules: &[FilterRule]) -> FileFilter {
    FileFilter {
        name: "Some".to_owned(),
        rules: rules.to_vec(),
    }
}

fn admits(filter: &FileFilter, name: &str) -> bool {
    filter.admits(OsStr::new(name), Some(&Types))
}

#[test]
fn a_glob_rule_admits_by_name_alone() {
    let pdfs = filter(&[FilterRule::Glob("*.pdf".to_owned())]);
    assert!(admits(&pdfs, "a.PDF"));
    assert!(!admits(&pdfs, "a.md"));
    // A name the database doesn't know is judged by the glob alone.
    assert!(!admits(&pdfs, "a.xyz"));
}

#[test]
fn a_mime_rule_admits_the_type_and_its_kinds() {
    let text = filter(&[FilterRule::MimeType("text/plain".to_owned())]);
    assert!(admits(&text, "notes.md"));
    assert!(!admits(&text, "photo.png"));
    // Nothing known about the name: let it through.
    assert!(admits(&text, "README"));
}

#[test]
fn any_rule_admits() {
    let both = filter(&[
        FilterRule::MimeType("image/png".to_owned()),
        FilterRule::Glob("*.pdf".to_owned()),
    ]);
    assert!(admits(&both, "a.pdf"));
    assert!(admits(&both, "a.png"));
    assert!(!admits(&both, "a.md"));
}

#[test]
fn without_a_database_mime_rules_admit_everything() {
    let text = filter(&[FilterRule::MimeType("text/plain".to_owned())]);
    assert!(text.admits(OsStr::new("photo.png"), None));
    let none = filter(&[]);
    assert!(!none.admits(OsStr::new("photo.png"), None));
}
