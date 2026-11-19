use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use nova_files_api::{EntryKind, FILE_ICON_PIXELS, FileIcon, IconFormat, IconKind, ThemeIcon};
use nova_files_disk::ports::Mimes;

use crate::icon_theme::{self, IconThemes, name};

const ANY_FILE: &str = "application/octet-stream";
const SVG_EXTENSION: &str = "svg";
const MAX_ICON_BYTES: u64 = 1024 * 1024;
const SCALE: u32 = 1;

#[derive(Debug)]
pub struct FileIcons {
    types: Arc<dyn Mimes>,
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
    pub fn from_env(types: Arc<dyn Mimes>, config: Option<&Path>) -> Self {
        let from_files = config.and_then(name::chosen);
        Self::new(
            types,
            from_files,
            icon_theme::bases(|name| env::var_os(name)),
        )
    }

    pub fn new(types: Arc<dyn Mimes>, from_files: Option<String>, bases: Vec<PathBuf>) -> Self {
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
        let types = &*self.types;
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
