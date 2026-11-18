use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::Context;
use nova_files_api::{Command, Properties, PropertiesId};

use super::MainWindow;
use crate::location::Location;

#[derive(Debug)]
pub struct Page {
    items: Vec<Item>,
    pub(super) measure: Option<super::measuring::Measure>,
    pub(super) opens_with: super::opens_with::OpensWith,
    pub(super) inside: bool,
    asked: HashMap<PropertiesId, usize>,
    gone: usize,
}

#[derive(Debug)]
struct Item {
    path: PathBuf,
    read: Option<Arc<Properties>>,
    gone: bool,
}

impl Page {
    fn new(paths: &[PathBuf]) -> Self {
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
            opens_with: super::opens_with::OpensWith::default(),
            inside: false,
        }
    }

    pub(super) fn paths(&self) -> Vec<PathBuf> {
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
}

impl MainWindow {
    pub fn show_properties(&mut self, cx: &mut Context<Self>) {
        if self.in_trash() {
            return;
        }
        let paths = self.selected_paths();
        if !paths.is_empty() {
            self.new_tab(&Location::Properties(paths), cx);
        }
    }

    pub fn properties_paths(&self) -> Option<&[PathBuf]> {
        match self.location()? {
            Location::Properties(paths) => Some(paths),
            Location::Start | Location::Folder(_) | Location::Gathered(_) | Location::Page(_) => None,
        }
    }

    pub fn properties_page(&self) -> Option<&Page> {
        self.properties_paths()?;
        self.properties.as_ref()
    }

    pub(crate) fn properties_folder(&self) -> Option<&Path> {
        let paths = self.properties_paths()?;
        let folder = paths.first()?.parent()?;
        paths
            .iter()
            .all(|path| path.parent() == Some(folder))
            .then_some(folder)
    }

    pub(super) fn arrive_at_properties(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        self.let_go_of_folder(cx);
        if self.properties.is_none() {
            self.properties = Some(Page::new(paths));
        }
        self.history.arrived(&Location::Properties(paths.to_vec()));
        self.ask_properties();
        self.measure();
        self.ask_apps();
        cx.notify();
    }

    pub(super) fn ask_properties(&mut self) {
        let Some(mut page) = self.properties.take() else {
            return;
        };
        page.asked.clear();
        for (index, item) in page.items.iter().enumerate() {
            let id = PropertiesId(self.next_ask());
            let sent = self.connection.as_ref().map(|connection| {
                connection.send(Command::Properties {
                    id,
                    path: item.path.clone(),
                })
            });
            if matches!(sent, Some(Ok(()))) {
                page.asked.insert(id, index);
            }
        }
        self.properties = Some(page);
    }

    pub(super) fn properties_arrived(
        &mut self,
        id: PropertiesId,
        read: Option<Arc<Properties>>,
        cx: &mut Context<Self>,
    ) {
        let Some(page) = self.properties.as_mut() else {
            return;
        };
        let Some(item) = page
            .asked
            .remove(&id)
            .and_then(|index| page.items.get_mut(index))
        else {
            return;
        };
        let gone = read.is_none();
        if gone != item.gone {
            if gone {
                page.gone += 1;
            } else {
                page.gone -= 1;
            }
        }
        item.gone = gone;
        item.read = read;
        let all_gone = page.gone == page.items.len();
        if all_gone && self.properties_page().is_some() {
            self.open_start(cx);
        }
        cx.notify();
    }
}
