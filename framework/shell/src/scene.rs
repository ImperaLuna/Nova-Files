use crate::location::Location;
use crate::rules::FolderRules;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    pub location: Option<Location>,
    pub rules: FolderRules,
    pub selected: usize,
    pub picker: bool,
}

impl Scene {
    pub fn on_page<K: crate::PageKind>(&self) -> Option<&K> {
        self.location.as_ref()?.page()
    }

    pub fn in_folder(&self, folder: &std::path::Path) -> bool {
        self.location
            .as_ref()
            .and_then(Location::folder)
            .is_some_and(|here| here == folder)
    }
}
