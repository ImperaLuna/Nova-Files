use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::channel::oneshot;
use gpui::{Context, Pixels, Point};
use nova_files_picker_api::{PickAnswer, PickId, PickKind, PickReply, PickRequest};
use nova_files_places_api::{BookmarkChange, Command};

use super::MainWindow;
use crate::assets::Icon;
use crate::menu::{MenuAction, MenuItem};

const PICK_TITLE: &str = "Pin a folder";
const PICK_LABEL: &str = "Pin to Places";
const UNPIN_LABEL: &str = "Remove from Places";

static NEXT_OWN_PICK: AtomicU64 = AtomicU64::new(u64::MAX);

impl MainWindow {
    pub fn pick_folder_to_pin(&mut self, cx: &mut Context<Self>) {
        self.pick_folder(PICK_TITLE, PICK_LABEL, cx, |this, path, _| {
            this.change_pinned(BookmarkChange::Add {
                path,
                label: None,
                at: None,
            });
        });
    }

    pub(super) fn pick_folder(
        &self,
        title: &str,
        accept: &str,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, PathBuf, &mut Context<Self>) + 'static,
    ) {
        let (deliver, answer) = oneshot::channel();
        let request = PickRequest {
            id: PickId(NEXT_OWN_PICK.fetch_sub(1, Ordering::Relaxed)),
            kind: PickKind::Open {
                multiple: false,
                folders: true,
            },
            title: title.to_owned(),
            accept_label: Some(accept.to_owned()),
            filters: Vec::new(),
            filter: None,
            folder: self.folder().map(ToOwned::to_owned),
            parent: None,
            types: None,
            reply: PickReply::new(move |picked| drop(deliver.send(picked))),
        };
        let connector = self.connector.clone();
        cx.defer(move |cx| crate::open_picker(&connector, Box::new(request), cx));
        cx.spawn(async move |this, cx| {
            let Ok(PickAnswer::Picked { paths, .. }) = answer.await else {
                return;
            };
            if let Some(path) = paths.into_iter().next() {
                this.update(cx, |this, cx| then(this, path, cx)).ok();
            }
        })
        .detach();
    }

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

    pub(crate) fn unpin(&self, path: PathBuf) {
        self.change_pinned(BookmarkChange::Remove { path });
    }

    fn change_pinned(&self, change: BookmarkChange) {
        if let Some(connection) = &self.connection {
            connection.send(Command::ChangeBookmark(change)).ok();
        }
    }
}
