//! Finding icons in the system's icon themes, as the freedesktop icon
//! theme spec lays them out.
//!
//! Themes live in `~/.icons` and in `icons` under each data folder
//! (`XDG_DATA_HOME`, then `XDG_DATA_DIRS`); one theme may be spread over
//! several of them, and its `index.theme` is the first one found. The
//! search goes through the chosen theme (or, when none is chosen, the
//! system's `default` theme), then the themes it inherits from (depth
//! first, each once), then `hicolor`. In each theme every name is tried
//! before the next theme, so the theme's own generic icon beats a parent's
//! exact one and the rows keep one look. Within a theme the folders made
//! for the asked size come first, then the nearest. Only file type folders
//! are looked in.
//!
//! A theme's folders are listed once, at its first search, so each later
//! search is a few hash lookups instead of thousands of file checks. The
//! caller reads the file it is handed; one it can't use (unreadable, too
//! big) lets the search go on to the next.

mod index;
mod ini;
pub mod name;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::PathBuf;

use index::Index;

use crate::{capped, mime};

/// Where the spec puts themes under a data folder.
const ICONS_FOLDER: &str = "icons";
/// The older place in home.
const HOME_ICONS: &str = ".icons";
const HOME_VAR: &str = "HOME";
const INDEX_FILE: &str = "index.theme";
/// Every theme falls back to it.
pub const HICOLOR: &str = "hicolor";
/// The theme systems set up for when the user chose none.
pub const DEFAULT_THEME: &str = "default";
/// The extensions looked for, in the spec's order; XPM isn't drawn.
const EXTENSIONS: [&str; 2] = ["png", "svg"];
const EXTENSION_SEPARATOR: char = '.';
/// A theme's index is tens of KB.
const MAX_INDEX_BYTES: u64 = 1024 * 1024;
/// More inherited themes than any real chain, so a loop of themes ends.
const MAX_THEMES: usize = 32;

/// The themes to search, read once.
#[derive(Debug)]
pub struct IconThemes {
    chosen: Option<String>,
    bases: Vec<PathBuf>,
    /// The chain, once read.
    chain: Option<Vec<Theme>>,
    size: u32,
    scale: u32,
}

#[derive(Debug)]
struct Theme {
    name: String,
    /// Folders made for the size first, then by distance.
    folders: Vec<String>,
    /// Each folder's icons, by name, once listed.
    listed: Option<Vec<HashMap<String, Found>>>,
}

/// Where one icon file is: the base folder it is in and its extension.
#[derive(Debug, Clone, Copy)]
struct Found {
    base: usize,
    extension: &'static str,
}

impl IconThemes {
    /// The themes in `bases`, starting from `chosen`, searched for icons
    /// `size` pixels across at `scale`.
    pub const fn new(chosen: Option<String>, bases: Vec<PathBuf>, size: u32, scale: u32) -> Self {
        Self {
            chosen,
            bases,
            chain: None,
            size,
            scale,
        }
    }

    /// Starts from `chosen` from now on; the chain is read again at the
    /// next search. Whether that changed anything.
    pub fn choose(&mut self, chosen: Option<String>) -> bool {
        if chosen == self.chosen {
            return false;
        }
        self.chosen = chosen;
        self.chain = None;
        true
    }

    /// Whether a theme called `name` is installed: some base has its
    /// index.
    pub fn installed(&self, name: &str) -> bool {
        self.bases
            .iter()
            .any(|base| base.join(name).join(INDEX_FILE).is_file())
    }

    /// The first icon file of `names`, tried in order in each theme of the
    /// chain, that `take` makes something of.
    pub fn find<T>(
        &mut self,
        names: &[String],
        mut take: impl FnMut(PathBuf) -> Option<T>,
    ) -> Option<T> {
        let bases = &self.bases;
        let chosen = self.chosen.as_deref().unwrap_or(DEFAULT_THEME);
        let chain = self
            .chain
            .get_or_insert_with(|| read_chain(chosen, bases, self.size, self.scale));
        chain
            .iter_mut()
            .find_map(|theme| theme.find(bases, names, &mut take))
    }
}

