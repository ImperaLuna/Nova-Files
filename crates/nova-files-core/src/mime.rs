//! The system's MIME database (freedesktop shared-mime-info), read for
//! the file picker's type filters: which type a file name says, and which
//! types are kinds of others.
//!
//! It reads `mime/globs2`, `mime/subclasses` and `mime/aliases` from the
//! user's data folder and each of the system's (`XDG_DATA_HOME`, then
//! `XDG_DATA_DIRS`), so it finds the database wherever the system keeps it;
//! on `NixOS` that isn't `/usr/share`. A name's type is the pattern that
//! matches it with the highest weight; on a tie a pattern that keeps case
//! wins, then the longest. Every
//! `text/*` type is also plain text, and every type is a kind of
//! `application/octet-stream`. The file's content is never read.

use std::collections::{HashMap, VecDeque};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use nova_files_api::{FileTypes, glob_matches};

const DATA_HOME_VAR: &str = "XDG_DATA_HOME";
const DATA_DIRS_VAR: &str = "XDG_DATA_DIRS";
const HOME_VAR: &str = "HOME";
const DATA_IN_HOME: &str = ".local/share";
const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";

const GLOBS_FILE: &str = "mime/globs2";
const SUBCLASSES_FILE: &str = "mime/subclasses";
const ALIASES_FILE: &str = "mime/aliases";
/// The real globs file is a few hundred KB.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

const COMMENT: char = '#';
const FIELD: char = ':';
const FLAG: char = ',';
const CASE_SENSITIVE_FLAG: &str = "cs";
/// A line that drops a type's patterns from folders read before; the
/// patterns from every folder are kept, so it is skipped.
const NO_GLOBS: &str = "__NOGLOBS__";
const WILDCARDS: [char; 3] = ['*', '?', '['];
const ANY_RUN: char = '*';
const MEDIA_SEPARATOR: char = '/';
const ANY_SUBTYPE: &str = "/*";
const TEXT_MEDIA: &str = "text/";
const PLAIN_TEXT: &str = "text/plain";
const ANY_FILE: &str = "application/octet-stream";

/// One pattern of `globs2`.
#[derive(Debug)]
struct Glob {
    weight: u32,
    mime: String,
    pattern: String,
    case_sensitive: bool,
}

/// The MIME database, read once.
#[derive(Debug, Default)]
pub struct MimeTypes {
    /// Whole names (`Makefile`), by their lower-case text.
    literals: HashMap<String, Vec<Glob>>,
    /// `*` and then plain text (`*.tar.gz`), by that text in lower case.
    suffixes: HashMap<String, Vec<Glob>>,
    /// Every other pattern (`*.[1-9]`, `README*`).
    others: Vec<Glob>,
    /// Each type's parent types.
    parents: HashMap<String, Vec<String>>,
    /// Other names to the type they stand for.
    aliases: HashMap<String, String>,
}

impl MimeTypes {
    /// Reads the database from the user's and the system's data folders.
    pub fn from_env() -> Self {
        Self::load(&data_dirs(|name| env::var_os(name)))
    }

    /// Reads the database in each of `dirs`.
    pub fn load(dirs: &[PathBuf]) -> Self {
        let mut types = Self::default();
        for dir in dirs {
            if let Some(text) = read_capped(&dir.join(GLOBS_FILE)) {
                text.lines()
                    .filter_map(glob)
                    .for_each(|glob| types.add(glob));
            }
            for (child, parent) in pairs(&dir.join(SUBCLASSES_FILE)) {
                types.parents.entry(child).or_default().push(parent);
            }
            types.aliases.extend(pairs(&dir.join(ALIASES_FILE)));
        }
        types
    }

    fn add(&mut self, glob: Glob) {
        let suffix = glob
            .pattern
            .strip_prefix(ANY_RUN)
            .filter(|rest| !rest.contains(WILDCARDS));
        match suffix {
            Some(suffix) => self.suffixes.entry(suffix.to_lowercase()),
            None if glob.pattern.contains(WILDCARDS) => {
                self.others.push(glob);
                return;
            }
            None => self.literals.entry(glob.pattern.to_lowercase()),
        }
        .or_default()
        .push(glob);
    }

