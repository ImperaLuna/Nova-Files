use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::channel::oneshot;
use gpui::{Context, Pixels, Point};
use nova_files_picker_api::{PickAnswer, PickId, PickKind, PickReply, PickRequest};

use super::MainWindow;
use crate::menu::MenuItem;

static NEXT_OWN_PICK: AtomicU64 = AtomicU64::new(u64::MAX);

impl MainWindow {
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

    pub(crate) fn open_row_menu(&mut self, row: &str, at: Point<Pixels>, cx: &mut Context<Self>) {
        let items: Vec<MenuItem> = self
            .parts
            .row_menu(row, cx)
            .into_iter()
            .map(MenuItem::from)
            .collect();
        if !items.is_empty() {
            self.open_menu(at, items, cx);
        }
    }
}
