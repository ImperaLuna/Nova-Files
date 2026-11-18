use std::fs;
use std::path::PathBuf;

use nova_files_api::IconKind;
use nova_files_picker_api::{FileFilter, FilterRule};

use super::*;

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
    assert_eq!(
        of(&types, "backup.tar.gz"),
        Some("application/x-compressed-tar")
    );
    assert_eq!(of(&types, "single.gz"), Some("application/gzip"));
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
        .env("XDG_DATA_HOME", &dir.0)
        .env("XDG_DATA_DIRS", dir.0.join("none"))
        .status()
        .unwrap();
    assert!(status.success());
}

fn filter(rules: &[FilterRule]) -> FileFilter {
    FileFilter {
        name: "Some".to_owned(),
        rules: rules.to_vec(),
    }
}

#[test]
fn a_glob_rule_admits_by_name_alone() {
    let (_dir, types) = types("filter-glob");
    let pdfs = filter(&[FilterRule::Glob("*.pdf".to_owned())]);
    assert!(pdfs.admits(OsStr::new("a.PDF"), Some(&types)));
    assert!(!pdfs.admits(OsStr::new("a.md"), Some(&types)));
    assert!(!pdfs.admits(OsStr::new("a.xyz"), Some(&types)));
}

#[test]
fn a_mime_rule_admits_the_type_and_its_kinds() {
    let (_dir, types) = types("filter-mime");
    let text = filter(&[FilterRule::MimeType("text/plain".to_owned())]);
    assert!(text.admits(OsStr::new("notes.md"), Some(&types)));
    assert!(text.admits(OsStr::new("main.c"), Some(&types)));
    assert!(!text.admits(OsStr::new("photo.png"), Some(&types)));
    assert!(text.admits(OsStr::new("blob"), Some(&types)));
}

#[test]
fn any_rule_admits() {
    let (_dir, types) = types("filter-any");
    let both = filter(&[
        FilterRule::MimeType("image/png".to_owned()),
        FilterRule::Glob("*.pdf".to_owned()),
    ]);
    assert!(both.admits(OsStr::new("a.pdf"), Some(&types)));
    assert!(both.admits(OsStr::new("a.png"), Some(&types)));
    assert!(!both.admits(OsStr::new("a.md"), Some(&types)));
}

#[test]
fn a_bare_star_matches_every_name() {
    let dir = Dir::new("bare-star");
    dir.write(GLOBS_FILE, "80:text/plain:*\n50:image/png:*.png\n");
    let types = MimeTypes::load(std::slice::from_ref(&dir.0));
    assert_eq!(types.of(OsStr::new("notes")), Some("text/plain"));
    assert_eq!(types.of(OsStr::new("a.png")), Some("text/plain"));
}

#[test]
fn on_a_tie_the_first_data_folder_wins() {
    let user = Dir::new("tie-user");
    user.write(GLOBS_FILE, "50:text/x-personal:*.foo\n");
    let system = Dir::new("tie-system");
    system.write(GLOBS_FILE, "50:application/x-system:*.foo\n");
    let types = MimeTypes::load(&[user.0.clone(), system.0.clone()]);
    assert_eq!(
        types.of(OsStr::new("document.foo")),
        Some("text/x-personal")
    );
}

#[test]
fn a_type_is_called_by_its_untranslated_comment() {
    let (dir, types) = types("names");
    fs::create_dir_all(dir.0.join("mime/image")).unwrap();
    dir.write(
        "mime/image/png.xml",
        "<?xml version=\"1.0\"?>\n<mime-type type=\"image/png\">\n  \
         <comment xml:lang=\"sv\">PNG-bild</comment>\n  \
         <comment>PNG &amp; &lt;friends&gt;</comment>\n</mime-type>\n",
    );
    assert_eq!(types.name("image/png").as_deref(), Some("PNG & <friends>"));
    assert_eq!(
        types.name("image/x-old").as_deref(),
        Some("PNG & <friends>")
    );
    assert_eq!(types.name("image/jpeg"), None);
}

#[test]
fn a_subtype_with_dots_is_named_by_its_whole_file() {
    let (dir, types) = types("dotted");
    fs::create_dir_all(dir.0.join("mime/application")).unwrap();
    dir.write(
        "mime/application/vnd.oasis.opendocument.text.xml",
        "<comment>ODT document</comment>",
    );
    dir.write(
        "mime/application/vnd.oasis.opendocument.xml",
        "<comment>Wrong file</comment>",
    );
    assert_eq!(
        types
            .name("application/vnd.oasis.opendocument.text")
            .as_deref(),
        Some("ODT document")
    );
}

