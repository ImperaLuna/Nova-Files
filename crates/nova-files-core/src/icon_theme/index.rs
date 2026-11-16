//! A theme's `index.theme`: the themes it inherits from and its icon
//! folders, each with the sizes it serves, as the freedesktop icon theme
//! spec defines them. Only the folders whose icons stand for file types
//! are kept. Keys the spec doesn't name, and values that don't parse, are
//! skipped; a folder missing its `Size` is left out.

use std::collections::HashSet;

const SECTION_OPEN: char = '[';
const SECTION_CLOSE: char = ']';
const COMMENT: char = '#';
const ASSIGN: char = '=';
const LIST: char = ',';
const THEME_SECTION: &str = "Icon Theme";
const INHERITS: &str = "Inherits";
const DIRECTORIES: &str = "Directories";
const SCALED_DIRECTORIES: &str = "ScaledDirectories";
const SIZE: &str = "Size";
const SCALE: &str = "Scale";
const CONTEXT: &str = "Context";
const TYPE: &str = "Type";
const MIN_SIZE: &str = "MinSize";
const MAX_SIZE: &str = "MaxSize";
const THRESHOLD: &str = "Threshold";
const FIXED: &str = "Fixed";
const SCALABLE: &str = "Scalable";
/// The folders of file type icons.
const MIME_CONTEXT: &str = "MimeTypes";
/// The spec's defaults.
const DEFAULT_SCALE: u32 = 1;
const DEFAULT_THRESHOLD: u32 = 2;

/// What a theme's index says.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Index {
    /// The themes asked for when this one has no icon, in order.
    pub inherits: Vec<String>,
    /// Its folders of file type icons, in the order it lists them.
    pub folders: Vec<Folder>,
}

/// One icon folder, relative to the theme's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub path: String,
    pub size: u32,
    pub scale: u32,
    pub sizes: Sizes,
}

/// The sizes a folder's icons may be drawn at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sizes {
    Fixed,
    Scalable { min: u32, max: u32 },
    Threshold(u32),
}

impl Folder {
    /// Whether its icons are made for `size` at `scale`.
    pub fn serves(&self, size: u32, scale: u32) -> bool {
        let (low, high) = self.range();
        self.scale == scale && (low..=high).contains(&size)
    }

    /// How far its icons are from `size` at `scale`, in device pixels.
    pub fn distance(&self, size: u32, scale: u32) -> u32 {
        let wanted = size.saturating_mul(scale);
        let (low, high) = self.range();
        let low = low.saturating_mul(self.scale);
        let high = high.saturating_mul(self.scale);
        low.saturating_sub(wanted).max(wanted.saturating_sub(high))
    }

    /// The smallest and largest size it serves.
    const fn range(&self) -> (u32, u32) {
        match self.sizes {
            Sizes::Fixed => (self.size, self.size),
            Sizes::Scalable { min, max } => (min, max),
            Sizes::Threshold(threshold) => (
                self.size.saturating_sub(threshold),
                self.size.saturating_add(threshold),
            ),
        }
    }
}

/// The settings of one folder's section, as read.
#[derive(Default)]
struct Section {
    size: Option<u32>,
    scale: Option<u32>,
    context: Option<String>,
    kind: Option<String>,
    min: Option<u32>,
    max: Option<u32>,
    threshold: Option<u32>,
}

impl Section {
    fn set(&mut self, key: &str, value: &str) {
        let number = || value.parse().ok();
        match key {
            SIZE => self.size = number(),
            SCALE => self.scale = number(),
            MIN_SIZE => self.min = number(),
            MAX_SIZE => self.max = number(),
            THRESHOLD => self.threshold = number(),
            CONTEXT => self.context = Some(value.to_owned()),
            TYPE => self.kind = Some(value.to_owned()),
            _ => {}
        }
    }

    /// The folder at `path`, if its section names a size and it holds
    /// file type icons (or doesn't say what it holds).
    fn folder(&self, path: &str) -> Option<Folder> {
        let size = self.size?;
        if self
            .context
            .as_deref()
            .is_some_and(|context| !context.eq_ignore_ascii_case(MIME_CONTEXT))
        {
            return None;
        }
        let sizes = match self.kind.as_deref() {
            Some(FIXED) => Sizes::Fixed,
            Some(SCALABLE) => Sizes::Scalable {
                min: self.min.unwrap_or(size),
                max: self.max.unwrap_or(size),
            },
            _ => Sizes::Threshold(self.threshold.unwrap_or(DEFAULT_THRESHOLD)),
        };
        Some(Folder {
            path: path.to_owned(),
            size,
            scale: self.scale.unwrap_or(DEFAULT_SCALE).max(1),
            sizes,
        })
    }
}

/// Reads an `index.theme`'s text.
pub fn parse(text: &str) -> Index {
    let mut current: Option<String> = None;
    let mut inherits = Vec::new();
    let mut listed: Vec<String> = Vec::new();
    let mut sections: Vec<(String, Section)> = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with(COMMENT) {
            continue;
        }
        if let Some(name) = line
            .strip_prefix(SECTION_OPEN)
            .and_then(|rest| rest.strip_suffix(SECTION_CLOSE))
        {
            current = Some(name.to_owned());
            if name != THEME_SECTION {
                sections.push((name.to_owned(), Section::default()));
            }
            continue;
        }
        let Some((key, value)) = line.split_once(ASSIGN) else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match current.as_deref() {
            Some(THEME_SECTION) => match key {
                INHERITS => inherits = list(value),
                DIRECTORIES | SCALED_DIRECTORIES => listed.extend(list(value)),
                _ => {}
            },
            Some(_) => {
                if let Some((_, section)) = sections.last_mut() {
                    section.set(key, value);
                }
            }
            None => {}
        }
    }
    let mut seen = HashSet::new();
    listed.retain(|path| seen.insert(path.clone()));
    let folders = listed
        .iter()
        .filter_map(|path| {
            let (_, section) = sections.iter().find(|(name, _)| name == path)?;
            section.folder(path)
        })
        .collect();
    Index { inherits, folders }
}

/// A comma-separated list, without empty items.
fn list(value: &str) -> Vec<String> {
    value
        .split(LIST)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}
