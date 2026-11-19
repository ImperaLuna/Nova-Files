#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::Arc;
use std::{env, fs};

use nova_files::MimeTypes;
use nova_files_api::{EntryKind, FileIcon, IconFormat, IconKind, ThemeIcon};
use nova_files_look_core::FileIcons;

const ICON_LIMIT: usize = 1024 * 1024;

struct Data(PathBuf);

impl Data {
    fn new(name: &str) -> Self {
        let dir = env::temp_dir().join(format!(
            "nova-files-file-icons-{name}-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&dir).ok();
        let write = |file: &str, text: &[u8]| {
            let path = dir.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        write(
            "mime/globs2",
            b"50:image/png:*.png\n50:image/jpeg:*.jpg\n50:text/plain:*.txt\n\
              50:application/zip:*.zip\n50:application/x-big:*.big\n\
              50:application/x-edge:*.edge\n",
        );
        write("mime/generic-icons", b"application/zip:package-x-generic\n");
        write(
            "icons/Mine/index.theme",
            b"[Icon Theme]\nDirectories=m\n[m]\nSize=48\nType=Scalable\nMinSize=8\nMaxSize=256\n",
        );
        write("icons/Mine/m/image-x-generic.svg", b"<svg/>");
        write("icons/Mine/m/text-plain.png", b"png");
        let mut big = vec![0; ICON_LIMIT];
        write("icons/Mine/m/application-x-edge.png", &big);
        big.push(0);
        write("icons/Mine/m/application-x-big.png", &big);
        Self(dir)
    }

    fn icons(&self) -> FileIcons {
        let types = Arc::new(MimeTypes::load(std::slice::from_ref(&self.0)));
        FileIcons::new(types, Some("Mine".to_owned()), vec![self.0.join("icons")])
    }
}

impl Drop for Data {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn theme(icon: &FileIcon) -> &ThemeIcon {
    match icon {
        FileIcon::Theme { icon, .. } => icon,
        FileIcon::BuiltIn(kind) => panic!("built in: {kind:?}"),
    }
}

#[test]
fn files_of_a_type_share_its_theme_icon() {
    let data = Data::new("share");
    let icons = data.icons();

    let png = icons.of(OsStr::new("a.png"), EntryKind::File);
    let jpeg = icons.of(OsStr::new("b.jpg"), EntryKind::File);
    let again = icons.of(OsStr::new("c.PNG"), EntryKind::File);

    let FileIcon::Theme { icon: shared, kind } = &png else {
        panic!("{png:?}")
    };
    assert_eq!(*kind, IconKind::Picture);
    assert_eq!(shared.format, IconFormat::Svg);
    assert_eq!(&*shared.bytes, b"<svg/>");
    assert!(
        matches!((&jpeg, &again), (FileIcon::Theme { icon: a, .. }, FileIcon::Theme { icon: b, .. })
        if Arc::ptr_eq(a, shared) && Arc::ptr_eq(b, shared))
    );
    let text = icons.of(OsStr::new("notes.txt"), EntryKind::File);
    assert_eq!(theme(&text).format, IconFormat::Png);
    assert_eq!(
        theme(&text).path,
        data.0.join("icons/Mine/m/text-plain.png")
    );
}

#[test]
fn the_desktops_first_installed_theme_counts_when_the_files_name_none() {
    let data = Data::new("desktop");
    let write = |file: &str, text: &[u8]| {
        let path = data.0.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    };
    write(
        "icons/Desk/index.theme",
        b"[Icon Theme]\nDirectories=m\n[m]\nSize=48\n",
    );
    write("icons/Desk/m/text-plain.svg", b"<svg/>");
    let bases = vec![data.0.join("icons")];
    let named =
        |names: &[&str]| -> Vec<String> { names.iter().map(|name| (*name).to_owned()).collect() };
    let mine = data.0.join("icons/Mine/m/text-plain.png");
    let desk = data.0.join("icons/Desk/m/text-plain.svg");

    let with_files = data.icons();
    with_files.follow_desktop(&named(&["Desk"]));
    let text = with_files.of(OsStr::new("a.txt"), EntryKind::File);
    assert_eq!(theme(&text).path, mine);

    for files in [None, Some("Gone".to_owned())] {
        let types = Arc::new(MimeTypes::load(std::slice::from_ref(&data.0)));
        let icons = FileIcons::new(types, files, bases.clone());
        let text = || icons.of(OsStr::new("a.txt"), EntryKind::File);
        icons.follow_desktop(&named(&["../Mine", "Missing", "Desk", "Mine"]));
        assert_eq!(theme(&text()).path, desk);
        icons.follow_desktop(&named(&["Mine"]));
        assert_eq!(theme(&text()).path, mine);
        icons.follow_desktop(&[]);
        assert_eq!(text(), FileIcon::BuiltIn(IconKind::Text));
    }
}

#[test]
fn without_a_theme_icon_a_file_shows_its_kind() {
    let data = Data::new("kinds");
    let icons = data.icons();
    let of = |name: &str, kind| icons.of(OsStr::new(name), kind);

    assert_eq!(
        of("a.png", EntryKind::Folder),
        FileIcon::BuiltIn(IconKind::Folder)
    );
    assert_eq!(
        of("a.zip", EntryKind::File),
        FileIcon::BuiltIn(IconKind::Archive)
    );
    assert_eq!(
        of("Makefile", EntryKind::File),
        FileIcon::BuiltIn(IconKind::File)
    );
    assert!(matches!(
        of("a.edge", EntryKind::File),
        FileIcon::Theme { .. }
    ));
    assert_eq!(
        of("a.big", EntryKind::File),
        FileIcon::BuiltIn(IconKind::File)
    );
}
