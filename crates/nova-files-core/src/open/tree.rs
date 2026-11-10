//! A scratch folder for the Open tests, removed when dropped.

use std::fs;
use std::path::PathBuf;

use super::defaults::Places;

pub struct Tree(pub PathBuf);

impl Tree {
    /// An empty folder; `name` is unique among the tests.
    pub fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("nova-open-{name}-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    /// Writes `text` to `path` in the tree, making its folders.
    pub fn write(&self, path: &str, text: &str) -> &Self {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        self
    }

    /// Settings in `config`, then data in `data`, for `desktops`; the
    /// first config folder is the user's.
    pub fn places_in(&self, config: &[&str], data: &[&str], desktops: &[&str]) -> Places {
        Places {
            config: config.iter().map(|folder| self.0.join(folder)).collect(),
            config_home: config.first().map(|folder| self.0.join(folder)),
            data: data.iter().map(|folder| self.0.join(folder)).collect(),
            desktops: desktops.iter().map(|&desktop| desktop.to_owned()).collect(),
            search_path: None,
        }
    }

    /// The user's settings in `config`, apps in `share`.
    pub fn places(&self) -> Places {
        self.places_in(&["config"], &["share"], &[])
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}
