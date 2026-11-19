use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Context, FocusHandle};
use nova_files_api::{Listing, ListingId};
use nova_files_shell::typed::HOME;
use nova_files_shell::{Entries, Laid, LaidOver, OverWant};

use super::MainWindow;
use super::events::Place;
use crate::location::Location;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusTo {
    Results,
    List,
    Handle(FocusHandle),
}

#[derive(Debug, Default)]
pub struct Over {
    from: Option<Location>,
    pub(crate) asked: Option<ListingId>,
    pub(crate) failed: bool,
    pub(crate) focus_to: Option<FocusTo>,
    pub(crate) reveal: Option<PathBuf>,
    laid: Option<LaidOver>,
    pub(crate) from_has_folders: bool,
}

impl Over {
    pub(crate) const fn from(&self) -> Option<&Location> {
        self.from.as_ref()
    }

    pub(super) const fn folder_asked(&mut self) {
        self.asked = None;
    }

    pub(crate) fn marks(&self) -> Option<&str> {
        self.from
            .as_ref()
            .and(self.laid.as_ref())
            .map(|laid| laid.marks.as_str())
    }
}

impl MainWindow {
    pub const fn searching(&self) -> bool {
        self.search.from.is_some()
    }

    pub(super) const fn laid(&self) -> Laid {
        match (self.searching(), self.search.failed) {
            (false, _) => Laid::Off,
            (true, false) => Laid::Over,
            (true, true) => Laid::Failed,
        }
    }

    pub(super) fn over_wanted(&mut self, want: OverWant, cx: &mut Context<Self>) {
        match want {
            OverWant::Lay(laid) => {
                self.search.laid = Some(laid);
                self.find();
                cx.notify();
            }
            OverWant::TakeBack => self.end_search(cx),
        }
    }

    pub(crate) fn find(&mut self) {
        let on_its_way = self.asked.and_then(|_| self.requested.clone());
        let Some(from) = on_its_way
            .or_else(|| self.search.from.clone())
            .or_else(|| self.place_on_screen_now())
        else {
            return;
        };
        let Some(laid) = &self.search.laid else {
            return;
        };
        let id = ListingId(self.next_listing);
        self.next_listing += 1;
        let ask = (laid.ask)(id, &from.shell());
        let sent = self
            .connection
            .as_ref()
            .map(|connection| connection.send_envelope(ask));
        if matches!(sent, Some(Ok(()))) {
            if !self.searching() {
                self.search.from_has_folders = self.asked.is_none() && self.has_folders();
            }
            self.asked = None;
            self.search.asked = Some(id);
            self.search.from = Some(from);
        }
    }

    pub(super) fn found_arrived(&mut self, listing: Listing, cx: &mut Context<Self>) {
        self.search.asked = None;
        self.search.failed = false;
        self.selection = None;
        self.listed_page = None;
        self.listing_replaced(listing, Place::Top);
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
            self.host.entries.update(cx, Entries::over_left);
        }
    }

    fn end_search(&mut self, cx: &mut Context<Self>) {
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
        self.listed_page.is_none()
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
