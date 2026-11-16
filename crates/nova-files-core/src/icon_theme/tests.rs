use std::fs;
use std::path::PathBuf;

use super::index::{Folder, Sizes};
use super::*;

/// A folder of icon themes of its own, removed afterwards.
struct Base(PathBuf);

impl Base {
    fn new(name: &str) -> Self {
        let dir = env::temp_dir().join(format!("nova-files-icons-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
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

impl Drop for Base {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
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

fn themes(chosen: Option<&str>, bases: &[&Base]) -> IconThemes {
    let bases = bases.iter().map(|base| base.0.clone()).collect();
    IconThemes::new(chosen.map(str::to_owned), bases, 48, 1)
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

    let found = themes.find(&names(&["image-png"]));
    assert_eq!(
        found,
        Some(base.path("Mine", "48x48/mimetypes/image-png.png"))
    );
    let scalable = themes.find(&names(&["text-plain"]));
    assert_eq!(
        scalable,
        Some(base.path("Mine", "scalable/mimetypes/text-plain.svg"))
    );
    // Only file type folders count.
    assert_eq!(themes.find(&names(&["text-html"])), None);
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

    let found = themes.find(&names(&["image-png", "image-x-generic"]));
    assert_eq!(
        found,
        Some(base.path("Mine", "48x48/mimetypes/image-x-generic.png"))
    );
    let inherited = themes.find(&names(&["text-plain"]));
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

    // Parent lists hicolor before it is searched: hicolor still comes last.
    let mut chosen = themes(Some("Mine"), &[&base]);
    let found = chosen.find(&names(&["text-plain"]));
    assert_eq!(found, Some(base.path("Parent", "mimes/text-plain.png")));
    let last = chosen.find(&names(&["audio-x-generic"]));
    assert_eq!(
        last,
        Some(base.path(HICOLOR, "48x48/mimetypes/audio-x-generic.png"))
    );

    let mut unset = themes(None, &[&base]);
    let found = unset.find(&names(&["text-plain"]));
    assert_eq!(
        found,
        Some(base.path(HICOLOR, "48x48/mimetypes/text-plain.png"))
    );
    assert_eq!(unset.find(&names(&["video-x-generic"])), None);
}

#[test]
fn a_theme_spreads_over_several_folders() {
    let user = Base::new("spread-user");
    let system = Base::new("spread-system");
    user.index("Mine", MINE)
        .icon("Mine", "48x48/mimetypes/a.png");
    // The first index found counts: this one lists no folders.
    system
        .index("Mine", "[Icon Theme]\nDirectories=\n")
        .icon("Mine", "48x48/mimetypes/b.svg");
    let mut themes = themes(Some("Mine"), &[&user, &system]);

    assert_eq!(
        themes.find(&names(&["a"])),
        Some(user.path("Mine", "48x48/mimetypes/a.png"))
    );
    assert_eq!(
        themes.find(&names(&["b"])),
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
        looped.find(&names(&["x"])),
        Some(base.path("Parent", "mimes/x.png"))
    );
    assert_eq!(looped.find(&names(&["y"])), None);

    let mut missing = themes(Some("Gone"), &[&base]);
    assert_eq!(missing.find(&names(&["x"])), None);
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
    // Listed twice, or with no section or size: once, or not at all.
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
    assert_eq!(name::chosen(&config.0), None);
    write(
        "kdeglobals",
        "[General]\nTheme=No\n[Icons]\nTheme=breeze-dark\n",
    );
    assert_eq!(name::chosen(&config.0).as_deref(), Some("breeze-dark"));
    write(
        "gtk-4.0/settings.ini",
        "[Settings]\ngtk-icon-theme-name = \"Papirus\"\n",
    );
    assert_eq!(name::chosen(&config.0).as_deref(), Some("Papirus"));
    // A name that reaches outside the icons folder is passed over.
    write(
        "gtk-3.0/settings.ini",
        "[Settings]\ngtk-icon-theme-name=../x\n",
    );
    assert_eq!(name::chosen(&config.0).as_deref(), Some("Papirus"));
    write(
        "gtk-3.0/settings.ini",
        "[Settings]\ngtk-icon-theme-name=Adwaita\n",
    );
    assert_eq!(name::chosen(&config.0).as_deref(), Some("Adwaita"));
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
