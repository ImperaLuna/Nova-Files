//! Each entry's icon: its type's icon from the system icon theme, read
//! once and shared by every entry of that type, or the kind of file it is
//! when no theme has one. Folders always take the window's own icon.
//!
//! A name's type comes from the MIME database (the name alone, no read of
//! the file); a name the database doesn't know is
//! `application/octet-stream`. Answers are kept per type for as long as
//! the core runs, and icon files per path, so a folder of a thousand
//! photos costs one search. An icon file that can't be read, or is too big
//! for an icon, counts as missing and the search goes on. The answers sit
//! behind one lock shared by every client; a search under it costs a few
//! hash lookups once the theme's folders are listed (see `icon_theme`).

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use nova_files_api::{
    EntryKind, FILE_ICON_PIXELS, FileIcon, FileTypes as _, IconFormat, IconKind, ThemeIcon,
};

use crate::icon_theme::{IconThemes, name};
use crate::mime::MimeTypes;

/// The type of a file whose name says nothing.
const ANY_FILE: &str = "application/octet-stream";
const SVG_EXTENSION: &str = "svg";
/// Real icons are a few KB; anything far bigger isn't one.
const MAX_ICON_BYTES: u64 = 1024 * 1024;
/// Theme icons are picked for one scale; the window shrinks them.
const SCALE: u32 = 1;

/// The icons of every client's entries.
#[derive(Debug)]
pub struct FileIcons {
    types: Arc<OnceLock<MimeTypes>>,
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    themes: IconThemes,
    by_type: HashMap<String, FileIcon>,
    files: HashMap<PathBuf, Arc<ThemeIcon>>,
}

impl FileIcons {
    /// Icons from the theme chosen in the settings under `config`, in the
    /// system's icon folders; `types` is read at the first answer if no
    /// one has read it yet.
    pub fn from_env(types: Arc<OnceLock<MimeTypes>>, config: Option<&Path>) -> Self {
        let chosen = config.and_then(name::chosen);
        Self::new(types, chosen, IconThemes::bases_from_env())
    }

    /// Icons from the theme `chosen`, in the icon folders `bases`.
    pub fn new(
        types: Arc<OnceLock<MimeTypes>>,
        chosen: Option<String>,
        bases: Vec<PathBuf>,
    ) -> Self {
        Self {
            types,
            state: Mutex::new(State {
                themes: IconThemes::new(chosen, bases, FILE_ICON_PIXELS, SCALE),
                by_type: HashMap::new(),
                files: HashMap::new(),
            }),
        }
    }

    /// Icons with no MIME database and no themes: every file is a plain
    /// file, for tests about something else.
    #[cfg(test)]
    pub fn plain() -> Arc<Self> {
        let types = Arc::new(OnceLock::from(MimeTypes::default()));
        Arc::new(Self::new(types, None, Vec::new()))
    }

    /// The icon of an entry called `name`.
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

/// The icon file at `path`, read once and kept in `files`.
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

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use super::*;

    /// A data folder with a MIME database and an icon theme, removed
    /// afterwards.
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
                  50:application/zip:*.zip\n50:application/x-big:*.big\n",
            );
            write("mime/generic-icons", b"application/zip:package-x-generic\n");
            write(
                "icons/Mine/index.theme",
                b"[Icon Theme]\nDirectories=m\n[m]\nSize=48\nType=Scalable\nMinSize=8\nMaxSize=256\n",
            );
            write("icons/Mine/m/image-x-generic.svg", b"<svg/>");
            write("icons/Mine/m/text-plain.png", b"png");
            let big = vec![0; usize::try_from(MAX_ICON_BYTES).unwrap() + 1];
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
        // Too big to be an icon.
        assert_eq!(
            of("a.big", EntryKind::File),
            FileIcon::BuiltIn(IconKind::File)
        );
    }
}
