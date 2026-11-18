//! The Properties page: what the core says of the files and folders a
//! person asked about, in a tab of its own (drawn in `view/properties`).
//!
//! Right-click > Properties, or Alt+Enter, opens the page of the entries
//! selected in a new tab, after the others; never in the trash, whose
//! files can only be put back. The page is a place like Start: it joins
//! the tab's history, needs no listing, and lets go of the folder that was
//! on screen. Arriving asks the core for each path's properties, one
//! request each with a new id, and an item takes the answer to its newest
//! request. Until the
//! answers come the page shows what it had, so a tab shown again draws at
//! once and then catches up with what changed meanwhile.
//!
//! A folder's size, or several paths' total, is measured while the page
//! is on screen (see `measuring`).
//!
//! A path that can't be read any more (deleted, moved, its drive gone)
//! drops off the page; when none is left the tab goes to Start, as a tab
//! on a folder that is gone does.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::Context;
use nova_files_api::{Command, Properties, PropertiesId};

use super::MainWindow;
use crate::location::Location;

/// A Properties page: one item per path, in the order they were selected.
#[derive(Debug)]
pub struct Page {
    items: Vec<Item>,
    /// What is under its paths, as measured so far (see `measuring`).
    pub(super) measure: Option<super::measuring::Measure>,
    /// The item each request out is for, by its id.
    asked: HashMap<PropertiesId, usize>,
    /// How many items are gone.
    gone: usize,
}

/// One path on a Properties page.
#[derive(Debug)]
struct Item {
    path: PathBuf,
    /// What the core said last.
    read: Option<Arc<Properties>>,
    /// The core couldn't look at it: it is left off the page.
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
        }
    }

    /// The paths still on the page, in order.
    pub(super) fn paths(&self) -> Vec<PathBuf> {
        self.items
            .iter()
            .filter(|item| !item.gone)
            .map(|item| item.path.clone())
            .collect()
    }

    /// What the core said of each path still on the page, in order;
    /// empty until every path has an answer.
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

    /// Whether every path's answer is in, or it is gone.
    pub fn complete(&self) -> bool {
        self.items
            .iter()
            .all(|item| item.gone || item.read.is_some())
    }
}

impl MainWindow {
    /// Opens the Properties of the entries selected in a new tab.
    pub fn show_properties(&mut self, cx: &mut Context<Self>) {
        if self.in_trash() {
            return;
        }
        let paths = self.selected_paths();
        if !paths.is_empty() {
            self.new_tab(&Location::Properties(paths), cx);
        }
    }

    /// The paths of the Properties page on screen, if it is.
    pub fn properties_paths(&self) -> Option<&[PathBuf]> {
        match self.location()? {
            Location::Properties(paths) => Some(paths),
            Location::Start | Location::Folder(_) | Location::Gathered(_) => None,
        }
    }

    /// The Properties page on screen, if it is.
    pub fn properties_page(&self) -> Option<&Page> {
        self.properties_paths()?;
        self.properties.as_ref()
    }

    /// The folder that holds the paths of the Properties page on screen,
    /// when one holds them all.
    pub(crate) fn properties_folder(&self) -> Option<&Path> {
        let paths = self.properties_paths()?;
        let folder = paths.first()?.parent()?;
        paths
            .iter()
            .all(|path| path.parent() == Some(folder))
            .then_some(folder)
    }

    /// Shows the Properties page of `paths`, leaving the history's trip
    /// as it is.
    pub(super) fn arrive_at_properties(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        self.let_go_of_folder(cx);
        if self.properties.is_none() {
            self.properties = Some(Page::new(paths));
        }
        self.history.arrived(&Location::Properties(paths.to_vec()));
        self.ask_properties();
        self.measure();
        cx.notify();
    }

    /// Asks the core again for each path on the page, gone ones too, so a
    /// path that is back shows again.
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

    /// The answer to request `id`: the path's properties, or `None` when
    /// it couldn't be looked at.
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
