use std::fs;
use std::path::PathBuf;

use nova_files_test_folder::TestFolder;

use super::index::{Folder, Sizes};
use super::*;

struct Base(TestFolder);

impl Base {
    fn new(name: &str) -> Self {
        Self(TestFolder::new(&format!("icons-{name}")))
    }

    fn index(&self, theme: &str, text: &str) -> &Self {
        fs::create_dir_all(self.0.join(theme)).unwrap();
        fs::write(self.0.join(theme).join(INDEX_FILE), text).unwrap();
        self
    }

    fn icon(&self, theme: &str, file: &str) -> &Self {
        let path = self.0.join(theme).join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, file).unwrap();
        self
    }

    fn path(&self, theme: &str, file: &str) -> PathBuf {
        self.0.join(theme).join(file)
    }
}

const MINE: &str = "\
[Icon Theme]
Name=Mine
Inherits=Parent
Directories=16x16/mimetypes,48x48/mimetypes,48x48/apps,scalable/mimetypes

[16x16/mimetypes]
Size=16
Context=MimeTypes
Type=Fixed

[48x48/mimetypes]
Size=48
Context=MimeTypes
Type=Fixed

[48x48/apps]
Size=48
Context=Applications
Type=Fixed

[scalable/mimetypes]
Size=16
MinSize=8
MaxSize=512
Context=MimeTypes
Type=Scalable
";

const PARENT: &str = "\
[Icon Theme]
Inherits=Mine,hicolor
Directories=mimes

[mimes]
Size=48
";

const HICOLOR_INDEX: &str = "\
[Icon Theme]
Directories=48x48/mimetypes

[48x48/mimetypes]
Size=48
Context=MimeTypes
Type=Threshold
";

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

const SIZE: u32 = 48;

fn themes(chosen: Option<&str>, bases: &[&Base]) -> IconThemes {
    let bases = bases
        .iter()
        .map(|base| base.0.path().to_path_buf())
        .collect();
    IconThemes::new(chosen.map(str::to_owned), bases, SIZE, 1)
}

#[test]
fn the_folder_made_for_the_size_wins() {
    let base = Base::new("size");
    base.index("Mine", MINE)
        .icon("Mine", "16x16/mimetypes/image-png.png")
        .icon("Mine", "48x48/mimetypes/image-png.png")
        .icon("Mine", "scalable/mimetypes/text-plain.svg")
        .icon("Mine", "48x48/apps/text-html.png");
    let mut themes = themes(Some("Mine"), &[&base]);

    let found = themes.find(&names(&["image-png"]), Some);
    assert_eq!(
        found,
        Some(base.path("Mine", "48x48/mimetypes/image-png.png"))
    );
    let scalable = themes.find(&names(&["text-plain"]), Some);
    assert_eq!(
        scalable,
        Some(base.path("Mine", "scalable/mimetypes/text-plain.svg"))
    );
    assert_eq!(themes.find(&names(&["text-html"]), Some), None);
}

#[test]
fn a_long_index_is_read_whole() {
    let base = Base::new("long");
    let comment = "x".repeat(64 * 1024);
    let index = MINE.replacen(
        "[Icon Theme]\n",
        &format!("[Icon Theme]\nComment={comment}\n"),
        1,
    );
    base.index("Mine", &index)
        .icon("Mine", "48x48/mimetypes/image-png.png");
    let mut themes = themes(Some("Mine"), &[&base]);

    assert!(themes.find(&names(&["image-png"]), Some).is_some());
}

#[test]
fn a_themes_generic_icon_beats_its_parents_exact_one() {
    let base = Base::new("generic");
    base.index("Mine", MINE)
        .index("Parent", PARENT)
        .icon("Mine", "48x48/mimetypes/image-x-generic.png")
        .icon("Parent", "mimes/image-png.svg")
        .icon("Parent", "mimes/text-plain.png");
    let mut themes = themes(Some("Mine"), &[&base]);

    let found = themes.find(&names(&["image-png", "image-x-generic"]), Some);
    assert_eq!(
        found,
        Some(base.path("Mine", "48x48/mimetypes/image-x-generic.png"))
    );
    let inherited = themes.find(&names(&["text-plain"]), Some);
    assert_eq!(inherited, Some(base.path("Parent", "mimes/text-plain.png")));
}

