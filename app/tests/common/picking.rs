use std::ffi::OsString;
use std::path::PathBuf;

use gpui::{App, Context, Entity, Window};
use nova_files_api::{EntryId, ListingId};
use nova_files_picker_api::{PickId, PickRequest};
use nova_files_picker_ui::{PickOpening, PickerPart};
use nova_files_window::MainWindow;

#[expect(dead_code, reason = "each test binary uses only some of these")]
pub trait Picking {
    fn set_pick(
        &mut self,
        request: Box<PickRequest>,
        window: &mut Window,
        cx: &mut Context<MainWindow>,
    );
    fn pick_id(&self, cx: &App) -> Option<PickId>;
    fn pick_name(&self, cx: &App) -> Option<String>;
    fn pick_conflict(&self, cx: &App) -> Option<OsString>;
    fn pick_filter(&self, cx: &App) -> Option<usize>;
    fn pick_entry(&mut self, listing: ListingId, id: EntryId, cx: &mut Context<MainWindow>)
    -> bool;
    fn accept(&mut self, window: &mut Window, cx: &mut Context<MainWindow>);
    fn answer(&mut self, path: PathBuf, cx: &mut Context<MainWindow>) -> bool;
}

fn picking(view: &MainWindow, cx: &App) -> Option<Entity<PickerPart>> {
    view.part::<PickerPart>()
        .filter(|part| part.read(cx).id().is_some())
}

impl Picking for MainWindow {
    fn set_pick(&mut self, request: Box<PickRequest>, window: &mut Window, cx: &mut Context<Self>) {
        self.open_as(PickOpening::of(request), window, cx);
    }

    fn pick_id(&self, cx: &App) -> Option<PickId> {
        self.part::<PickerPart>()?.read(cx).id()
    }

    fn pick_name(&self, cx: &App) -> Option<String> {
        self.part::<PickerPart>()?.read(cx).name(cx)
    }

    fn pick_conflict(&self, cx: &App) -> Option<OsString> {
        let part = self.part::<PickerPart>()?;
        let conflict = part.read(cx).conflict()?;
        Some(conflict.keep_both.clone())
    }

    fn pick_filter(&self, cx: &App) -> Option<usize> {
        self.part::<PickerPart>()?.read(cx).filter()
    }

    fn pick_entry(&mut self, listing: ListingId, id: EntryId, cx: &mut Context<Self>) -> bool {
        self.settle(cx, |view, cx| {
            picking(view, cx)
                .is_some_and(|part| part.update(cx, |part, cx| part.pick_entry(listing, id, cx)))
        })
    }

    fn accept(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.settle(cx, |view, cx| {
            if let Some(part) = picking(view, cx) {
                part.update(cx, PickerPart::accept);
            }
        });
    }

    fn answer(&mut self, path: PathBuf, cx: &mut Context<Self>) -> bool {
        picking(self, cx).is_some_and(|part| part.update(cx, |part, cx| part.answer(path, cx)))
    }
}
