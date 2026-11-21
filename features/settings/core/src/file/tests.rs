use std::collections::BTreeMap;

use super::*;

fn keys(entries: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
    entries
        .iter()
        .map(|(action, keys)| {
            (
                (*action).to_owned(),
                keys.iter().map(|key| (*key).to_owned()).collect(),
            )
        })
        .collect()
}

#[test]
fn an_empty_file_is_all_defaults() {
    assert_eq!(read(""), (Settings::default(), Vec::new()));
}

#[test]
fn every_key_reads() {
    let text = r#"
appearance = "light"
thumbnails = false
hidden_files = true
folder_sizes = false

[keys]
"go.back" = ["alt-left", "backspace"]
"go.up" = []
"#;
    let (settings, problems) = read(text);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        settings,
        Settings {
            appearance: Appearance::Light,
            thumbnails: Thumbnails::Hidden,
            hidden_files: HiddenFiles::Shown,
            folder_sizes: FolderSizes::Left,
            keys: keys(&[("go.back", &["alt-left", "backspace"]), ("go.up", &[])]),
        }
    );
    assert_eq!(read("appearance = \"dark\"").0.appearance, Appearance::Dark);
    assert_eq!(
        read("appearance = \"system\""),
        (Settings::default(), Vec::new())
    );
    assert_eq!(read("thumbnails = true").0.thumbnails, Thumbnails::Shown);
    assert_eq!(
        read("hidden_files = false").0.hidden_files,
        HiddenFiles::Left
    );
    assert_eq!(
        read("folder_sizes = true").0.folder_sizes,
        FolderSizes::Measured
    );
}

#[test]
fn a_bad_value_costs_only_its_own_key() {
    let text = r#"
appearance = "blue"
thumbnails = "yes"
hidden_files = 1
folder_sizes = "on"
colour = "dark"
[keys]
"go.back" = ["alt-left", 3]
"go.up" = ["alt-up"]
"#;
    let (settings, problems) = read(text);
    assert_eq!(settings.appearance, Appearance::System);
    assert_eq!(settings.thumbnails, Thumbnails::Shown);
    assert_eq!(settings.hidden_files, HiddenFiles::Left);
    assert_eq!(settings.folder_sizes, FolderSizes::Measured);
    assert_eq!(settings.keys, keys(&[("go.up", &["alt-up"])]));
    assert_eq!(problems.len(), 6, "{problems:?}");
    assert!(problems.iter().any(|problem| problem.contains("colour")));
    let (settings, problems) = read("keys = 3\nthumbnails = false");
    assert_eq!(settings.thumbnails, Thumbnails::Hidden);
    assert_eq!(problems.len(), 1);
}

#[test]
fn text_that_isnt_toml_is_all_defaults() {
    let (settings, problems) = read("appearance = ");
    assert_eq!(settings, Settings::default());
    assert_eq!(problems.len(), 1);
}

#[test]
fn an_edit_keeps_comments_and_other_keys() {
    let text = "# mine\nappearance = \"dark\" # night owl\nmystery = 1\n";
    let edited = edit(text, &SettingChange::Appearance(Appearance::Light)).unwrap();
    assert_eq!(
        edited,
        "# mine\nappearance = \"light\" # night owl\nmystery = 1\n"
    );
    let edited = edit(&edited, &SettingChange::Thumbnails(Thumbnails::Hidden)).unwrap();
    assert_eq!(read(&edited).0.thumbnails, Thumbnails::Hidden);
    assert!(edited.starts_with("# mine\n"));
}

#[test]
fn a_default_value_is_only_written_over_a_key_already_there() {
    assert_eq!(
        edit("", &SettingChange::Appearance(Appearance::System)).unwrap(),
        ""
    );
    assert_eq!(
        edit("", &SettingChange::Thumbnails(Thumbnails::Shown)).unwrap(),
        ""
    );
    assert_eq!(
        edit(
            "thumbnails = false\n",
            &SettingChange::Thumbnails(Thumbnails::Shown)
        )
        .unwrap(),
        "thumbnails = true\n"
    );
    assert_eq!(
        edit("", &SettingChange::Thumbnails(Thumbnails::Hidden)).unwrap(),
        "thumbnails = false\n"
    );
    assert_eq!(
        edit("", &SettingChange::HiddenFiles(HiddenFiles::Left)).unwrap(),
        ""
    );
    assert_eq!(
        edit("", &SettingChange::HiddenFiles(HiddenFiles::Shown)).unwrap(),
        "hidden_files = true\n"
    );
    assert_eq!(
        edit(
            "hidden_files = true\n",
            &SettingChange::HiddenFiles(HiddenFiles::Left)
        )
        .unwrap(),
        "hidden_files = false\n"
    );
    assert_eq!(
        edit("", &SettingChange::FolderSizes(FolderSizes::Measured)).unwrap(),
        ""
    );
    assert_eq!(
        edit("", &SettingChange::FolderSizes(FolderSizes::Left)).unwrap(),
        "folder_sizes = false\n"
    );
}

#[test]
fn key_overrides_are_added_and_removed() {
    let back = SettingChange::Keys {
        action: "go.back".to_owned(),
        keys: Some(vec!["alt-left".to_owned()]),
    };
    let edited = edit("", &back).unwrap();
    assert_eq!(read(&edited).0.keys, keys(&[("go.back", &["alt-left"])]));
    let edited_over = edit("keys = 3\n", &back).unwrap();
    assert_eq!(
        read(&edited_over).0.keys,
        keys(&[("go.back", &["alt-left"])])
    );
    let dropped = edit(
        &edited,
        &SettingChange::Keys {
            action: "go.back".to_owned(),
            keys: None,
        },
    )
    .unwrap();
    assert_eq!(dropped, "");
    let none = SettingChange::Keys {
        action: "go.back".to_owned(),
        keys: None,
    };
    assert_eq!(
        edit("thumbnails = false\n", &none).unwrap(),
        "thumbnails = false\n"
    );
    let two = edit("[keys]\n\"go.back\" = []\n\"go.up\" = []\n", &none).unwrap();
    assert_eq!(read(&two).0.keys, keys(&[("go.up", &[])]));
}

#[test]
fn text_that_isnt_toml_is_never_edited() {
    assert_eq!(
        edit(
            "appearance = ",
            &SettingChange::Appearance(Appearance::Dark)
        ),
        None
    );
}
