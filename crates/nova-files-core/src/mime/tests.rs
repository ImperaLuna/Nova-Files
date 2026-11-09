use std::fs;
use std::path::PathBuf;

use super::*;

/// A data folder of its own, removed afterwards.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir = env::temp_dir().join(format!("nova-files-mime-{name}-{}", std::process::id()));
        fs::create_dir_all(dir.join("mime")).unwrap();
        Self(dir)
    }

    fn write(&self, file: &str, text: &str) -> &Self {
        fs::write(self.0.join(file), text).unwrap();
        self
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

const GLOBS: &str = "\
# a comment
50:application/pdf:*.pdf
50:application/gzip:*.gz
50:application/x-compressed-tar:*.tar.gz
50:text/x-makefile:Makefile
50:text/x-makefile:makefile
10:text/x-readme:README*
50:text/x-troff-man:*.[1-9]
50:text/x-c++src:*.C:cs
50:text/x-csrc:*.c
50:text/markdown:*.md
60:text/x-markdown-special:*.special.md
50:image/png:*.png
50:image/x-old:*.old
50:text/x-gone:__NOGLOBS__
";

fn types(name: &str) -> (Dir, MimeTypes) {
    let dir = Dir::new(name);
    dir.write(GLOBS_FILE, GLOBS)
        .write(
            SUBCLASSES_FILE,
            "text/x-csrc text/plain\napplication/x-compressed-tar application/gzip\n",
        )
        .write(
            ALIASES_FILE,
            "image/x-old image/png\ntext/x-markdown text/markdown\n",
        );
    let types = MimeTypes::load(std::slice::from_ref(&dir.0));
    (dir, types)
}

fn of<'a>(types: &'a MimeTypes, name: &str) -> Option<&'a str> {
    types.of(OsStr::new(name))
}

#[test]
fn a_name_gets_the_type_of_its_best_pattern() {
    let (_dir, types) = types("of");
    assert_eq!(of(&types, "report.PDF"), Some("application/pdf"));
    // The longer pattern wins a tie on weight.
    assert_eq!(
        of(&types, "backup.tar.gz"),
        Some("application/x-compressed-tar")
    );
    assert_eq!(of(&types, "single.gz"), Some("application/gzip"));
    // A higher weight wins over a longer pattern too.
    assert_eq!(of(&types, "a.special.md"), Some("text/x-markdown-special"));
    assert_eq!(of(&types, "Makefile"), Some("text/x-makefile"));
    assert_eq!(of(&types, "README.txt"), Some("text/x-readme"));
    assert_eq!(of(&types, "ls.1"), Some("text/x-troff-man"));
    assert_eq!(of(&types, "notes"), None);
    assert_eq!(of(&types, "x.gone"), None);
}

#[test]
fn a_case_sensitive_pattern_keeps_its_case() {
    let (_dir, types) = types("case");
    assert_eq!(of(&types, "main.C"), Some("text/x-c++src"));
    assert_eq!(of(&types, "main.c"), Some("text/x-csrc"));
}

#[test]
fn kinds_follow_subclasses_aliases_and_media() {
    let (_dir, types) = types("kinds");
    assert!(types.is_a("text/x-csrc", "text/plain"));
    // Every text type is plain text, listed or not.
    assert!(types.is_a("text/markdown", "text/plain"));
    assert!(types.is_a("application/x-compressed-tar", "application/gzip"));
    assert!(!types.is_a("application/gzip", "application/x-compressed-tar"));
    assert!(types.is_a("image/x-old", "image/png"));
    assert!(types.is_a("text/markdown", "text/x-markdown"));
    assert!(types.is_a("image/png", "image/*"));
    assert!(!types.is_a("text/plain", "image/*"));
    assert!(types.is_a("application/pdf", "application/octet-stream"));
    assert!(!types.is_a("application/pdf", "text/plain"));
}

#[test]
fn a_loop_of_subclasses_ends() {
    let dir = Dir::new("loop");
    dir.write(SUBCLASSES_FILE, "a/x a/y\na/y a/x\n");
    let types = MimeTypes::load(std::slice::from_ref(&dir.0));
    assert!(types.is_a("a/x", "a/y"));
    assert!(!types.is_a("a/x", "a/z"));
}