#[test]
fn hicolor_comes_last_even_when_no_theme_is_chosen() {
    let base = Base::new("hicolor");
    base.index("Mine", MINE)
        .index("Parent", PARENT)
        .index(HICOLOR, HICOLOR_INDEX)
        .icon("Parent", "mimes/text-plain.png")
        .icon(HICOLOR, "48x48/mimetypes/text-plain.png")
        .icon(HICOLOR, "48x48/mimetypes/audio-x-generic.png");

    let mut chosen = themes(Some("Mine"), &[&base]);
    let found = chosen.find(&names(&["text-plain"]), Some);
    assert_eq!(found, Some(base.path("Parent", "mimes/text-plain.png")));
    let last = chosen.find(&names(&["audio-x-generic"]), Some);
    assert_eq!(
        last,
        Some(base.path(HICOLOR, "48x48/mimetypes/audio-x-generic.png"))
    );

    let mut unset = themes(None, &[&base]);
    let found = unset.find(&names(&["text-plain"]), Some);
    assert_eq!(
        found,
        Some(base.path(HICOLOR, "48x48/mimetypes/text-plain.png"))
    );
    assert_eq!(unset.find(&names(&["video-x-generic"]), Some), None);
}

#[test]
fn a_theme_spreads_over_several_folders() {
    let user = Base::new("spread-user");
    let system = Base::new("spread-system");
    user.index("Mine", MINE)
        .icon("Mine", "48x48/mimetypes/a.png");
    system
        .index("Mine", "[Icon Theme]\nDirectories=\n")
        .icon("Mine", "48x48/mimetypes/b.svg");
    let mut themes = themes(Some("Mine"), &[&user, &system]);

    assert_eq!(
        themes.find(&names(&["a"]), Some),
        Some(user.path("Mine", "48x48/mimetypes/a.png"))
    );
    assert_eq!(
        themes.find(&names(&["b"]), Some),
        Some(system.path("Mine", "48x48/mimetypes/b.svg"))
    );
}

#[test]
fn a_missing_theme_or_a_loop_of_themes_ends() {
    let base = Base::new("loop");
    base.index("Mine", MINE)
        .index("Parent", PARENT)
        .icon("Parent", "mimes/x.png");
    let mut looped = themes(Some("Parent"), &[&base]);
    assert_eq!(
        looped.find(&names(&["x"]), Some),
        Some(base.path("Parent", "mimes/x.png"))
    );
    assert_eq!(looped.find(&names(&["y"]), Some), None);
    let mut tried = 0;
    looped.find(&names(&["x"]), |_| {
        tried += 1;
        None::<PathBuf>
    });
    assert_eq!(tried, 1);

    let mut missing = themes(Some("Gone"), &[&base]);
    assert_eq!(missing.find(&names(&["x"]), Some), None);
}

