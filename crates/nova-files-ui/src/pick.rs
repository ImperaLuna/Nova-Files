//! Pick mode: the main window opened for another app's Open dialog.
//!
//! The window holds the app's request and answers it once: a double-click
//! on a file picks it when the app asked for files, and the window then
//! closes. Closing the window any other way drops the request, which
//! answers cancelled. The portal may close the request first; [`close`]
//! then closes its window. Picking folders, saving, the type filter and
//! the bar with the name box come with the pick-mode bar.

use gpui::{App, ClickEvent, Window};
use nova_files_api::{EntryId, EntryKind, PickAnswer, PickId, PickKind, PickRequest};

use crate::window::MainWindow;

/// The clicks in a double-click.
const DOUBLE_CLICK: usize = 2;

impl MainWindow {
    /// Puts the window in pick mode for `request`.
    pub fn set_pick(&mut self, request: Box<PickRequest>) {
        self.pick = Some(request);
    }

    /// The request the window answers, while it is a picker that hasn't.
    pub fn pick_id(&self) -> Option<PickId> {
        self.pick.as_ref().map(|request| request.id)
    }

    /// Whether a double-click on a file picks it.
    pub(crate) fn picks_files(&self) -> bool {
        self.pick
            .as_ref()
            .is_some_and(|request| matches!(request.kind, PickKind::Open { folders: false, .. }))
    }

    /// A click on the file `id`: a double-click picks it and closes the
    /// window.
    pub(crate) fn file_clicked(&mut self, id: EntryId, event: &ClickEvent, window: &mut Window) {
        if event.click_count() >= DOUBLE_CLICK && self.pick_entry(id) {
            window.remove_window();
        }
    }

    /// Picks the file `id` of the folder on screen and answers the app.
    /// Whether it answered, so the window can close.
    pub fn pick_entry(&mut self, id: EntryId) -> bool {
        if !self.picks_files() {
            return false;
        }
        let Some(listing) = &self.listing else {
            return false;
        };
        let entry = usize::try_from(id.0)
            .ok()
            .and_then(|index| listing.entries.get(index))
            .filter(|entry| entry.kind == EntryKind::File);
        let Some(path) = entry.map(|entry| listing.path.join(&entry.name)) else {
            return false;
        };
        let Some(request) = self.pick.take() else {
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
        handle
            .update(cx, |view, window, _| {
                if view.pick_id() == Some(id) {
                    // Answered now, not when the window's view is freed.
                    view.pick = None;
                    window.remove_window();
                }
            })
            .ok();
    }
}
