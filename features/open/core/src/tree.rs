use std::fs;

use nova_files_test_folder::TestFolder;

use super::defaults::Places;

pub struct Tree(pub TestFolder);

impl Tree {
    pub fn new(name: &str) -> Self {
        Self(TestFolder::new(&format!("open-{name}")))
    }

    pub fn write(&self, path: &str, text: &str) -> &Self {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        self
    }

    pub fn places_in(&self, config: &[&str], data: &[&str], desktops: &[&str]) -> Places {
        Places {
            config: config.iter().map(|folder| self.0.join(folder)).collect(),
            config_home: config.first().map(|folder| self.0.join(folder)),
            data: data.iter().map(|folder| self.0.join(folder)).collect(),
            desktops: desktops.iter().map(|&desktop| desktop.to_owned()).collect(),
            search_path: None,
        }
    }

    pub fn places(&self) -> Places {
        self.places_in(&["config"], &["share"], &[])
    }
}
