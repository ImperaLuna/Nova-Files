use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{AppContext as _, Context, Entity, Subscription};
use nova_files_api::{Command, Listing, ListingId};

use super::MainWindow;
use super::events::Place;
use crate::location::Location;
use crate::pick::typed::HOME;
use nova_files_shell::text::{TextField, TextFieldEvent};

const PLACEHOLDER: &str = "Search";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    Everywhere,
    HereOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTo {
    Box,
    Results,
    List,
}

pub struct Search {
    pub(crate) field: Entity<TextField>,
    pub(crate) scope: Scope,
    from: Option<Location>,
    pub(crate) asked: Option<ListingId>,
    pub(crate) failed: bool,
    pub(crate) focus_to: Option<FocusTo>,
    pub(crate) reveal: Option<PathBuf>,
    _changes: Subscription,
}

impl Search {
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
            reveal: None,
            _changes: changes,
        }
    }

    pub(crate) const fn from(&self) -> Option<&Location> {
        self.from.as_ref()
    }

    pub(super) const fn folder_asked(&mut self) {
        self.asked = None;
    }
}

impl MainWindow {
    pub const fn searching(&self) -> bool {
        self.search.from.is_some()
    }

    pub fn query(&self, cx: &gpui::App) -> String {
        self.search.field.read(cx).text().to_owned()
    }

    fn query_changed(&mut self, cx: &mut Context<Self>) {
        if self.query(cx).trim().is_empty() {
            self.end_search(cx);
        } else {
            self.find(cx);
        }
        cx.notify();
    }

    pub(crate) fn find(&mut self, cx: &Context<Self>) {
        let on_its_way = self.asked.and_then(|_| self.requested.clone());
        let Some(from) = on_its_way
            .or_else(|| self.search.from.clone())
            .or_else(|| self.place_on_screen_now())
        else {
            return;
        };
        let id = ListingId(self.next_listing);
        self.next_listing += 1;
        let query = self.query(cx).trim().to_owned();
        let within = (self.search.scope == Scope::HereOnly)
            .then(|| from.folder().map(Path::to_path_buf))
            .flatten();
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

    pub(super) fn found_arrived(&mut self, listing: Listing, cx: &mut Context<Self>) {
        self.search.asked = None;
        self.search.failed = false;
        self.selection = None;
        self.gathered.shown = None;
        self.listed_page = None;
        self.listing_replaced(listing, Place::Top, cx);
        cx.notify();
    }

    pub(super) fn find_failed(&mut self, id: ListingId, cx: &mut Context<Self>) {
        let Some(from) = self.search.from.clone() else {
            return;
        };
        let nothing = Listing {
            id,
            path: from.folder().map(Path::to_path_buf).unwrap_or_default(),
            entries: Arc::new([]),
            folders: Some(Arc::new([])),
        };
        self.found_arrived(nothing, cx);
        self.search.failed = true;
    }

    pub(super) fn search_left(&mut self, cx: &mut Context<Self>) {
        self.search.asked = None;
        if self.search.from.take().is_some() {
            self.search
                .field
                .update(cx, |field, cx| field.set_text("", cx));
        }
    }

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
        }
        self.request_place(&from);
        cx.notify();
    }

    pub(crate) fn showing_found(&self) -> bool {
        self.gathered_shown().is_none()
            && self
                .listing
                .as_ref()
                .is_some_and(|listing| listing.folders.is_some())
    }

    pub(crate) fn reveal(&mut self, path: PathBuf) {
        let Some(folder) = path.parent().map(Path::to_path_buf) else {
            return;
        };
        self.search.reveal = Some(path);
        self.open(&folder);
    }

    pub fn set_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        if self.search.scope == scope {
            return;
        }
        self.search.scope = scope;
        if self.searching() {
            self.find(cx);
        }
        cx.notify();
    }

    pub const fn scope(&self) -> Scope {
        self.search.scope
    }

    pub(crate) fn focus_later(&mut self, to: FocusTo, cx: &mut Context<Self>) {
        self.search.focus_to = Some(to);
        cx.notify();
    }
}

pub fn short_folder(folder: &Path, home: &Path) -> String {
    match folder.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => HOME.to_owned(),
        Ok(rest) => format!("{HOME}/{}", rest.to_string_lossy()),
        Err(_) => folder.to_string_lossy().into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_under_home_start_with_a_tilde() {
        let home = Path::new("/home/me");
        assert_eq!(short_folder(Path::new("/home/me"), home), "~");
        assert_eq!(
            short_folder(Path::new("/home/me/Documents/tax"), home),
            "~/Documents/tax"
        );
        assert_eq!(short_folder(Path::new("/home/meg"), home), "/home/meg");
        assert_eq!(short_folder(Path::new("/etc"), home), "/etc");
    }
}
