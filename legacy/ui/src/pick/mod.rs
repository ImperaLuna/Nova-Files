pub use nova_files_picker_ui::{foreign_parent, titlebar};

use std::ffi::OsString;
use std::io::{self, Write as _};
use std::path::PathBuf;

use gpui::{App, Context, Entity, Window};
use nova_files_api::{EntryId, ListingId};
use nova_files_picker_api::{PickId, PickRequest};
use nova_files_picker_ui::PickerPart;

use crate::window::MainWindow;

impl MainWindow {
    pub fn set_pick(
        &mut self,
        request: Box<PickRequest>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_as_picker(request.id);
        if let Some(part) = self.picker_part() {
            part.update(cx, |part, cx| part.start(request, cx));
            if let Some(focus) = part.read(cx).name_focus(cx) {
                window.focus(&focus, cx);
            }
        }
        self.watch_size(window, cx);
    }

    pub(crate) fn picker_part(&self) -> Option<Entity<PickerPart>> {
        self.part::<PickerPart>()
    }

    pub(crate) fn picking(&self, cx: &App) -> Option<Entity<PickerPart>> {
        self.picker_part()
            .filter(|part| part.read(cx).id().is_some())
    }

    pub fn pick_id(&self, cx: &App) -> Option<PickId> {
        self.picker_part()?.read(cx).id()
    }

    pub fn listing_id(&self) -> Option<ListingId> {
        self.listing.as_ref().map(|listing| listing.id)
    }

    pub fn pick_name(&self, cx: &App) -> Option<String> {
        self.picker_part()?.read(cx).name(cx)
    }

    pub fn pick_conflict(&self, cx: &App) -> Option<OsString> {
        let part = self.picker_part()?;
        let conflict = part.read(cx).conflict()?;
        Some(conflict.keep_both.clone())
    }

    pub fn pick_filter(&self, cx: &App) -> Option<usize> {
        self.picker_part()?.read(cx).filter()
    }

    pub fn pick_entry(&mut self, listing: ListingId, id: EntryId, cx: &mut Context<Self>) -> bool {
        self.publish(cx);
        let Some(part) = self.picking(cx) else {
            return false;
        };
        let picked = part.update(cx, |part, cx| part.pick_entry(listing, id, cx));
        self.take_nav_asks(cx);
        picked
    }

    pub fn accept(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.publish(cx);
        if let Some(part) = self.picking(cx) {
            part.update(cx, PickerPart::accept);
            self.take_nav_asks(cx);
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(part) = self.picking(cx) {
            part.update(cx, PickerPart::cancel);
        }
    }

    pub fn answer(&mut self, path: PathBuf, cx: &mut Context<Self>) -> bool {
        self.picking(cx)
            .is_some_and(|part| part.update(cx, |part, cx| part.answer(path, cx)))
    }
}

pub fn close(id: PickId, cx: &mut App) {
    for handle in cx.windows() {
        let Some(handle) = handle.downcast::<MainWindow>() else {
            continue;
        };
        let closed = handle.update(cx, |view, window, cx| {
            if view.pick_id(cx) == Some(id) {
                view.cancel(cx);
                window.remove_window();
            }
        });
        if let Err(error) = closed {
            writeln!(
                io::stderr(),
                "nova-files: could not close a picker: {error:#}"
            )
            .ok();
        }
    }
}