#[test]
fn an_index_lists_its_parents_and_file_type_folders() {
    let read = index::parse(MINE);
    assert_eq!(read.inherits, ["Parent"]);
    let paths: Vec<_> = read
        .folders
        .iter()
        .map(|folder| folder.path.as_str())
        .collect();
    assert_eq!(
        paths,
        ["16x16/mimetypes", "48x48/mimetypes", "scalable/mimetypes"]
    );
    assert_eq!(read.folders[2].sizes, Sizes::Scalable { min: 8, max: 512 });
    let odd = index::parse(
        "[Icon Theme]\nDirectories=a,a,b\nScaledDirectories=a@2,c\n\
         [a]\nSize=x\nSize=24\n[a@2]\nSize=24\nScale=2\n[c]\nContext=MimeTypes\n",
    );
    let paths: Vec<_> = odd
        .folders
        .iter()
        .map(|folder| folder.path.as_str())
        .collect();
    assert_eq!(paths, ["a", "a@2"]);
    assert_eq!(odd.folders[0].sizes, Sizes::Threshold(2));
    assert_eq!(odd.folders[1].scale, 2);
    let kinds = index::parse(
        "[Icon Theme]\nDirectories=f,t\n[f]\nSize=16\nType=Fixed\n\
         [t]\nSize=32\nType=Threshold\nThreshold=5\n",
    );
    assert_eq!(kinds.folders[0].sizes, Sizes::Fixed);
    assert_eq!(kinds.folders[1].sizes, Sizes::Threshold(5));
}

#[test]
fn folders_measure_their_distance_from_a_size() {
    let folder = |size, scale, sizes| Folder {
        path: String::new(),
        size,
        scale,
        sizes,
    };
    let fixed = folder(32, 1, Sizes::Fixed);
    assert!(fixed.serves(32, 1));
    assert!(!fixed.serves(32, 2));
    assert_eq!(fixed.distance(48, 1), 16);
    assert_eq!(fixed.distance(16, 1), 16);
    let doubled = folder(24, 2, Sizes::Fixed);
    assert!(!doubled.serves(48, 1));
    assert_eq!(doubled.distance(48, 1), 0);
    let scalable = folder(16, 1, Sizes::Scalable { min: 8, max: 32 });
    assert!(scalable.serves(32, 1));
    assert_eq!(scalable.distance(48, 1), 16);
    assert_eq!(scalable.distance(4, 1), 4);
    let threshold = folder(48, 1, Sizes::Threshold(2));
    assert!(threshold.serves(46, 1));
    assert!(threshold.serves(50, 1));
    assert!(!threshold.serves(51, 1));
    assert_eq!(threshold.distance(53, 1), 3);
}

#[test]
fn the_chosen_theme_comes_from_the_first_settings_file_naming_one() {
    let config = Base::new("config");
    let write = |file: &str, text: &str| {
        let path = config.0.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    };
    assert_eq!(name::chosen(config.0.path()), None);
    let comment = "x".repeat(64 * 1024);
    write(
        "kdeglobals",
        &format!("# {comment}\n[General]\nTheme=No\n[Icons]\nTheme=breeze-dark\n"),
    );
    assert_eq!(
        name::chosen(config.0.path()).as_deref(),
        Some("breeze-dark")
    );
    write(
        "gtk-4.0/settings.ini",
        "[Settings]\ngtk-icon-theme-name = \"Papirus\"\n",
    );
    assert_eq!(name::chosen(config.0.path()).as_deref(), Some("Papirus"));
    write(
        "gtk-3.0/settings.ini",
        "[Settings]\ngtk-icon-theme-name=../x\n",
    );
    assert_eq!(name::chosen(config.0.path()).as_deref(), Some("Papirus"));
    write(
        "gtk-3.0/settings.ini",
        "[Settings]\ngtk-icon-theme-name=Adwaita\n",
    );
    assert_eq!(name::chosen(config.0.path()).as_deref(), Some("Adwaita"));
    for not_a_name in ["", " ", "..", ".", "a/b", "''"] {
        assert_eq!(name::checked(not_a_name), None, "{not_a_name}");
    }
}

#[test]
fn icons_live_in_home_then_each_data_folder() {
    let var = |name: &str| match name {
        "HOME" => Some("/h".into()),
        "XDG_DATA_DIRS" => Some("/a:/b".into()),
        _ => None,
    };
    let expected: Vec<PathBuf> = ["/h/.icons", "/h/.local/share/icons", "/a/icons", "/b/icons"]
        .into_iter()
        .map(PathBuf::from)
        .collect();
    assert_eq!(bases(var), expected);
}

