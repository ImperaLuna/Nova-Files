use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use nova_files_api::{
    EntryKind, FILE_ICON_PIXELS, FileIcon, FileTypes as _, IconFormat, IconKind, ThemeIcon,
};

use crate::icon_theme::{self, IconThemes, name};
use nova_files_open_core::MimeTypes;

const ANY_FILE: &str = "application/octet-stream";
const SVG_EXTENSION: &str = "svg";
const MAX_ICON_BYTES: u64 = 1024 * 1024;
const SCALE: u32 = 1;

#[derive(Debug)]
pub struct FileIcons {
    types: Arc<OnceLock<MimeTypes>>,
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    themes: IconThemes,
    from_files: Option<String>,
    by_type: HashMap<String, FileIcon>,
    files: HashMap<PathBuf, Arc<ThemeIcon>>,
}

impl FileIcons {
    pub fn from_env(types: Arc<OnceLock<MimeTypes>>, config: Option<&Path>) -> Self {
        let from_files = config.and_then(name::chosen);
        Self::new(
            types,
            from_files,
            icon_theme::bases(|name| env::var_os(name)),
        )
    }

    pub fn new(
        types: Arc<OnceLock<MimeTypes>>,
        from_files: Option<String>,
        bases: Vec<PathBuf>,
    ) -> Self {
        Self {
            types,
            state: Mutex::new(State {
                themes: IconThemes::new(from_files.clone(), bases, FILE_ICON_PIXELS, SCALE),
                from_files,
                by_type: HashMap::new(),
                files: HashMap::new(),
            }),
        }
    }

    #[cfg(test)]
    pub fn plain() -> Arc<Self> {
        let types = Arc::new(OnceLock::from(MimeTypes::default()));
        Arc::new(Self::new(types, None, Vec::new()))
    }

    pub fn follow_desktop(&self, named: &[String]) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let installed = |name: &String| state.themes.installed(name);
        let chosen = state
            .from_files
            .clone()
            .filter(installed)
            .or_else(|| {
                named
                    .iter()
                    .filter_map(|name| name::checked(name))
                    .find(installed)
            })
            .or_else(|| state.from_files.clone());
        if state.themes.choose(chosen) {
            state.by_type.clear();
            state.files.clear();
        }
    }

    #[expect(
        clippy::significant_drop_tightening,
        reason = "a type is searched once: the search and keeping its answer share the lock"
    )]
    pub fn of(&self, name: &OsStr, kind: EntryKind) -> FileIcon {
        if kind == EntryKind::Folder {
            return FileIcon::BuiltIn(IconKind::Folder);
        }
        let types = self.types.get_or_init(MimeTypes::from_env);
        let mime = types.of(name).unwrap_or(ANY_FILE);
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let State {
            themes,
            by_type,
            files,
            ..
        } = &mut *state;
        if let Some(icon) = by_type.get(mime) {
            return icon.clone();
        }
        let found = themes.find(&types.icon_names(mime), |path| read(files, path));
        let kind = types.icon_kind(mime);
        let icon = found.map_or(FileIcon::BuiltIn(kind), |icon| FileIcon::Theme {
            icon,
            kind,
        });
        by_type.insert(mime.to_owned(), icon.clone());
        icon
    }
}

fn read(files: &mut HashMap<PathBuf, Arc<ThemeIcon>>, path: PathBuf) -> Option<Arc<ThemeIcon>> {
    if let Some(icon) = files.get(&path) {
        return Some(Arc::clone(icon));
    }
    let format = if path.extension() == Some(OsStr::new(SVG_EXTENSION)) {
        IconFormat::Svg
    } else {
        IconFormat::Png
    };
    let mut bytes = Vec::new();
    File::open(&path)
        .ok()?
        .take(MAX_ICON_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if u64::try_from(bytes.len()).map_or(true, |length| length > MAX_ICON_BYTES) {
        return None;
    }
    let icon = Arc::new(ThemeIcon {
        path: path.clone(),
        format,
        bytes: bytes.into(),
    });
    files.insert(path, Arc::clone(&icon));
    Some(icon)
}

impl nova_files_disk::ports::Icons for FileIcons {
    fn of(&self, name: &OsStr, kind: EntryKind) -> FileIcon {
        Self::of(self, name, kind)
    }

    fn follow_desktop(&self, named: &[String]) {
        Self::follow_desktop(self, named);
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use super::*;

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
            let types = MimeTypes::load(std::slice::from_ref(&self.0));
            let types = Arc::new(OnceLock::from(types));
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
        let named = |names: &[&str]| -> Vec<String> {
            names.iter().map(|name| (*name).to_owned()).collect()
        };
        let mine = data.0.join("icons/Mine/m/text-plain.png");
        let desk = data.0.join("icons/Desk/m/text-plain.svg");

        let with_files = data.icons();
        with_files.follow_desktop(&named(&["Desk"]));
        let text = with_files.of(OsStr::new("a.txt"), EntryKind::File);
        assert_eq!(theme(&text).path, mine);

        for files in [None, Some("Gone".to_owned())] {
            let types = Arc::new(OnceLock::from(MimeTypes::load(std::slice::from_ref(
                &data.0,
            ))));
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
}
