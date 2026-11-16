//! Finding icons in the system's icon themes, as the freedesktop icon
//! theme spec lays them out.
//!
//! Themes live in `~/.icons` and in `icons` under each data folder
//! (`XDG_DATA_HOME`, then `XDG_DATA_DIRS`); one theme may be spread over
//! several of them, and its `index.theme` is the first one found. The
//! search goes through the chosen theme, then the themes it inherits from
//! (depth first, each once), then `hicolor`. In each theme every name is
//! tried before the next theme, so the theme's own generic icon beats a
//! parent's exact one and the rows keep one look. Within a theme the
//! folders made for the asked size come first, then the nearest. The
//! chain is read at the first search; only file type folders are looked in.

mod index;
pub mod name;
#[cfg(test)]
mod tests;

use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use index::Index;

use crate::mime;

/// Where the spec puts themes under a data folder.
const ICONS_FOLDER: &str = "icons";
/// The older place in home.
const HOME_ICONS: &str = ".icons";
const HOME_VAR: &str = "HOME";
const INDEX_FILE: &str = "index.theme";
/// Every theme falls back to it.
pub const HICOLOR: &str = "hicolor";
/// The extensions tried, in the spec's order; XPM isn't drawn.
const EXTENSIONS: [&str; 2] = ["png", "svg"];
/// A theme's index is tens of KB.
const MAX_INDEX_BYTES: u64 = 1024 * 1024;
/// More inherited themes than any real chain, so a loop of themes ends.
const MAX_THEMES: usize = 32;

/// The themes to search, read once.
#[derive(Debug)]
pub struct IconThemes {
    chosen: Option<String>,
    bases: Vec<PathBuf>,
    /// The chain, once read; each theme with its folders ordered for the
    /// size asked for.
    chain: Option<Vec<Theme>>,
    size: u32,
    scale: u32,
}

#[derive(Debug)]
struct Theme {
    name: String,
    /// Folders made for the size first, then by distance.
    folders: Vec<String>,
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

    /// The folders themes live in, in the spec's order.
    pub fn bases_from_env() -> Vec<PathBuf> {
        bases(|name| env::var_os(name))
    }

    /// The first icon file of `names`, tried in order in each theme of the
    /// chain.
    pub fn find(&mut self, names: &[String]) -> Option<PathBuf> {
        let bases = &self.bases;
        let chain = self.chain.get_or_insert_with(|| {
            read_chain(self.chosen.as_deref(), bases, self.size, self.scale)
        });
        chain.iter().find_map(|theme| {
            names.iter().find_map(|name| {
                theme
                    .folders
                    .iter()
                    .find_map(|folder| find_in(bases, &theme.name, folder, name))
            })
        })
    }
}

/// The file of icon `name` in `folder` of `theme`, in the first of `bases`
/// that has it.
fn find_in(bases: &[PathBuf], theme: &str, folder: &str, name: &str) -> Option<PathBuf> {
    bases.iter().find_map(|base| {
        let folder = base.join(theme).join(folder);
        EXTENSIONS
            .iter()
            .map(|extension| folder.join(format!("{name}.{extension}")))
            .find(|path| path.is_file())
    })
}

/// `chosen`, the themes it inherits from and `hicolor`, each read once.
fn read_chain(chosen: Option<&str>, bases: &[PathBuf], size: u32, scale: u32) -> Vec<Theme> {
    let mut chain: Vec<Theme> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut pending: Vec<String> = chosen.into_iter().map(str::to_owned).collect();
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
    }
}

/// The first `index.theme` of the theme called `name`.
fn read_index(name: &str, bases: &[PathBuf]) -> Option<Index> {
    bases
        .iter()
        .find_map(|base| read_capped(&base.join(name).join(INDEX_FILE)))
        .map(|text| index::parse(&text))
}

/// `~/.icons`, then `icons` in each data folder.
fn bases(var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let home = var(HOME_VAR)
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(HOME_ICONS));
    let data = mime::data_dirs(&var)
        .into_iter()
        .map(|dir| dir.join(ICONS_FOLDER));
    home.into_iter().chain(data).collect()
}

fn read_capped(path: &Path) -> Option<String> {
    let mut text = String::new();
    File::open(path)
        .ok()?
        .take(MAX_INDEX_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}