    fn canonical<'a>(&'a self, mime: &'a str) -> &'a str {
        self.aliases.get(mime).map_or(mime, String::as_str)
    }

    /// `mime`, then every type it is a kind of, each once.
    fn ancestry(&self, mime: &str) -> Vec<String> {
        let mut order: Vec<String> = Vec::new();
        let mut queue = VecDeque::from([self.canonical(mime).to_owned()]);
        while let Some(current) = queue.pop_front() {
            if order.contains(&current) {
                continue;
            }
            let parents = self.parents.get(&current).into_iter().flatten();
            queue.extend(parents.map(|parent| self.canonical(parent).to_owned()));
            if current.starts_with(TEXT_MEDIA) {
                queue.push_back(PLAIN_TEXT.to_owned());
            }
            order.push(current);
        }
        order
    }
}

impl FileTypes for MimeTypes {
    fn of(&self, name: &OsStr) -> Option<&str> {
        let name = name.to_string_lossy();
        let lower = name.to_lowercase();
        let literals = self.literals.get(&lower).into_iter().flatten();
        let suffixes = lower
            .char_indices()
            .filter_map(|(at, _)| self.suffixes.get(&lower[at..]))
            .flatten();
        literals
            .chain(suffixes)
            .chain(&self.others)
            .filter(|glob| glob_matches(&glob.pattern, &name, glob.case_sensitive))
            .max_by_key(|glob| {
                (
                    glob.weight,
                    glob.case_sensitive,
                    glob.pattern.chars().count(),
                )
            })
            .map(|glob| glob.mime.as_str())
    }

    fn is_a(&self, mime: &str, wanted: &str) -> bool {
        let wanted = self.canonical(wanted);
        if wanted == ANY_FILE {
            return true;
        }
        let ancestry = self.ancestry(mime);
        wanted.strip_suffix(ANY_SUBTYPE).map_or_else(
            || ancestry.iter().any(|mime| mime == wanted),
            |media| {
                ancestry.iter().any(|mime| {
                    mime.split_once(MEDIA_SEPARATOR)
                        .is_some_and(|(of, _)| of == media)
                })
            },
        )
    }
}

/// One `weight:type:pattern[:flags]` line of `globs2`.
fn glob(line: &str) -> Option<Glob> {
    if line.starts_with(COMMENT) {
        return None;
    }
    let mut fields = line.split(FIELD);
    let weight = fields.next()?.parse().ok()?;
    let mime = fields.next()?.to_owned();
    let pattern = fields.next()?.to_owned();
    let case_sensitive = fields
        .next()
        .is_some_and(|flags| flags.split(FLAG).any(|flag| flag == CASE_SENSITIVE_FLAG));
    (pattern != NO_GLOBS && !pattern.is_empty()).then_some(Glob {
        weight,
        mime,
        pattern,
        case_sensitive,
    })
}

/// The `first second` lines of `subclasses` or `aliases`.
fn pairs(path: &Path) -> Vec<(String, String)> {
    read_capped(path)
        .map(|text| {
            text.lines()
                .filter(|line| !line.starts_with(COMMENT))
                .filter_map(|line| line.split_once(' '))
                .map(|(first, second)| (first.to_owned(), second.trim().to_owned()))
                .collect()
        })
        .unwrap_or_default()
}

/// The user's data folder, then the system's, as the XDG base directory
/// spec lists them.
fn data_dirs(var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let set = |name: &str| var(name).filter(|value| !value.is_empty());
    let home = set(DATA_HOME_VAR)
        .map(PathBuf::from)
        .or_else(|| set(HOME_VAR).map(|home| PathBuf::from(home).join(DATA_IN_HOME)));
    let system = set(DATA_DIRS_VAR).unwrap_or_else(|| DEFAULT_DATA_DIRS.into());
    home.into_iter().chain(env::split_paths(&system)).collect()
}

fn read_capped(path: &Path) -> Option<String> {
    let mut text = String::new();
    File::open(path)
        .ok()?
        .take(MAX_FILE_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

#[cfg(test)]
mod tests;
