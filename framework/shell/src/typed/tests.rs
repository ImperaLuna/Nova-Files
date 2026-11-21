use std::path::{Path, PathBuf};

use super::{Typed, read};

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
fn dots_alone_are_folders() {
    assert_eq!(
        typed(".."),
        Typed::Folder(PathBuf::from("/home/me/Documents/.."))
    );
    assert_eq!(
        typed(" . "),
        Typed::Folder(PathBuf::from("/home/me/Documents/."))
    );
    assert_eq!(typed("..notes"), Typed::Name("..notes".to_owned()));
}
