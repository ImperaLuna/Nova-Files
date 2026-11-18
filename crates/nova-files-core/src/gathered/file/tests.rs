//! Reading and editing gathered folders text.

use super::*;

const HOME: &str = "/home/me";

fn home() -> &'static Path {
    Path::new(HOME)
}

fn folder(path: &str, holds: Holds, name: Option<&str>) -> GatheredFolder {
    GatheredFolder {
        path: PathBuf::from(path),
        holds,
        name: name.map(str::to_owned),
    }
}

fn read_ok(text: &str) -> (Gathered, Vec<String>) {
    read(text, home()).unwrap()
}

fn paths(folders: &[GatheredFolder]) -> Vec<&str> {
    folders
        .iter()
        .map(|folder| folder.path.to_str().unwrap())
        .collect()
}

fn changed(edit: Edit) -> String {
    match edit {
        Edit::Changed(text) => text,
        other => panic!("not changed: {other:?}"),
    }
}

#[test]
fn an_empty_file_gathers_nothing() {
    assert_eq!(read_ok(""), (Gathered::default(), Vec::new()));
}

#[test]
fn every_key_reads() {
    let text = r#"
[[projects]]
path = "~/Projects"
holds = "many"

[[projects]]
path = "~/Skills"
holds = "one"

[[games]]
path = "/mnt/games/common"
name = "Games SSD"

[[games]]
path = "~"
"#;
    let (gathered, problems) = read_ok(text);
    assert_eq!(problems, Vec::<String>::new());
    assert_eq!(
        *gathered.projects,
        [
            folder("/home/me/Projects", Holds::Many, None),
            folder("/home/me/Skills", Holds::One, None),
        ]
    );
    assert_eq!(
        *gathered.games,
        [
            folder("/mnt/games/common", Holds::Many, Some("Games SSD")),
            folder("/home/me", Holds::Many, None),
        ]
    );
}

