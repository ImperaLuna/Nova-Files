mod opening;

pub use nova_files_picker_ui::titlebar;

use std::ffi::OsString;
use std::io::{self, Write as _};
use std::path::PathBuf;

use gpui::{App, ClickEvent, Context, Entity, Window};
use nova_files_api::{EntryId, ListingId};
use nova_files_picker_api::{PickId, PickRequest};
use nova_files_picker_ui::{Clicked, PickerPart};
use nova_files_properties_api::PropertiesId;

pub use self::opening::foreign_parent;
use crate::selecting::DOUBLE_CLICK;
use crate::selection::Click;
use crate::window::MainWindow;

impl MainWindow {
    pub fn set_pick(
        &mut self,
        request: Box<PickRequest>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_as_picker(request.id);
        let Some(part) = self.picker_part() else {
            return;
        };
        part.update(cx, |part, cx| part.start(request, cx));
        if let Some(focus) = part.read(cx).name_focus(cx) {
            window.focus(&focus, cx);
        }
        self.watch_pick_size(window, cx);
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

    pub fn entry_clicked(
        &mut self,
        (listing, id): (ListingId, EntryId),
        event: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let double = event.click_count() >= DOUBLE_CLICK;
        let Some(kind) = self.entry(listing, id).map(|entry| entry.kind) else {
            return;
        };
        let Some(part) = self.picking(cx) else {
            self.main_clicked((listing, id), event, cx);
            return;
        };
        let several = part.read(cx).several();
        let click = Click::of(event.modifiers(), several);
        match part.read(cx).clicked(kind, click == Click::Only, double) {
            Clicked::Open => self.open_entry(id),
            Clicked::Select => self.reselect(
                |before, picks| {
                    Some(crate::selection::Selection::clicked(
                        before, listing, id, click, picks,
                    ))
                },
                cx,
            ),
            Clicked::Pick => {
                self.pick_entry(listing, id, cx);
            }
            Clicked::Nothing => {}
        }
        cx.notify();
    }

    fn entry(&self, listing: ListingId, id: EntryId) -> Option<&nova_files_api::Entry> {
        let shown = self.listing.as_ref().filter(|shown| shown.id == listing)?;
        usize::try_from(id.0)
            .ok()
            .and_then(|index| shown.entries.get(index))
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

    pub(crate) fn pick_selection_changed(&self, cx: &mut Context<Self>) {
        if let Some(part) = self.picking(cx) {
            part.update(cx, PickerPart::selection_changed);
        }
    }

    pub(crate) fn save_probed(&self, id: PropertiesId, missing: bool, cx: &mut Context<Self>) {
        if let Some(part) = self.picking(cx) {
            part.update(cx, |part, cx| part.probed(id, missing, cx));
        }
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
