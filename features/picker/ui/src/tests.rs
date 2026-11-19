use std::ffi::OsStr;

use nova_files_picker_api::{PickId, PickReply};

use super::*;

fn request(title: &str, kind: PickKind) -> PickRequest {
    PickRequest {
        id: PickId(1),
        kind,
        title: title.to_owned(),
        accept_label: None,
        filters: Vec::new(),
        filter: None,
        folder: None,
        parent: None,
        types: None,
        reply: PickReply::new(|_| {}),
    }
}

const FILES: PickKind = PickKind::Open {
    multiple: true,
    folders: false,
};
const FOLDERS: PickKind = PickKind::Open {
    multiple: false,
    folders: true,
};

#[test]
fn a_picker_is_titled_as_its_app_asks_or_by_what_it_is_for() {
    let title = |request| titlebar(&request).title.map(|title| title.to_string());
    assert_eq!(
        title(request("Upload photos", FILES)),
        Some("Upload photos".to_owned())
    );
    assert_eq!(title(request("", FILES)), Some("Open".to_owned()));
    assert_eq!(title(request("", FOLDERS)), Some("Open".to_owned()));
    let save = PickKind::Save {
        name: "a.txt".into(),
    };
    assert_eq!(title(request("", save)), Some("Save".to_owned()));
}

#[test]
fn the_button_says_what_the_app_asks_or_open_or_save() {
    let upload = PickRequest {
        accept_label: Some("Upload".to_owned()),
        ..request("", FILES)
    };
    assert_eq!(accept_label(&upload), "Upload");
    let mnemonic = PickRequest {
        accept_label: Some("_Save__as".to_owned()),
        ..request("", FILES)
    };
    assert_eq!(accept_label(&mnemonic), "Save_as");
    assert_eq!(accept_label(&request("", FOLDERS)), "Open");
    let save = PickKind::Save {
        name: "a.txt".into(),
    };
    assert_eq!(accept_label(&request("", save)), "Save");
}

#[test]
fn only_a_plain_name_is_taken_as_it_came() {
    for plain in ["notes.txt", " notes.txt", "~", "..notes"] {
        assert!(is_plain_name(OsStr::new(plain)), "{plain:?}");
    }
    for path in [
        "",
        ".",
        "..",
        "../notes.txt",
        "/etc/passwd",
        "a/b",
        "notes.txt/",
    ] {
        assert!(!is_plain_name(OsStr::new(path)), "{path:?}");
    }
}

#[test]
fn a_picker_starts_on_the_type_the_app_asks_for_or_its_first() {
    let with = |count: usize, chosen| PickRequest {
        filters: (0..count)
            .map(|at| nova_files_picker_api::FileFilter {
                name: at.to_string(),
                rules: Vec::new(),
            })
            .collect(),
        filter: chosen,
        ..request("", FILES)
    };
    assert_eq!(filter::first(&with(3, Some(2))), Some(2));
    assert_eq!(filter::first(&with(3, None)), Some(0));
    assert_eq!(filter::first(&with(3, Some(3))), Some(0));
    assert_eq!(filter::first(&with(0, None)), None);
    assert_eq!(filter::first(&with(0, Some(0))), None);
}

#[test]
fn the_menu_names_each_type_and_marks_the_one_chosen() {
    let request = PickRequest {
        filters: vec![nova_files_picker_api::FileFilter {
            name: "Text".to_owned(),
            rules: Vec::new(),
        }],
        ..request("", FILES)
    };
    assert_eq!(filter::label(&request, Some(0)), "Text");
    assert_eq!(filter::label(&request, None), "All files");
    assert_eq!(filter::label(&request, Some(1)), "All files");
    assert!(filter::is_chosen(Some(0), Some(0)));
    assert!(filter::is_chosen(None, None));
    assert!(!filter::is_chosen(Some(0), None));
}

#[test]
fn the_mode_follows_what_the_app_asked() {
    assert_eq!(Mode::of(&FILES), Mode::Files);
    assert_eq!(Mode::of(&FOLDERS), Mode::Folders);
    let save = PickKind::Save {
        name: "a.txt".into(),
    };
    assert_eq!(Mode::of(&save), Mode::Save);
}

#[test]
fn the_box_shows_one_name_as_it_is_and_several_in_quotes() {
    let names = |names: &[&str]| {
        names
            .iter()
            .map(|&name| name.to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(names_text(&names(&["a b.txt"])), "a b.txt");
    assert_eq!(
        names_text(&names(&["a.txt", "b.txt"])),
        "\"a.txt\" \"b.txt\""
    );
    assert_eq!(names_text(&[]), "");
}