#[test]
fn an_inline_list_reads_too() {
    let (gathered, problems) =
        read_ok(r#"projects = [{ path = "~/a" }, { path = "/b", holds = "one" }]"#);
    assert_eq!(problems, Vec::<String>::new());
    assert_eq!(paths(&gathered.projects), ["/home/me/a", "/b"]);
    assert_eq!(gathered.projects[1].holds, Holds::One);
}

#[test]
fn a_mistake_costs_only_its_own_entry() {
    let text = r#"
colour = "red"

[[projects]]
path = "relative/folder"

[[projects]]
path = "~/kept"
holds = "some"

[[projects]]
holds = "one"

[[projects]]
path = "/kept"
name = 3
size = "big"

[[games]]
path = 7
"#;
    let (gathered, problems) = read_ok(text);
    assert_eq!(paths(&gathered.projects), ["/kept"]);
    assert_eq!(gathered.projects[0].name, None);
    assert_eq!(gathered.games.len(), 0);
    assert_eq!(problems.len(), 7, "{problems:?}");
    assert!(problems[0].contains("colour"));
}

#[test]
fn a_place_that_is_no_list_is_reported() {
    let (gathered, problems) = read_ok("projects = \"~/Projects\"");
    assert_eq!(gathered.projects.len(), 0);
    assert_eq!(problems.len(), 1);
    // In an inline list, an entry that is no table costs only itself.
    let text = r#"projects = [{ path = "/a" }, "/b", { path = "/c" }]"#;
    let (gathered, problems) = read_ok(text);
    assert_eq!(paths(&gathered.projects), ["/a", "/c"]);
    assert_eq!(problems.len(), 1);
    let new = changed(add(
        text,
        home(),
        Gathering::Projects,
        &folder("/d", Holds::Many, None),
        Some(1),
    ));
    assert_eq!(paths(&read_ok(&new).0.projects), ["/a", "/d", "/c"]);
}

#[test]
fn a_folder_listed_twice_counts_once() {
    let text =
        "[[projects]]\npath = \"~/a\"\n[[projects]]\npath = \"/home/me/a/\"\nholds = \"one\"\n";
    let (gathered, _) = read_ok(text);
    assert_eq!(
        *gathered.projects,
        [folder("/home/me/a", Holds::Many, None)]
    );
}

#[test]
fn text_that_isnt_toml_is_an_error() {
    assert!(read("[[projects", home()).is_err());
}

#[test]
fn adding_keeps_comments_and_writes_home_as_a_tilde() {
    let text = "# mine\n[[projects]]\npath = \"~/a\" # first\n";
    let new = changed(add(
        text,
        home(),
        Gathering::Projects,
        &folder("/home/me/b", Holds::One, None),
        None,
    ));
    assert!(new.starts_with("# mine\n"), "{new}");
    assert!(new.contains("# first"), "{new}");
    assert!(new.contains("path = \"~/b\""), "{new}");
    assert!(new.contains("holds = \"one\""), "{new}");
    let (gathered, problems) = read_ok(&new);
    assert_eq!(problems, Vec::<String>::new());
    assert_eq!(paths(&gathered.projects), ["/home/me/a", "/home/me/b"]);
}

#[test]
fn adding_to_an_empty_file_makes_the_place() {
    let new = changed(add(
        "",
        home(),
        Gathering::Games,
        &folder("/mnt/games", Holds::Many, Some("SSD")),
        None,
    ));
    let (gathered, _) = read_ok(&new);
    assert_eq!(
        *gathered.games,
        [folder("/mnt/games", Holds::Many, Some("SSD"))]
    );
    // What holds many by default isn't written out.
    assert!(!new.contains(HOLDS), "{new}");
    assert_eq!(gathered.projects.len(), 0);
}

#[test]
fn adding_at_a_position_counts_only_the_folders_read() {
    let text =
        "[[projects]]\npath = \"bad\"\n[[projects]]\npath = \"/a\"\n[[projects]]\npath = \"/c\"\n";
    let new = changed(add(
        text,
        home(),
        Gathering::Projects,
        &folder("/b", Holds::Many, None),
        Some(1),
    ));
    assert_eq!(paths(&read_ok(&new).0.projects), ["/a", "/b", "/c"]);
    let first = changed(add(
        text,
        home(),
        Gathering::Projects,
        &folder("/z", Holds::Many, None),
        Some(0),
    ));
    assert_eq!(paths(&read_ok(&first).0.projects), ["/z", "/a", "/c"]);
    let past = changed(add(
        text,
        home(),
        Gathering::Projects,
        &folder("/z", Holds::Many, None),
        Some(9),
    ));
    assert_eq!(paths(&read_ok(&past).0.projects), ["/a", "/c", "/z"]);
}

#[test]
fn adding_into_an_inline_list_keeps_it_inline() {
    let text = "projects = [{ path = \"/a\" }, { path = \"/c\" }]\n";
    let new = changed(add(
        text,
        home(),
        Gathering::Projects,
        &folder("/b", Holds::Many, None),
        Some(1),
    ));
    assert!(new.starts_with("projects = ["), "{new}");
    assert_eq!(paths(&read_ok(&new).0.projects), ["/a", "/b", "/c"]);
}

#[test]
fn adding_a_folder_already_there_changes_nothing() {
    let text = "[[projects]]\npath = \"~/a\"\n";
    let again = add(
        text,
        home(),
        Gathering::Projects,
        &folder("/home/me/a", Holds::One, None),
        None,
    );
    assert_eq!(again, Edit::Unchanged);
    // The other place is another list.
    let games = add(
        text,
        home(),
        Gathering::Games,
        &folder("/home/me/a", Holds::Many, None),
        None,
    );
    assert!(matches!(games, Edit::Changed(_)));
}

#[test]
fn a_change_never_overwrites_what_it_cant_read() {
    let not_toml = add(
        "[[projects",
        home(),
        Gathering::Projects,
        &folder("/a", Holds::Many, None),
        None,
    );
    assert!(matches!(not_toml, Edit::Refused(_)));
    let not_a_list = add(
        "projects = 3",
        home(),
        Gathering::Projects,
        &folder("/a", Holds::Many, None),
        None,
    );
    assert!(matches!(not_a_list, Edit::Refused(_)));
    assert!(matches!(
        remove("[[games", home(), Gathering::Games, Path::new("/a")),
        Edit::Refused(_)
    ));
}

#[test]
fn a_relative_path_is_refused() {
    let refused = add(
        "",
        home(),
        Gathering::Projects,
        &folder("Code", Holds::Many, None),
        None,
    );
    assert!(matches!(refused, Edit::Refused(_)));
}

#[test]
fn a_path_that_isnt_text_is_refused() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;

    let path = Path::new(OsStr::from_bytes(b"/bad-\xff"));
    let refused = add(
        "",
        home(),
        Gathering::Projects,
        &GatheredFolder {
            path: path.to_owned(),
            holds: Holds::Many,
            name: None,
        },
        None,
    );
    assert!(matches!(refused, Edit::Refused(_)));
}

#[test]
fn removing_takes_every_entry_naming_the_folder() {
    let text = "[[projects]]\npath = \"~/a\"\n# mine\n[[projects]]\npath = \"/b\"\n[[projects]]\npath = \"/home/me/a\"\n";
    let new = changed(remove(
        text,
        home(),
        Gathering::Projects,
        Path::new("/home/me/a"),
    ));
    // A comment goes with the entry it sits above, and stays with others.
    assert!(new.starts_with("# mine\n"), "{new}");
    assert_eq!(paths(&read_ok(&new).0.projects), ["/b"]);
    assert_eq!(
        remove(&new, home(), Gathering::Projects, Path::new("/home/me/a")),
        Edit::Unchanged
    );
    assert_eq!(
        remove(text, home(), Gathering::Games, Path::new("/b")),
        Edit::Unchanged
    );
    let inline = changed(remove(
        "games = [{ path = \"/x\" }, { path = \"/y\" }]",
        home(),
        Gathering::Games,
        Path::new("/x"),
    ));
    assert_eq!(paths(&read_ok(&inline).0.games), ["/y"]);
}

#[test]
fn a_new_file_holds_each_folder_given() {
    let text = new_file(
        home(),
        &[
            (
                Gathering::Projects,
                folder("/home/me/Projects", Holds::Many, None),
            ),
            (
                Gathering::Games,
                folder(
                    "/home/me/.local/share/Steam/steamapps/common",
                    Holds::Many,
                    Some("Steam"),
                ),
            ),
        ],
    );
    let (gathered, problems) = read_ok(&text);
    assert_eq!(problems, Vec::<String>::new());
    assert_eq!(paths(&gathered.projects), ["/home/me/Projects"]);
    assert_eq!(gathered.games[0].name.as_deref(), Some("Steam"));
    assert_eq!(new_file(home(), &[]), "");
}