#[test]
fn every_data_folder_is_read() {
    let first = Dir::new("first");
    first.write(GLOBS_FILE, "50:application/pdf:*.pdf\n");
    let second = Dir::new("second");
    second.write(GLOBS_FILE, "50:image/png:*.png\n");
    let types = MimeTypes::load(&[first.0.clone(), PathBuf::from("/nowhere"), second.0.clone()]);
    assert_eq!(of(&types, "a.pdf"), Some("application/pdf"));
    assert_eq!(of(&types, "a.png"), Some("image/png"));
}

#[test]
fn the_data_folders_follow_the_xdg_spec() {
    let vars = |values: &'static [(&'static str, &'static str)]| {
        move |name: &str| {
            values
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        }
    };
    assert_eq!(
        data_dirs(vars(&[("HOME", "/home/me")])),
        ["/home/me/.local/share", "/usr/local/share", "/usr/share"].map(PathBuf::from)
    );
    assert_eq!(
        data_dirs(vars(&[
            ("HOME", "/home/me"),
            ("XDG_DATA_HOME", "/data"),
            (
                "XDG_DATA_DIRS",
                "/run/current-system/sw/share:/nix/profile/share"
            ),
        ])),
        [
            "/data",
            "/run/current-system/sw/share",
            "/nix/profile/share"
        ]
        .map(PathBuf::from)
    );
    assert_eq!(
        data_dirs(vars(&[("XDG_DATA_HOME", ""), ("XDG_DATA_DIRS", "")])),
        ["/usr/local/share", "/usr/share"].map(PathBuf::from)
    );
}

#[test]
fn a_name_gets_the_type_its_alias_stands_for() {
    let (_dir, types) = types("alias-of");
    assert_eq!(types.of(OsStr::new("scan.old")), Some("image/png"));
}

#[test]
fn ancestry_lists_the_type_then_its_parents_nearest_first() {
    let (_dir, types) = types("ancestry");
    assert_eq!(
        types.ancestry("application/x-compressed-tar"),
        ["application/x-compressed-tar", "application/gzip"]
    );
    assert_eq!(
        types.ancestry("text/x-markdown"),
        ["text/markdown", "text/plain"]
    );
    assert_eq!(types.ancestry("text/x-csrc"), ["text/x-csrc", "text/plain"]);
}

#[test]
fn a_type_with_no_globs_matches_no_name() {
    let (_dir, types) = types("no-globs");
    assert_eq!(types.of(OsStr::new("__NOGLOBS__")), None);
}

#[test]
fn plain_names_are_looked_up_not_matched() {
    let (_dir, types) = types("literals");
    assert!(types.literals.contains_key("makefile"));
    assert!(types.others.iter().all(|glob| glob.pattern != "Makefile"));
}

#[test]
fn a_large_globs_file_is_read_to_its_end() {
    let dir = Dir::new("large");
    let padding = "# padding\n".repeat(200_000);
    dir.write(GLOBS_FILE, &format!("{padding}50:application/pdf:*.pdf\n"));
    let types = MimeTypes::load(std::slice::from_ref(&dir.0));
    assert_eq!(types.of(OsStr::new("a.pdf")), Some("application/pdf"));
}

/// Run by `the_environment_names_the_data_folders`, in a process of its
/// own with the environment set, since a test can't safely change its own.
#[test]
#[ignore = "run by the_environment_names_the_data_folders"]
fn from_env_child() {
    let types = MimeTypes::from_env();
    assert_eq!(types.of(OsStr::new("a.pdf")), Some("application/pdf"));
}

#[test]
fn the_environment_names_the_data_folders() {
    let dir = Dir::new("env");
    dir.write(GLOBS_FILE, GLOBS);
    let status = std::process::Command::new(env::current_exe().unwrap())
        .args(["--exact", "mime::tests::from_env_child", "--ignored"])
        .env(DATA_HOME_VAR, &dir.0)
        .env(DATA_DIRS_VAR, dir.0.join("none"))
        .status()
        .unwrap();
    assert!(status.success());
}
