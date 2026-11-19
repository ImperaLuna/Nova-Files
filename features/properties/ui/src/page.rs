use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use nova_files_open_api::{App, OpenWithId};
use nova_files_properties_api::{MeasureId, Properties, PropertiesId, ScanState, Size};

#[derive(Debug)]
pub struct Page {
    pub(crate) items: Vec<Item>,
    pub(crate) measure: Option<Measure>,
    pub(crate) opens_with: OpensWith,
    pub(crate) inside: bool,
    pub(crate) asked: HashMap<PropertiesId, usize>,
    pub(crate) gone: usize,
}

#[derive(Debug)]
pub struct Item {
    pub(crate) path: PathBuf,
    pub(crate) read: Option<Arc<Properties>>,
    pub(crate) gone: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measure {
    pub(crate) id: MeasureId,
    pub size: Size,
    pub state: ScanState,
    pub(crate) before: Option<Size>,
}

#[derive(Debug, Default)]
pub struct OpensWith {
    pub(crate) asked: Option<OpenWithId>,
    pub(crate) mime: Option<String>,
    pub(crate) apps: Vec<App>,
}

impl Measure {
    pub const fn shown(&self) -> Size {
        match (self.state, self.before) {
            (ScanState::Counting, Some(before)) => before,
            _ => self.size,
        }
    }
}

impl Page {
    pub fn new(paths: &[PathBuf]) -> Self {
        let items = paths
            .iter()
            .map(|path| Item {
                path: path.clone(),
                read: None,
                gone: false,
            })
            .collect();
        Self {
            items,
            asked: HashMap::new(),
            gone: 0,
            measure: None,
            opens_with: OpensWith::default(),
            inside: false,
        }
    }

    pub fn same_paths(&self, paths: &[PathBuf]) -> bool {
        self.items.len() == paths.len()
            && self
                .items
                .iter()
                .zip(paths)
                .all(|(item, path)| item.path == *path)
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.items
            .iter()
            .filter(|item| !item.gone)
            .map(|item| item.path.clone())
            .collect()
    }

    pub fn read(&self) -> Vec<Arc<Properties>> {
        if !self.complete() {
            return Vec::new();
        }
        self.items
            .iter()
            .filter(|item| !item.gone)
            .filter_map(|item| item.read.clone())
            .collect()
    }

    pub fn complete(&self) -> bool {
        self.items
            .iter()
            .all(|item| item.gone || item.read.is_some())
    }

    pub const fn measured(&self) -> Option<Measure> {
        self.measure
    }

    pub fn apps(&self) -> &[App] {
        &self.opens_with.apps
    }

    pub fn default_app(&self) -> Option<&App> {
        self.apps().iter().find(|app| app.default)
    }

    pub const fn reaches_inside(&self) -> bool {
        self.inside
    }

    pub fn may_change_access(&self) -> bool {
        let read = self.read();
        !read.is_empty()
            && read
                .iter()
                .all(|one| one.may_change && one.read_only != Some(true))
    }
}
