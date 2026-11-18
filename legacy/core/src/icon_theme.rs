// Follows the freedesktop icon theme spec.
// https://specifications.freedesktop.org/icon-theme-spec/latest/
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

use crate::capped;
use nova_files_disk::home::data_dirs;

const ICONS_FOLDER: &str = "icons";
const HOME_ICONS: &str = ".icons";
const HOME_VAR: &str = "HOME";
const INDEX_FILE: &str = "index.theme";
pub const HICOLOR: &str = "hicolor";
pub const DEFAULT_THEME: &str = "default";
const EXTENSIONS: [&str; 2] = ["png", "svg"];
const EXTENSION_SEPARATOR: char = '.';
const MAX_INDEX_BYTES: u64 = 1024 * 1024;
const MAX_THEMES: usize = 32;

#[derive(Debug)]
pub struct IconThemes {
    chosen: Option<String>,
    bases: Vec<PathBuf>,
    chain: Option<Vec<Theme>>,
    size: u32,
    scale: u32,
}

#[derive(Debug)]
struct Theme {
    name: String,
    folders: Vec<String>,
    listed: Option<Vec<HashMap<String, Found>>>,
}

#[derive(Debug, Clone, Copy)]
struct Found {
    base: usize,
    extension: &'static str,
}

impl IconThemes {
    pub const fn new(chosen: Option<String>, bases: Vec<PathBuf>, size: u32, scale: u32) -> Self {
        Self {
            chosen,
            bases,
            chain: None,
            size,
            scale,
        }
    }

    pub fn choose(&mut self, chosen: Option<String>) -> bool {
        if chosen == self.chosen {
            return false;
        }
        self.chosen = chosen;
        self.chain = None;
        true
    }

    pub fn installed(&self, name: &str) -> bool {
        self.bases
            .iter()
            .any(|base| base.join(name).join(INDEX_FILE).is_file())
    }

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

fn ordered(name: String, index: Index, size: u32, scale: u32) -> Theme {
    let mut folders = index.folders;
    folders.sort_by_key(|folder| (!folder.serves(size, scale), folder.distance(size, scale)));
    Theme {
        name,
        folders: folders.into_iter().map(|folder| folder.path).collect(),
        listed: None,
    }
}

fn read_index(name: &str, bases: &[PathBuf]) -> Option<Index> {
    bases
        .iter()
        .find_map(|base| capped::text(&base.join(name).join(INDEX_FILE), MAX_INDEX_BYTES))
        .map(|text| index::parse(&text))
}

pub fn bases(var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let home = var(HOME_VAR)
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(HOME_ICONS));
    let data = data_dirs(&var)
        .into_iter()
        .map(|dir| dir.join(ICONS_FOLDER));
    home.into_iter().chain(data).collect()
}
