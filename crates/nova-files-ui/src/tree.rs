//! The sidebar's folder tree, "This computer": the folders under Home,
//! opened and closed in place. This file holds what the tree knows and
//! shows; asking the core is in `window/tree`, drawing in
//! `view/sidebar/tree`.
//!
//! The tree knows a folder's subfolders once it has been listed. It
//! lists a folder when it is opened, again each time it is opened (so a
//! folder made since shows), and whenever the window's own listing of it
//! arrives. Until a folder is listed it is drawn with its arrow, since it
//! may hold folders; once listed with none, the arrow goes, until its
//! parent is opened again: then what it holds is unknown again, so a
//! folder made in it since can be found. Folders whose
//! names start with a dot stay out, as hidden files do.
//!
//! The tree opens itself down to the folder you're in: each folder
//! between Home and it opens, so its row shows. It never closes what the
//! user opened.
//!
//! Only one listing is asked for at a time (the core cancels a client's
//! older listing for a newer one): the others wait in order.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::f32::consts::FRAC_PI_2;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use gpui::SharedString;
use nova_files_api::{Entry, EntryKind};

use crate::places::folder_name;

/// A hidden file's name starts with this byte.
const HIDDEN: u8 = b'.';

/// The folder tree under one root.
#[derive(Debug)]
pub struct FolderTree {
    root: PathBuf,
    /// The folders opened, the root among them once opened.
    open: HashSet<PathBuf>,
    /// The subfolders of each folder listed so far, in order.
    children: HashMap<PathBuf, Vec<OsString>>,
    /// The folders to list, oldest first; none twice.
    waiting: VecDeque<PathBuf>,
}

/// Whether a row's folder can be opened, and whether it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    /// Listed with no subfolders.
    None,
    Closed,
    Open,
}

/// One row of the tree as drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub path: PathBuf,
    pub name: SharedString,
    /// How deep under the root: the root is 0.
    pub depth: usize,
    pub toggle: Toggle,
}

impl Toggle {
    /// How far its arrow turns, in radians: down when open.
    pub const fn angle(self) -> f32 {
        match self {
            Self::Open => FRAC_PI_2,
            Self::Closed | Self::None => 0.0,
        }
    }
}

impl TreeRow {
    /// Whether it is the tree's root, Home.
    pub const fn is_root(&self) -> bool {
        self.depth == 0
    }

    /// Whether the window is in its folder: `here` is the folder on
    /// screen, if any.
    pub fn is_here(&self, here: Option<&Path>) -> bool {
        here == Some(self.path.as_path())
    }
}

impl FolderTree {
    /// A tree under `root`, closed.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            open: HashSet::new(),
            children: HashMap::new(),
            waiting: VecDeque::new(),
        }
    }

    /// The rows as drawn, from the root down, each open folder followed
    /// by its subfolders.
    pub fn rows(&self) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        self.push_rows(&self.root, folder_name(&self.root), 0, &mut rows);
        rows
    }

    fn push_rows(&self, path: &Path, name: SharedString, depth: usize, rows: &mut Vec<TreeRow>) {
        let children = self.children.get(path);
        let toggle = match children {
            Some(children) if children.is_empty() => Toggle::None,
            _ if self.open.contains(path) => Toggle::Open,
            _ => Toggle::Closed,
        };
        rows.push(TreeRow {
            path: path.to_owned(),
            name,
            depth,
            toggle,
        });
        if toggle != Toggle::Open {
            return;
        }
        for child in children.into_iter().flatten() {
            let child_path = path.join(child);
            let child_name = child.to_string_lossy().into_owned().into();
            self.push_rows(&child_path, child_name, depth + 1, rows);
        }
    }

    /// Opens `path` if closed, closes it if open. An opened folder is
    /// listed again, and its subfolders known to hold none are unknown
    /// again, and closed, so their arrow lists them when pressed.
    pub fn toggle(&mut self, path: &Path) {
        if !self.open.remove(path) {
            self.open.insert(path.to_owned());
            let emptied: Vec<PathBuf> = self
                .children
                .iter()
                .filter(|(child, held)| held.is_empty() && child.parent() == Some(path))
                .map(|(child, _)| child.clone())
                .collect();
            for child in emptied {
                self.children.remove(&child);
                self.open.remove(&child);
            }
            self.wait_for(path);
        }
    }

    /// Opens each folder from the root down to the one holding `here`, so
    /// `here`'s row shows; lists those not listed yet. Nothing when `here`
    /// isn't under the root.
    pub fn reveal(&mut self, here: &Path) {
        let Ok(inside) = here.strip_prefix(&self.root) else {
            return;
        };
        let mut folder = self.root.clone();
        let mut parts = inside.components().peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                break;
            }
            self.open_listed(&folder);
            folder.push(part);
        }
        if folder != here {
            self.open_listed(&folder);
        }
    }

    /// Opens `folder`, listing it unless it is listed already.
    fn open_listed(&mut self, folder: &Path) {
        if self.open.insert(folder.to_owned()) && !self.children.contains_key(folder) {
            self.wait_for(folder);
        }
    }

    fn wait_for(&mut self, folder: &Path) {
        if !self.waiting.iter().any(|waiting| waiting == folder) {
            self.waiting.push_back(folder.to_owned());
        }
    }

    /// The next folder to list, taken off the queue.
    pub fn next_to_list(&mut self) -> Option<PathBuf> {
        self.waiting.pop_front()
    }

    /// Takes `entries`, a listing of `folder`, as its subfolders.
    pub fn listed(&mut self, folder: &Path, entries: &[Entry]) {
        let mut names: Vec<OsString> = entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::Folder)
            .filter(|entry| entry.name.as_bytes().first() != Some(&HIDDEN))
            .map(|entry| entry.name.clone())
            .collect();
        names.sort_by(|a, b| by_name(a, b));
        self.children.insert(folder.to_owned(), names);
    }

    /// `folder` couldn't be listed: it shows no subfolders.
    pub fn unreadable(&mut self, folder: &Path) {
        self.children.insert(folder.to_owned(), Vec::new());
    }
}