impl Theme {
    /// The first icon file of `names` in this theme that `take` makes
    /// something of; its folders are listed at the first search.
    fn find<T>(
        &mut self,
        bases: &[PathBuf],
        names: &[String],
        take: &mut impl FnMut(PathBuf) -> Option<T>,
    ) -> Option<T> {
        let listed = self
            .listed
            .get_or_insert_with(|| list(bases, &self.name, &self.folders));
        let folders = || self.folders.iter().zip(listed.iter());
        names.iter().find_map(|name| {
            folders().find_map(|(folder, icons)| {
                let found = icons.get(name)?;
                let file = format!("{name}{EXTENSION_SEPARATOR}{}", found.extension);
                take(bases[found.base].join(&self.name).join(folder).join(file))
            })
        })
    }
}

/// The icons in each of `folders` of `theme`, by name.
fn list(bases: &[PathBuf], theme: &str, folders: &[String]) -> Vec<HashMap<String, Found>> {
    folders
        .iter()
        .map(|folder| {
            let mut icons = HashMap::new();
            for (base, root) in bases.iter().enumerate() {
                let Ok(entries) = fs::read_dir(root.join(theme).join(folder)) else {
                    continue;
                };
                for entry in entries.flatten() {
                    add(&mut icons, &entry.file_name(), base);
                }
            }
            icons
        })
        .collect()
}

/// Notes the icon in `file`, from base `base`, if it has an extension that
/// is drawn. A name in several bases, or with both extensions, counts
/// where it is first found, PNG before SVG.
fn add(icons: &mut HashMap<String, Found>, file: &OsStr, base: usize) {
    let Some((name, extension)) = file
        .to_str()
        .and_then(|file| file.rsplit_once(EXTENSION_SEPARATOR))
    else {
        return;
    };
    let Some(extension) = EXTENSIONS.iter().find(|known| **known == extension) else {
        return;
    };
    let found = Found { base, extension };
    let first = icons.entry(name.to_owned()).or_insert(found);
    if first.base == base && found.extension == EXTENSIONS[0] {
        *first = found;
    }
}

/// `chosen`, the themes it inherits from and `hicolor`, each read once.
fn read_chain(chosen: &str, bases: &[PathBuf], size: u32, scale: u32) -> Vec<Theme> {
    let mut chain: Vec<Theme> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut pending = vec![chosen.to_owned()];
    while let Some(name) = pending.pop() {
        if seen.contains(&name) || seen.len() >= MAX_THEMES {
            continue;
        }
        seen.push(name.clone());
        let Some(index) = read_index(&name, bases) else {
            continue;
        };
        // Pushed last first, so the first parent is searched next.
        pending.extend(
            index
                .inherits
                .iter()
                .rev()
                .filter_map(|parent| name::checked(parent)),
        );
        if name != HICOLOR {
            chain.push(ordered(name, index, size, scale));
        }
    }
    if let Some(index) = read_index(HICOLOR, bases) {
        chain.push(ordered(HICOLOR.to_owned(), index, size, scale));
    }
    chain
}

/// The theme called `name` with its folders in search order.
fn ordered(name: String, index: Index, size: u32, scale: u32) -> Theme {
    let mut folders = index.folders;
    // Stable, so folders as near as each other keep the index's order.
    folders.sort_by_key(|folder| (!folder.serves(size, scale), folder.distance(size, scale)));
    Theme {
        name,
        folders: folders.into_iter().map(|folder| folder.path).collect(),
        listed: None,
    }
}

/// The first `index.theme` of the theme called `name`.
fn read_index(name: &str, bases: &[PathBuf]) -> Option<Index> {
    bases
        .iter()
        .find_map(|base| capped::text(&base.join(name).join(INDEX_FILE), MAX_INDEX_BYTES))
        .map(|text| index::parse(&text))
}

/// The folders themes live in, in the spec's order: `~/.icons`, then
/// `icons` in each data folder, from the environment `var` reads.
pub fn bases(var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let home = var(HOME_VAR)
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(HOME_ICONS));
    let data = mime::data_dirs(&var)
        .into_iter()
        .map(|dir| dir.join(ICONS_FOLDER));
    home.into_iter().chain(data).collect()
}
