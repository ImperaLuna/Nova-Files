//! Pinning folders to Places and taking them out again. The + on the
//! Places head opens a folder picker, a picker window of the app's own,
//! starting at the folder on screen; the folder picked is pinned last. A
//! path is never typed for it. Right-click on a pinned folder offers
//! "Remove from Places". The core saves each change and tells every
//! window (see `places`).
//!
//! The app's own pickers take ids counting down from the top, so they
//! never meet the ids the portal gives its requests, which count up from
//! 0, and the portal closing one of its requests never closes a pin
//! picker.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::channel::oneshot;
use gpui::{Context, Pixels, Point};
use nova_files_api::{
    BookmarkChange, Command, PickAnswer, PickId, PickKind, PickReply, PickRequest,
};

use super::MainWindow;
use crate::assets::Icon;
use crate::menu::{MenuAction, MenuItem};

/// The pin picker's title and its button.
const PICK_TITLE: &str = "Pin a folder";
const PICK_LABEL: &str = "Pin to Places";
const UNPIN_LABEL: &str = "Remove from Places";

/// The id of the app's next own picker.
static NEXT_OWN_PICK: AtomicU64 = AtomicU64::new(u64::MAX);

impl MainWindow {
    /// Opens a folder picker and pins the folder picked.
    pub fn pick_folder_to_pin(&mut self, cx: &mut Context<Self>) {
        let (deliver, answer) = oneshot::channel();
        let request = PickRequest {
            id: PickId(NEXT_OWN_PICK.fetch_sub(1, Ordering::Relaxed)),
            kind: PickKind::Open {
                multiple: false,
                folders: true,
            },
            title: PICK_TITLE.to_owned(),
            accept_label: Some(PICK_LABEL.to_owned()),
            filters: Vec::new(),
            filter: None,
            folder: self.folder().map(ToOwned::to_owned),
            parent: None,
            types: None,
            // The pin window may be gone by then; nothing waits for it.
            reply: PickReply::new(move |picked| drop(deliver.send(picked))),
        };
        let connector = self.connector.clone();
        // A window opens outside this window's update.
        cx.defer(move |cx| crate::open_picker(&connector, Box::new(request), cx));
        cx.spawn(async move |this, cx| {
            let Ok(PickAnswer::Picked { paths, .. }) = answer.await else {
                return;
            };
            if let Some(path) = paths.into_iter().next() {
                this.update(cx, |this, _| {
                    this.change_pinned(BookmarkChange::Add {
                        path,
                        label: None,
                        at: None,
                    });
                })
                .ok();
            }
        })
        .detach();
    }

    /// Opens the menu of the pinned folder at `path`, at `at`.
    pub(crate) fn open_pinned_menu(
        &mut self,
        path: PathBuf,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let items = vec![MenuItem::new(
            Icon::Close,
            UNPIN_LABEL,
            MenuAction::Unpin(path),
        )];
        self.open_menu(at, items, cx);
    }

    /// Takes the pinned folder at `path` out of Places.
    pub(crate) fn unpin(&self, path: PathBuf) {
        self.change_pinned(BookmarkChange::Remove { path });
    }

    fn change_pinned(&self, change: BookmarkChange) {
        if let Some(connection) = &self.connection {
            // A core gone ends the window's events, which shows it.
            connection.send(Command::ChangeBookmark(change)).ok();
        }
    }
}
