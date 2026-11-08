use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::{Typed, keep_both, read, stem};

fn typed(text: &str) -> Typed {
    read(text, Path::new("/home/me"), Path::new("/home/me/Documents"))
}

#[test]
fn nothing_typed_is_nothing() {
    assert_eq!(typed(""), Typed::Nothing);
    assert_eq!(typed("   "), Typed::Nothing);
}

#[test]
fn a_plain_name_is_a_name_in_the_folder_on_screen() {
    assert_eq!(typed("notes.txt"), Typed::Name("notes.txt".to_owned()));
    assert_eq!(typed("  notes.txt "), Typed::Name("notes.txt".to_owned()));
    // Only `~` alone or `~/` means home.
    assert_eq!(typed("~notes"), Typed::Name("~notes".to_owned()));
}

#[test]
fn a_tilde_goes_home() {
    assert_eq!(typed("~"), Typed::Folder(PathBuf::from("/home/me")));
    assert_eq!(typed("~/"), Typed::Folder(PathBuf::from("/home/me")));
    assert_eq!(
        typed("~/Pictures"),
        Typed::Path(PathBuf::from("/home/me/Pictures"))
    );
    assert_eq!(
        typed("~/Pictures/"),
        Typed::Folder(PathBuf::from("/home/me/Pictures"))
    );
}

#[test]
fn a_path_is_tried_as_a_folder_unless_it_ends_in_a_slash() {
    assert_eq!(typed("/etc"), Typed::Path(PathBuf::from("/etc")));
    assert_eq!(typed("/etc/"), Typed::Folder(PathBuf::from("/etc")));
    assert_eq!(typed("/"), Typed::Folder(PathBuf::from("/")));
    // Relative to the folder on screen.
    assert_eq!(
        typed("Work/plan.txt"),
        Typed::Path(PathBuf::from("/home/me/Documents/Work/plan.txt"))
    );
    assert_eq!(
        typed("../"),
        Typed::Folder(PathBuf::from("/home/me/Documents/.."))
    );
}

#[test]
fn the_stem_stops_at_the_extension() {
    assert_eq!(stem("report-q3.pdf"), 0..9);
    assert_eq!(stem("my.report.pdf"), 0..9);
    assert_eq!(stem("backup.tar.gz"), 0..6);
    assert_eq!(stem("README"), 0..6);
    assert_eq!(stem(".bashrc"), 0..7);
    // `.tar.gz` alone is all extension after a leading dot.
    assert_eq!(stem(".tar.gz"), 0..4);
}

#[test]
fn keeping_both_numbers_the_new_one() {
    let taken = |names: &'static [&'static str]| {
        move |name: &OsStr| names.iter().any(|n| OsStr::new(n) == name)
    };
    assert_eq!(
        keep_both(OsStr::new("report-q3.pdf"), taken(&["report-q3.pdf"])),
        "report-q3 (2).pdf"
    );
    assert_eq!(
        keep_both(
            OsStr::new("report-q3.pdf"),
            taken(&["report-q3.pdf", "report-q3 (2).pdf"])
        ),
        "report-q3 (3).pdf"
    );
    assert_eq!(
        keep_both(OsStr::new("backup.tar.gz"), taken(&[])),
        "backup (2).tar.gz"
    );
    assert_eq!(keep_both(OsStr::new("README"), taken(&[])), "README (2)");
    assert_eq!(keep_both(OsStr::new(".bashrc"), taken(&[])), ".bashrc (2)");
}

#[test]
fn a_name_that_is_not_utf8_keeps_its_bytes() {
    use std::os::unix::ffi::OsStrExt as _;
    let name = OsStr::from_bytes(b"caf\xe9.txt");
    let kept = keep_both(name, |_| false);
    assert_eq!(kept.as_bytes(), b"caf\xe9.txt (2)");
}
