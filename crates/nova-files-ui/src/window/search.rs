//! Searching from the window: what is typed in the top bar's search box
//! is looked for across the whole index (or only inside the folder the
//! search started in, see [`Scope`]), and the files found take the list's
//! place until the box is cleared.
//!
//! Each change to the box asks the core for a `Find` with a new listing
//! id; the answer for that id replaces the list, older ones are dropped
//! (see `events`). The folder the search started in stays the window's
//! folder meanwhile: the path, Up and Here only go by it, and the history
//! doesn't change. Clearing the box, or Escape in it, reads that folder
//! again. Going anywhere else ends the search once that folder arrives,
//! so a folder that can't be read leaves the search as it was. A find
//! asked for while a folder is on its way replaces it (the core keeps one
//! listing per window), so the search then starts from that folder.
//! A find that can't run shows that, not an empty result.
//!
//! Keys: `/` or Ctrl+F put the caret in the box; Down or Enter in it move
//! into the results; Escape in it clears it. A focus change waits for the
//! next frame, which has a window to focus in (see `view`).
//!
//! A picker doesn't search yet: its rows answer the asking app by name in
//! the folder on screen.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{AppContext as _, Context, Entity, Subscription};
use nova_files_api::{Command, Listing, ListingId};

use super::MainWindow;
use crate::text::{TextField, TextFieldEvent};

const PLACEHOLDER: &str = "Search";

/// Where a search looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    /// The whole index.
    #[default]
    Everywhere,
    /// Inside the folder the search started in, at any depth.
    HereOnly,
}

/// Where the keyboard should go once the window next draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTo {
    /// The search box, its text selected.
    Box,
    /// The list, its first row under the cursor if none is.
    Results,
}

/// The window's search: the box, and the search under way.
pub struct Search {
    pub(crate) field: Entity<TextField>,
    pub(crate) scope: Scope,
    /// The folder the search started in, while one is under way.
    from: Option<PathBuf>,
    /// The find asked for most recently, until its answer arrives.
    pub(crate) asked: Option<ListingId>,
    /// The last find couldn't run.
    pub(crate) failed: bool,
    pub(crate) focus_to: Option<FocusTo>,
    _changes: Subscription,
}

impl Search {
    /// A search box for `window`, empty.
    pub(super) fn new(cx: &mut Context<MainWindow>) -> Self {
        let field = cx.new(|cx| TextField::new(cx).with_placeholder(PLACEHOLDER));
        let changes = cx.subscribe(&field, |this, _, event, cx| match event {
            TextFieldEvent::Changed => this.query_changed(cx),
            TextFieldEvent::Submitted => this.focus_later(FocusTo::Results, cx),
            TextFieldEvent::Cancelled => {
                this.end_search(cx);
                this.focus_later(FocusTo::Results, cx);
            }
        });
        Self {
            field,
            scope: Scope::default(),
            from: None,
            asked: None,
            failed: false,
            focus_to: None,
            _changes: changes,
        }
    }

    /// The folder the search started in, while one is under way.
    pub(crate) fn from(&self) -> Option<&Path> {
        self.from.as_deref()
    }

    /// The window asked for a folder, which replaces any find on its way.
    pub(super) const fn folder_asked(&mut self) {
        self.asked = None;
    }
}

impl MainWindow {
    /// Whether a search is under way.
    pub fn searching(&self) -> bool {
        self.search.from().is_some()
    }

    /// The text in the search box.
    pub fn query(&self, cx: &gpui::App) -> String {
        self.search.field.read(cx).text().to_owned()
    }

    /// Searches for what the box holds now, or ends the search when it
    /// holds only spaces.
    fn query_changed(&mut self, cx: &mut Context<Self>) {
        if self.query(cx).trim().is_empty() {
            self.end_search(cx);
        } else {
            self.find(cx);
        }
        cx.notify();
    }

    /// Asks the core for what the box holds, in the search's scope. A
    /// folder still on its way is given up, and the search starts there.
    pub(crate) fn find(&mut self, cx: &Context<Self>) {
        let on_its_way = self.asked.and_then(|_| self.requested.clone());
        let Some(from) = on_its_way
            .or_else(|| self.search.from.clone())
            .or_else(|| self.folder().map(Path::to_path_buf))
        else {
            return;
        };
        let id = ListingId(self.next_listing);
        self.next_listing += 1;
        let query = self.query(cx).trim().to_owned();
        let within = (self.search.scope == Scope::HereOnly).then(|| from.clone());
        let sent = self
            .connection
            .as_ref()
            .map(|connection| connection.send(Command::Find { id, query, within }));
        if matches!(sent, Some(Ok(()))) {
            self.asked = None;
            self.search.asked = Some(id);
            self.search.from = Some(from);
        }
    }

    /// Shows `listing`, the files the search asked for last found.
    pub(super) fn found_arrived(&mut self, listing: Listing, cx: &mut Context<Self>) {
        self.search.asked = None;
        self.search.failed = false;
        self.selection = None;
        self.listing_replaced(listing, false, cx);
        cx.notify();
    }

    /// The find asked for last, `id`, couldn't run: the list empties and
    /// says so.
    pub(super) fn find_failed(&mut self, id: ListingId, cx: &mut Context<Self>) {
        let Some(from) = self.search.from.clone() else {
            return;
        };
        let nothing = Listing {
            id,
            path: from,
            entries: Arc::new([]),
            folders: Some(Arc::new([])),
        };
        self.found_arrived(nothing, cx);
        self.search.failed = true;
    }

    /// A folder arrived: the search under way, if any, ends.
    pub(super) fn search_left(&mut self, cx: &mut Context<Self>) {
        self.search.asked = None;
        if self.search.from.take().is_some() {
            self.search
                .field
                .update(cx, |field, cx| field.set_text("", cx));
        }
    }

    /// Empties the box and reads the folder the search started in again,
    /// if its results took the list's place.
    pub(crate) fn end_search(&mut self, cx: &mut Context<Self>) {
        self.search
            .field
            .update(cx, |field, cx| field.set_text("", cx));
        self.search.asked = None;
        let Some(from) = self.search.from.take() else {
            return;
        };
        if self.showing_found() {
            self.selection = None;
            self.request(&from);
        }
        cx.notify();
    }

    /// Whether the list shows files a search found.
    pub(crate) fn showing_found(&self) -> bool {
        self.listing
            .as_ref()
            .is_some_and(|listing| listing.folders.is_some())
    }

    /// Moves the keyboard to `to` once the window next draws.
    pub(crate) fn focus_later(&mut self, to: FocusTo, cx: &mut Context<Self>) {
        self.search.focus_to = Some(to);
        cx.notify();
    }
}