#[test]
fn a_name_that_is_not_a_type_reads_no_file() {
    let (dir, types) = types("not-types");
    dir.write("mime/globs.xml", "<comment>Not a type</comment>");
    dir.write("mime/x.xml", "<comment>Not a type</comment>");
    for name in ["globs", "x", "/globs", "mime/../globs", "../mime", "a/.."] {
        assert_eq!(types.name(name), None, "{name}");
    }
}

#[test]
fn a_type_file_without_a_name_gives_none() {
    let (dir, types) = types("no-comment");
    fs::create_dir_all(dir.0.join("mime/text")).unwrap();
    dir.write("mime/text/plain.xml", "<mime-type><comment> </comment>");
    dir.write("mime/text/markdown.xml", "<mime-type><comment>Open");
    assert_eq!(types.name("text/plain"), None);
    assert_eq!(types.name("text/markdown"), None);
}

#[test]
fn the_first_data_folder_names_a_type() {
    let user = Dir::new("name-user");
    let system = Dir::new("name-system");
    for (dir, name) in [(&user, "Mine"), (&system, "System's")] {
        fs::create_dir_all(dir.0.join("mime/text")).unwrap();
        dir.write("mime/text/plain.xml", &format!("<comment>{name}</comment>"));
    }
    let types = MimeTypes::load(&[user.0.clone(), system.0.clone()]);
    assert_eq!(types.name("text/plain").as_deref(), Some("Mine"));
}

fn icon_types(name: &str) -> (Dir, MimeTypes) {
    let (dir, _) = types(name);
    dir.write(
        GENERIC_ICONS_FILE,
        "application/gzip:package-x-generic\n\
         application/x-compressed-tar:package-x-generic\n\
         application/x-sharedlib:application-x-executable\n\
         text/x-csrc:text-x-script\n\
         application/vnd.oasis.opendocument.text:x-office-document\n",
    )
    .write(ICONS_FILE, "application/pdf:my-pdf\n");
    let types = MimeTypes::load(std::slice::from_ref(&dir.0));
    (dir, types)
}

#[test]
fn icon_names_go_from_the_type_to_its_generic_icon() {
    let (_dir, types) = icon_types("icon-names");
    assert_eq!(
        types.icon_names("application/x-compressed-tar"),
        [
            "application-x-compressed-tar",
            "package-x-generic",
            "application-gzip",
            "application-x-generic",
        ]
    );
    assert_eq!(
        types.icon_names("text/x-csrc"),
        [
            "text-x-csrc",
            "text-x-script",
            "text-plain",
            "text-x-generic"
        ]
    );
    assert_eq!(
        types.icon_names("application/pdf"),
        ["my-pdf", "application-pdf", "application-x-generic"]
    );
    assert_eq!(
        types.icon_names("image/x-old"),
        ["image-png", "image-x-generic"]
    );
}

#[test]
fn the_first_data_folder_names_a_types_icon() {
    let user = Dir::new("icon-user");
    let system = Dir::new("icon-system");
    user.write(GENERIC_ICONS_FILE, "application/zip:mine\n");
    system.write(
        GENERIC_ICONS_FILE,
        "application/zip:package-x-generic\n# a:b\n",
    );
    let types = MimeTypes::load(&[user.0.clone(), system.0.clone()]);
    assert_eq!(
        types.icon_names("application/zip"),
        ["application-zip", "mine", "application-x-generic"]
    );
}

#[test]
fn a_types_kind_comes_from_its_media_then_its_generic_icon() {
    let (_dir, types) = icon_types("icon-kinds");
    let kind = |mime| types.icon_kind(mime);
    assert_eq!(kind("image/png"), IconKind::Picture);
    assert_eq!(kind("video/mp4"), IconKind::Video);
    assert_eq!(kind("audio/flac"), IconKind::Audio);
    assert_eq!(kind("text/x-csrc"), IconKind::Text);
    assert_eq!(kind("application/x-compressed-tar"), IconKind::Archive);
    assert_eq!(kind("application/x-sharedlib"), IconKind::Program);
    assert_eq!(
        kind("application/vnd.oasis.opendocument.text"),
        IconKind::Text
    );
    assert_eq!(kind("application/pdf"), IconKind::File);
    assert_eq!(kind(ANY_FILE), IconKind::File);
}