/// A to Z as a person reads it, ignoring case; names that read the same
/// by their bytes.
fn by_name(a: &OsStr, b: &OsStr) -> Ordering {
    let read = |name: &OsStr| name.to_string_lossy().to_lowercase();
    read(a).cmp(&read(b)).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use nova_files_api::EntryId;

    use super::*;

    fn entries(folders: &[&str], files: &[&str]) -> Vec<Entry> {
        let entry = |(index, (name, kind)): (usize, (&&str, EntryKind))| Entry {
            id: EntryId(u32::try_from(index).unwrap()),
            name: OsString::from(name),
            kind,
        };
        folders
            .iter()
            .map(|name| (name, EntryKind::Folder))
            .chain(files.iter().map(|name| (name, EntryKind::File)))
            .enumerate()
            .map(entry)
            .collect()
    }

    fn shown(tree: &FolderTree) -> Vec<(String, usize, Toggle)> {
        tree.rows()
            .into_iter()
            .map(|row| (row.name.to_string(), row.depth, row.toggle))
            .collect()
    }

    fn home() -> FolderTree {
        FolderTree::new(PathBuf::from("/home/me"))
    }

    #[test]
    fn a_new_tree_shows_its_root_closed() {
        let tree = home();
        assert_eq!(shown(&tree), [("me".to_owned(), 0, Toggle::Closed)]);
    }

    #[test]
    fn opening_lists_and_shows_subfolders_a_to_z_without_hidden_ones() {
        let mut tree = home();
        tree.toggle(Path::new("/home/me"));
        assert_eq!(tree.next_to_list(), Some(PathBuf::from("/home/me")));
        assert_eq!(tree.next_to_list(), None);
        tree.listed(
            Path::new("/home/me"),
            &entries(&["music", ".config", "Desktop"], &["notes.txt"]),
        );
        assert_eq!(
            shown(&tree),
            [
                ("me".to_owned(), 0, Toggle::Open),
                ("Desktop".to_owned(), 1, Toggle::Closed),
                ("music".to_owned(), 1, Toggle::Closed),
            ]
        );
        assert_eq!(tree.rows()[2].path, Path::new("/home/me/music"));
    }

    #[test]
    fn a_folder_listed_empty_loses_its_arrow_and_one_unreadable_too() {
        let mut tree = home();
        tree.toggle(Path::new("/home/me"));
        tree.listed(Path::new("/home/me"), &entries(&["a", "b"], &[]));
        tree.listed(Path::new("/home/me/a"), &entries(&[], &["x"]));
        tree.unreadable(Path::new("/home/me/b"));
        let toggles: Vec<Toggle> = tree.rows().iter().map(|row| row.toggle).collect();
        assert_eq!(toggles, [Toggle::Open, Toggle::None, Toggle::None]);
    }

    #[test]
    fn closing_hides_subfolders_and_opening_again_lists_again() {
        let mut tree = home();
        let root = Path::new("/home/me");
        tree.toggle(root);
        tree.next_to_list();
        tree.listed(root, &entries(&["a"], &[]));
        tree.toggle(root);
        assert_eq!(shown(&tree), [("me".to_owned(), 0, Toggle::Closed)]);
        assert_eq!(tree.next_to_list(), None);
        tree.toggle(root);
        assert_eq!(tree.next_to_list(), Some(root.to_owned()));
        assert_eq!(shown(&tree).len(), 2);
    }

    #[test]
    fn revealing_opens_the_folders_down_to_the_one_holding_it() {
        let mut tree = home();
        tree.reveal(Path::new("/home/me/Projects/nova/src"));
        let waiting: Vec<PathBuf> = std::iter::from_fn(|| tree.next_to_list()).collect();
        assert_eq!(
            waiting,
            [
                PathBuf::from("/home/me"),
                PathBuf::from("/home/me/Projects"),
                PathBuf::from("/home/me/Projects/nova"),
            ]
        );
        tree.listed(Path::new("/home/me"), &entries(&["Projects"], &[]));
        tree.listed(Path::new("/home/me/Projects"), &entries(&["nova"], &[]));
        tree.listed(Path::new("/home/me/Projects/nova"), &entries(&["src"], &[]));
        let rows = tree.rows();
        assert_eq!(
            rows.last().unwrap().path,
            Path::new("/home/me/Projects/nova/src")
        );
        assert_eq!(rows.last().unwrap().toggle, Toggle::Closed);
    }

    #[test]
    fn revealing_lists_only_what_isnt_listed_and_skips_paths_outside() {
        let mut tree = home();
        tree.listed(Path::new("/home/me"), &entries(&["a"], &[]));
        tree.reveal(Path::new("/home/me/a"));
        assert_eq!(tree.next_to_list(), None);
        assert_eq!(shown(&tree).len(), 2);
        tree.reveal(Path::new("/home/me"));
        tree.reveal(Path::new("/etc/nix"));
        assert_eq!(tree.next_to_list(), None);
    }

    #[test]
    fn opening_a_parent_again_gives_an_empty_subfolder_its_arrow_back() {
        let mut tree = home();
        let root = Path::new("/home/me");
        tree.toggle(root);
        tree.listed(root, &entries(&["a"], &[]));
        tree.listed(Path::new("/home/me/a"), &entries(&[], &[]));
        tree.listed(root, &entries(&["a"], &[]));
        assert_eq!(tree.rows()[1].toggle, Toggle::None);
        tree.toggle(root);
        tree.toggle(root);
        assert_eq!(tree.rows()[1].toggle, Toggle::Closed);
    }

    #[test]
    fn an_opened_empty_subfolder_closes_when_its_parent_opens_again() {
        let mut tree = home();
        let root = Path::new("/home/me");
        let a = Path::new("/home/me/a");
        tree.toggle(root);
        tree.listed(root, &entries(&["a"], &[]));
        tree.toggle(a);
        tree.listed(a, &entries(&[], &[]));
        tree.toggle(root);
        tree.toggle(root);
        assert_eq!(tree.rows()[1].toggle, Toggle::Closed);
        while tree.next_to_list().is_some() {}
        tree.toggle(a);
        assert_eq!(tree.next_to_list(), Some(a.to_owned()));
    }

    #[test]
    fn opening_a_folder_forgets_only_its_own_empty_subfolders() {
        let mut tree = home();
        let root = Path::new("/home/me");
        let a = Path::new("/home/me/a");
        tree.toggle(root);
        tree.listed(root, &entries(&["a", "b"], &[]));
        tree.toggle(a);
        tree.listed(a, &entries(&["x"], &[]));
        tree.listed(Path::new("/home/me/b"), &entries(&[], &[]));
        tree.listed(Path::new("/home/me/a/x"), &entries(&[], &[]));
        // Opening a again forgets x's emptiness, not b's.
        tree.toggle(a);
        tree.toggle(a);
        let shown = shown(&tree);
        assert_eq!(shown[2], ("x".to_owned(), 2, Toggle::Closed));
        assert_eq!(shown[3], ("b".to_owned(), 1, Toggle::None));
        // Opening the root again keeps what a holds.
        tree.toggle(root);
        tree.toggle(root);
        assert_eq!(tree.rows().len(), 4);
    }

    #[test]
    fn a_folder_waits_once_however_often_it_is_asked_for() {
        let mut tree = home();
        let root = Path::new("/home/me");
        tree.toggle(root);
        tree.toggle(root);
        tree.toggle(root);
        assert_eq!(tree.next_to_list(), Some(root.to_owned()));
        assert_eq!(tree.next_to_list(), None);
    }

    #[test]
    fn only_an_open_arrow_turns_down() {
        assert!((Toggle::Open.angle() - FRAC_PI_2).abs() < f32::EPSILON);
        assert!(Toggle::Closed.angle().abs() < f32::EPSILON);
        assert!(Toggle::None.angle().abs() < f32::EPSILON);
    }

    #[test]
    fn the_root_is_home_and_a_row_is_here_only_in_its_folder() {
        let mut tree = home();
        tree.toggle(Path::new("/home/me"));
        tree.listed(Path::new("/home/me"), &entries(&["a"], &[]));
        let rows = tree.rows();
        assert!(rows[0].is_root());
        assert!(!rows[1].is_root());
        assert!(rows[1].is_here(Some(Path::new("/home/me/a"))));
        assert!(!rows[1].is_here(Some(Path::new("/home/me"))));
        assert!(!rows[1].is_here(None));
    }

    #[test]
    fn names_go_a_to_z_ignoring_case() {
        let mut names = [
            OsString::from("b"),
            OsString::from("A"),
            OsString::from("a"),
        ];
        names.sort_by(|a, b| by_name(a, b));
        assert_eq!(
            names,
            [
                OsString::from("A"),
                OsString::from("a"),
                OsString::from("b")
            ]
        );
    }
}
