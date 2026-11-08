//! Pick mode: the main window opened for another app's Open dialog.
//!
//! The window holds the app's request and answers it once: a double-click
//! on a file picks it when the app asked for files, and the window then
//! closes. Closing the window any other way drops the request, which
//! answers cancelled. The portal may close the request first; [`close`]
//! then closes its window. Picking folders, saving, the type filter and
//! the bar with the name box come with the pick-mode bar; until then
//! those requests open no window and answer cancelled.

use std::io::{self, Write as _};

use gpui::{App, ClickEvent, TitlebarOptions, Window};
use nova_files_api::{EntryId, EntryKind, ListingId, PickAnswer, PickId, PickKind, PickRequest};

use crate::window::MainWindow;

/// The clicks in a double-click.
const DOUBLE_CLICK: usize = 2;

/// A picker's title when the asking app gives none.
const TITLE: &str = "Open";

/// The title bar of a picker for an app that asked with `title`; the
/// portal lets an app leave it out.
pub fn titlebar(title: &str) -> TitlebarOptions {
    let title = if title.is_empty() { TITLE } else { title };
    TitlebarOptions {
        title: Some(title.to_owned().into()),
        ..TitlebarOptions::default()
    }
}

/// Whether a picker window can answer `request`: for now, only an Open
/// dialog for files.
pub const fn can_answer(request: &PickRequest) -> bool {
    matches!(request.kind, PickKind::Open { folders: false, .. })
}

impl MainWindow {
    /// Puts the window in pick mode for `request`.
    pub fn set_pick(&mut self, request: Box<PickRequest>) {
        self.pick = Some(request);
    }

    /// The request the window answers, while it is a picker that hasn't.
    pub fn pick_id(&self) -> Option<PickId> {
        self.pick.as_ref().map(|request| request.id)
    }

    /// The listing on screen, which names the entries a pick refers to.
    pub fn listing_id(&self) -> Option<ListingId> {
        self.listing.as_ref().map(|listing| listing.id)
    }

    /// A click on the file `id` of `listing`: a double-click picks it and
    /// closes the window.
    pub(crate) fn file_clicked(
        &mut self,
        listing: ListingId,
        id: EntryId,
        event: &ClickEvent,
        window: &mut Window,
    ) {
        if event.click_count() >= DOUBLE_CLICK && self.pick_entry(listing, id) {
            window.remove_window();
        }
    }

    /// Picks the file `id` of `listing` and answers the app. Whether it
    /// answered, so the window can close. Nothing is picked once another
    /// listing replaced that one, or while a folder asked for is loading,
    /// since the user has already moved on.
    pub fn pick_entry(&mut self, listing: ListingId, id: EntryId) -> bool {
        if self.waiting() {
            return false;
        }
        let Some(shown) = self.listing.as_ref().filter(|shown| shown.id == listing) else {
            return false;
        };
        let entry = usize::try_from(id.0)
            .ok()
            .and_then(|index| shown.entries.get(index))
            .filter(|entry| entry.kind == EntryKind::File);
        let Some(path) = entry.map(|entry| shown.path.join(&entry.name)) else {
            return false;
        };
        let Some(request) = self.pick.take_if(|request| can_answer(request)) else {
            return false;
        };
        let filter = request.filter;
        request.reply.answer(PickAnswer::Picked {
            paths: vec![path],
            filter,
        });
        true
    }
}

/// Closes the picker window for `id`, if one is open, and drops its
/// request, which answers cancelled.
pub fn close(id: PickId, cx: &mut App) {
    for handle in cx.windows() {
        let Some(handle) = handle.downcast::<MainWindow>() else {
            continue;
        };
        let closed = handle.update(cx, |view, window, _| {
            if view.pick_id() == Some(id) {
                // Answered now, not when the window's view is freed.
                view.pick = None;
                window.remove_window();
            }
        });
        if let Err(error) = closed {
            // A window that is going away anyway; its request is answered
            // when it is freed. Until there is a log, stderr is where this
            // goes.
            writeln!(
                io::stderr(),
                "nova-files: could not close a picker: {error:#}"
            )
            .ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use nova_files_api::PickReply;

    use super::*;

    fn request(kind: PickKind) -> PickRequest {
        PickRequest {
            id: PickId(1),
            kind,
            title: String::new(),
            accept_label: None,
            filters: Vec::new(),
            filter: None,
            folder: None,
            reply: PickReply::new(|_| {}),
        }
    }

    #[test]
    fn a_picker_is_titled_as_its_app_asks_or_open() {
        let title = |asked| titlebar(asked).title.map(|title| title.to_string());
        assert_eq!(title("Upload photos"), Some("Upload photos".to_owned()));
        assert_eq!(title(""), Some("Open".to_owned()));
    }

    #[test]
    fn only_an_open_dialog_for_files_gets_a_window_for_now() {
        let files = PickKind::Open {
            multiple: true,
            folders: false,
        };
        let folders = PickKind::Open {
            multiple: false,
            folders: true,
        };
        let save = PickKind::Save {
            name: "a.txt".into(),
        };
        assert!(can_answer(&request(files)));
        assert!(!can_answer(&request(folders)));
        assert!(!can_answer(&request(save)));
    }
}