#[test]
fn an_unusable_file_lets_the_search_go_on() {
    let base = Base::new("unusable");
    base.index("Mine", MINE)
        .icon("Mine", "48x48/mimetypes/image-png.png")
        .icon("Mine", "scalable/mimetypes/image-png.svg")
        .icon("Mine", "48x48/mimetypes/image-x-generic.png");
    let mut themes = themes(Some("Mine"), &[&base]);
    let wanted = names(&["image-png", "image-x-generic"]);

    let mut tried = Vec::new();
    let taken = themes.find(&wanted, |path| {
        tried.push(path.clone());
        path.ends_with("image-x-generic.png").then_some(path)
    });

    assert_eq!(
        taken,
        Some(base.path("Mine", "48x48/mimetypes/image-x-generic.png"))
    );
    assert_eq!(
        tried,
        [
            base.path("Mine", "48x48/mimetypes/image-png.png"),
            base.path("Mine", "scalable/mimetypes/image-png.svg"),
            base.path("Mine", "48x48/mimetypes/image-x-generic.png"),
        ]
    );
}

#[test]
fn png_comes_before_svg_in_one_folder() {
    let base = Base::new("png-first");
    base.index("Mine", MINE)
        .icon("Mine", "48x48/mimetypes/a.svg")
        .icon("Mine", "48x48/mimetypes/a.png")
        .icon("Mine", "48x48/mimetypes/a.xpm")
        .icon("Mine", "48x48/mimetypes/b.xpm");
    let mut themes = themes(Some("Mine"), &[&base]);
    let found = themes.find(&names(&["a"]), Some);
    assert_eq!(found, Some(base.path("Mine", "48x48/mimetypes/a.png")));
    assert_eq!(themes.find(&names(&["b"]), Some), None);
}

#[test]
fn an_earlier_folder_wins_over_a_later_ones_png() {
    let first = Base::new("first");
    let second = Base::new("second");
    first
        .index("Mine", MINE)
        .icon("Mine", "48x48/mimetypes/a.svg");
    second.icon("Mine", "48x48/mimetypes/a.png");
    let mut themes = themes(Some("Mine"), &[&first, &second]);
    let found = themes.find(&names(&["a"]), Some);
    assert_eq!(found, Some(first.path("Mine", "48x48/mimetypes/a.svg")));
}

#[test]
fn without_a_chosen_theme_the_default_one_is_searched() {
    let base = Base::new("default");
    base.index(DEFAULT_THEME, "[Icon Theme]\nInherits=Mine\n")
        .index("Mine", MINE)
        .icon("Mine", "48x48/mimetypes/a.png");
    let mut themes = themes(None, &[&base]);
    let found = themes.find(&names(&["a"]), Some);
    assert_eq!(found, Some(base.path("Mine", "48x48/mimetypes/a.png")));
}

#[test]
fn a_folder_reaching_outside_its_theme_is_left_out() {
    let read = index::parse(
        "[Icon Theme]\nDirectories=../x,/abs,a/../b,ok\n\
         [../x]\nSize=48\n[/abs]\nSize=48\n[a/../b]\nSize=48\n[ok]\nSize=48\n",
    );
    let paths: Vec<_> = read
        .folders
        .iter()
        .map(|folder| folder.path.as_str())
        .collect();
    assert_eq!(paths, ["ok"]);
}

#[test]
fn key_files_read_values_from_their_own_section() {
    let text = "# c\nKey=outside\n[A]\n Key = a \n[B]\nKey=b\nbroken line\n";
    assert_eq!(ini::value(text, "A", "Key"), Some("a"));
    assert_eq!(ini::value(text, "B", "Key"), Some("b"));
    assert_eq!(ini::value(text, "C", "Key"), None);
    assert_eq!(ini::value(text, "A", "Other"), None);
    assert_eq!(ini::lines("\n# a = b\n\n").count(), 0);
}
