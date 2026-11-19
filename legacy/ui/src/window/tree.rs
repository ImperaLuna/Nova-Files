use std::path::{Path, PathBuf};

use gpui::Context;
use nova_files_api::{Command, Event, Listing, ListingId, Order};
use nova_files_shell::{Line, Lines};

use super::MainWindow;

pub struct TreeLine {
    line: Line,
    asked: Option<(ListingId, PathBuf)>,
    next_listing: u64,
}

impl TreeLine {
    pub fn connect(lines: &Lines, cx: &mut Context<MainWindow>) -> Self {
        let line = lines.open(cx, |this, envelope, cx| {
            if let Some(event) = envelope.taken_as::<Event>() {
                this.tree_event(event, cx);
            }
        });
        Self {
            line,
            asked: None,
            next_listing: 0,
        }
    }
}

impl MainWindow {
    pub fn toggle_tree(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.tree.toggle(path);
        self.list_tree_folder();
        cx.notify();
    }

    pub const fn tree_settled(&self) -> bool {
        self.tree_line.asked.is_none()
    }

    pub(super) fn tree_follows(&mut self, listing: &Listing, arrived: bool) {
        self.tree.listed(&listing.path, &listing.entries);
        if arrived {
            self.tree.reveal(&listing.path);
        }
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
            _ => return,
        }
        self.list_tree_folder();
        cx.notify();
    }

    fn list_tree_folder(&mut self) {
        let line = &mut self.tree_line;
        if line.asked.is_some() {
            return;
        }
        let Some(path) = self.tree.next_to_list() else {
            return;
        };
        let id = ListingId(line.next_listing);
        line.next_listing += 1;
        let sent = line.line.send(Command::ListFolder {
            id,
            path: path.clone(),
            order: Order::default(),
        });
        if sent.is_ok() {
            line.asked = Some((id, path));
        }
    }
}
