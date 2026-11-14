//! The folder tree's own line to the core. The tree lists folders on a
//! connection of its own, so its listings never cancel the folder on
//! screen (the core keeps one listing per client). Its events arrive on
//! its own task, like the window's. One listing is asked for at a time;
//! when it answers, the next waiting folder is asked for (see `tree`).
//!
//! The window's own listings feed the tree too: a folder on screen is a
//! folder listed, and the tree opens itself down to it.

use std::path::{Path, PathBuf};

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui::{Context, Task};
use nova_files_api::{Command, Connection, Connector, Event, Listing, ListingId, Order};

use super::{MainWindow, WindowSink};

/// The tree's connection and the folder it asked for.
pub struct TreeLine {
    connection: Option<Connection>,
    /// The listing asked for and its folder, until it answers.
    asked: Option<(ListingId, PathBuf)>,
    next_listing: u64,
    _events: Task<()>,
}

impl TreeLine {
    /// Connects the tree to the core; without a core, it lists nothing.
    pub fn connect(connector: &Connector, cx: &Context<MainWindow>) -> Self {
        let (sink, mut events) = mpsc::unbounded();
        let connection = connector.connect(WindowSink(sink)).ok();
        let events = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this
                    .update(cx, |this, cx| this.tree_event(event, cx))
                    .is_err()
                {
                    return;
                }
            }
        });
        Self {
            connection,
            asked: None,
            next_listing: 0,
            _events: events,
        }
    }
}

impl MainWindow {
    /// Opens or closes the tree's folder at `path`.
    pub fn toggle_tree(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.tree.toggle(path);
        self.list_tree_folder();
        cx.notify();
    }

    /// Takes the window's own listing of a folder into the tree, and
    /// opens the tree down to it.
    pub(super) fn tree_follows(&mut self, listing: &Listing) {
        self.tree.listed(&listing.path, &listing.entries);
        self.tree.reveal(&listing.path);
        self.list_tree_folder();
    }

    fn tree_event(&mut self, event: Event, cx: &mut Context<Self>) {
        let asked = self.tree_line.asked.as_ref().map(|(id, _)| *id);
        match event {
            Event::FolderListed(listing) if asked == Some(listing.id) => {
                self.tree_line.asked = None;
                self.tree.listed(&listing.path, &listing.entries);
            }
            Event::FolderUnreadable { id, path, .. } if asked == Some(id) => {
                self.tree_line.asked = None;
                self.tree.unreadable(&path);
            }
            // The tree asks for nothing else.
            _ => return,
        }
        self.list_tree_folder();
        cx.notify();
    }

    /// Asks for the next waiting folder, unless a listing is under way.
    fn list_tree_folder(&mut self) {
        let line = &mut self.tree_line;
        if line.asked.is_some() {
            return;
        }
        let Some(connection) = &line.connection else {
            return;
        };
        let Some(path) = self.tree.next_to_list() else {
            return;
        };
        let id = ListingId(line.next_listing);
        line.next_listing += 1;
        let sent = connection.send(Command::ListFolder {
            id,
            path: path.clone(),
            order: Order::default(),
        });
        // A core gone ends the window's events, which shows it; the tree
        // stays as it is.
        if sent.is_ok() {
            line.asked = Some((id, path));
        }
    }
}
