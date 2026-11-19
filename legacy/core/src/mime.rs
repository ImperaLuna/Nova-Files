// Reads the shared-mime-info database.
// https://specifications.freedesktop.org/shared-mime-info-spec/latest/
use std::cmp::Reverse;
use std::collections::{HashMap, VecDeque};
use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use nova_files_api::{FileTypes, IconKind, glob_matches};

use crate::capped;
use nova_files_disk::home::data_dirs;

const GLOBS_FILE: &str = "mime/globs2";
const SUBCLASSES_FILE: &str = "mime/subclasses";
const ALIASES_FILE: &str = "mime/aliases";
const ICONS_FILE: &str = "mime/icons";
const GENERIC_ICONS_FILE: &str = "mime/generic-icons";
const TYPES_FOLDER: &str = "mime";
const TYPE_FILE_EXTENSION: &str = "xml";
const COMMENT_OPEN: &str = "<comment>";
const COMMENT_CLOSE: &str = "</comment>";
const ENTITIES: [(&str, &str); 5] = [
    ("&lt;", "<"),
    ("&gt;", ">"),
    ("&quot;", "\""),
    ("&apos;", "'"),
    ("&amp;", "&"),
];
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

const COMMENT: char = '#';
const FIELD: char = ':';
const FLAG: char = ',';
const CASE_SENSITIVE_FLAG: &str = "cs";
const NO_GLOBS: &str = "__NOGLOBS__";
const WILDCARDS: [char; 3] = ['*', '?', '['];
const ANY_RUN: char = '*';
const MEDIA_SEPARATOR: char = '/';
const ANY_SUBTYPE: &str = "/*";
const TEXT_MEDIA: &str = "text/";
const PLAIN_TEXT: &str = "text/plain";
const ANY_FILE: &str = "application/octet-stream";

#[derive(Debug)]
struct Glob {
    weight: u32,
    mime: String,
    pattern: String,
    case_sensitive: bool,
    order: usize,
}

#[derive(Debug, Default)]
pub struct MimeTypes {
    literals: HashMap<String, Vec<Glob>>,
    suffixes: HashMap<String, Vec<Glob>>,
    others: Vec<Glob>,
    parents: HashMap<String, Vec<String>>,
    aliases: HashMap<String, String>,
    icons: HashMap<String, String>,
    generic_icons: HashMap<String, String>,
    dirs: Vec<PathBuf>,
}

impl MimeTypes {
    pub fn from_env() -> Self {
        Self::load(&data_dirs(|name| env::var_os(name)))
    }

    pub fn load(dirs: &[PathBuf]) -> Self {
        let mut types = Self {
            dirs: dirs.to_vec(),
            ..Self::default()
        };
        let mut order = 0;
        for dir in dirs {
            if let Some(text) = read_capped(&dir.join(GLOBS_FILE)) {
                for mut glob in text.lines().filter_map(glob) {
                    glob.order = order;
                    order += 1;
                    types.add(glob);
                }
            }
            for (child, parent) in pairs(&dir.join(SUBCLASSES_FILE)) {
                types.parents.entry(child).or_default().push(parent);
            }
            types.aliases.extend(pairs(&dir.join(ALIASES_FILE)));
            for (mime, icon) in icon_pairs(&dir.join(ICONS_FILE)) {
                types.icons.entry(mime).or_insert(icon);
            }
            for (mime, icon) in icon_pairs(&dir.join(GENERIC_ICONS_FILE)) {
                types.generic_icons.entry(mime).or_insert(icon);
            }
        }
        types
    }

    fn add(&mut self, glob: Glob) {
        let suffix = glob
            .pattern
            .strip_prefix(ANY_RUN)
            .filter(|rest| !rest.is_empty() && !rest.contains(WILDCARDS));
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

    pub fn ancestry(&self, mime: &str) -> Vec<String> {
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

    pub fn name(&self, mime: &str) -> Option<String> {
        let (media, subtype) = self.canonical(mime).split_once(MEDIA_SEPARATOR)?;
        let part = |text: &str| {
            !text.is_empty() && !text.starts_with('.') && !text.contains(MEDIA_SEPARATOR)
        };
        if !part(media) || !part(subtype) {
            return None;
        }
        let file = Path::new(TYPES_FOLDER)
            .join(media)
            .join(format!("{subtype}.{TYPE_FILE_EXTENSION}"));
        self.dirs
            .iter()
            .find_map(|dir| read_capped(&dir.join(&file)))
            .and_then(|text| comment(&text))
    }
}

fn comment(text: &str) -> Option<String> {
    let start = text.find(COMMENT_OPEN)? + COMMENT_OPEN.len();
    let length = text[start..].find(COMMENT_CLOSE)?;
    let name = ENTITIES.iter().fold(
        text[start..start + length].to_owned(),
        |name, (entity, plain)| name.replace(entity, plain),
    );
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
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
                    Reverse(glob.order),
                )
            })
            .map(|glob| self.canonical(&glob.mime))
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
        order: 0,
    })
}

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

fn icon_pairs(path: &Path) -> Vec<(String, String)> {
    read_capped(path)
        .map(|text| {
            text.lines()
                .filter(|line| !line.starts_with(COMMENT))
                .filter_map(|line| line.split_once(FIELD))
                .map(|(mime, icon)| (mime.to_owned(), icon.trim().to_owned()))
                .collect()
        })
        .unwrap_or_default()
}

impl nova_files_disk::ports::Mimes for MimeTypes {
    fn ancestry(&self, mime: &str) -> Vec<String> {
        Self::ancestry(self, mime)
    }

    fn name(&self, mime: &str) -> Option<String> {
        Self::name(self, mime)
    }

    fn icon_names(&self, mime: &str) -> Vec<String> {
        Self::icon_names(self, mime)
    }

    fn icon_kind(&self, mime: &str) -> IconKind {
        Self::icon_kind(self, mime)
    }
}

#[derive(Debug)]
pub struct LazyMimes(pub Arc<OnceLock<MimeTypes>>);

impl LazyMimes {
    fn types(&self) -> &MimeTypes {
        self.0.get_or_init(MimeTypes::from_env)
    }
}

impl FileTypes for LazyMimes {
    fn of(&self, name: &OsStr) -> Option<&str> {
        self.types().of(name)
    }

    fn is_a(&self, mime: &str, wanted: &str) -> bool {
        self.types().is_a(mime, wanted)
    }
}

impl nova_files_disk::ports::Mimes for LazyMimes {
    fn ancestry(&self, mime: &str) -> Vec<String> {
        self.types().ancestry(mime)
    }

    fn name(&self, mime: &str) -> Option<String> {
        self.types().name(mime)
    }

    fn icon_names(&self, mime: &str) -> Vec<String> {
        self.types().icon_names(mime)
    }

    fn icon_kind(&self, mime: &str) -> IconKind {
        self.types().icon_kind(mime)
    }
}

fn read_capped(path: &Path) -> Option<String> {
    capped::text(path, MAX_FILE_BYTES)
}

mod icons;
#[cfg(test)]
mod tests;
