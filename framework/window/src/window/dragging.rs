// GPUI fixes a drag's value when the row is drawn, before any drag starts.
// https://github.com/ImperaLuna/zed/tree/6fab5ba6d9562f331e9c581696432f52ec260429/crates/gpui
use std::cell::OnceCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Div, DragPicture, Entity, ExternalDragPayload, FileDragPaths,
    InteractiveElement as _, IntoElement, Modifiers, ParentElement as _, Render, SharedString,
    Styled as _, WeakEntity, Window, div,
};
use nova_files_api::{Entry, EntryId, EntryKind, ListingId};
use nova_files_shell::{DropOver, WindowParts};

use super::MainWindow;
use super::aiming::{Aim, Aims};
use crate::dropping::{self, DropAt, Dropped, Sources};
use crate::island;
use crate::pile::PileDrawing;
use crate::selection::{Selection, entry_at};
use crate::theme::{Colors, radius, size, text};
use nova_files_shell::Location;

#[derive(Debug, Clone)]
pub struct DraggedRow {
    from: DraggedFrom,
    drag: Rc<Drag>,
}

#[derive(Debug, Clone)]
enum DraggedFrom {
    Entry(ListingId, EntryId),
    Pin(PathBuf, SharedString),
}

#[derive(Debug, Default)]
pub struct Drag {
    sources: OnceCell<Sources>,
    outward: OnceCell<FileDragPaths>,
    pub(super) picture: OnceCell<DragPicture>,
    pub(super) drawing: OnceCell<PileDrawing>,
    pub(super) aims: Aims,
}

impl Drag {
    pub fn sources(&self) -> Option<&Sources> {
        self.sources.get()
    }

    pub fn outward(&self) -> Option<ExternalDragPayload> {
        let paths = self.outward.get().cloned()?;
        let paths = match self.picture() {
            Some(picture) => paths.with_picture(picture),
            None => paths,
        };
        Some(ExternalDragPayload::Files(paths))
    }

    fn picture(&self) -> Option<DragPicture> {
        if let Some(picture) = self.picture.get() {
            return Some(picture.clone());
        }
        let picture = self.drawing.get()?.draw()?;
        self.picture.set(picture.clone()).ok();
        Some(picture)
    }

    pub fn says(
        &self,
        modifiers: Modifiers,
        parts: &WindowParts,
        cx: &App,
    ) -> Option<SharedString> {
        let aim = self.aims.aimed()?;
        let Some(at) = dropping::shell_place(&aim.at) else {
            return dropping::label(&aim.at, &aim.name).map(SharedString::from);
        };
        let over = DropOver {
            at: &at,
            name: &aim.name,
            sources: &self.sources()?.paths,
            modifiers,
        };
        if let Some(said) = parts.drop_label(&over, cx) {
            return Some(said);
        }
        dropping::label(&aim.at, &aim.name).map(SharedString::from)
    }

    pub fn aim(&self, aim: Aim) {
        self.aims.aim(aim);
    }
}

impl DraggedRow {
    pub fn new(listing: ListingId, id: EntryId) -> Self {
        Self {
            from: DraggedFrom::Entry(listing, id),
            drag: Rc::default(),
        }
    }

    pub fn pin(path: PathBuf, name: SharedString) -> Self {
        Self {
            from: DraggedFrom::Pin(path, name),
            drag: Rc::default(),
        }
    }

    pub fn sources(&self) -> Option<&Sources> {
        self.drag.sources()
    }

    pub const fn drag(&self) -> &Rc<Drag> {
        &self.drag
    }
}

pub struct DragChip {
    what: SharedString,
    drag: Rc<Drag>,
    origin: WeakEntity<MainWindow>,
}

impl DragChip {
    fn label(&self, modifiers: Modifiers, cx: &App) -> SharedString {
        self.origin
            .upgrade()
            .and_then(|window| self.drag.says(modifiers, &window.read(cx).parts, cx))
            .unwrap_or_else(|| self.what.clone())
    }
}

impl Render for DragChip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = self.label(window.modifiers(), cx);
        chip(label, cx)
    }
}

pub(super) fn chip(label: SharedString, cx: &App) -> Div {
    let colors = Colors::of(cx);
    div()
        .debug_selector(|| "drag-chip".to_owned())
        .px(size::DRAG_CHIP_PAD)
        .py(size::DRAG_CHIP_PAD_Y)
        .rounded(radius::CONTROL)
        .bg(colors.lifted)
        .border(size::LINE)
        .border_color(colors.line)
        .text_size(text::BODY)
        .text_color(colors.text)
        .child(label)
}

impl MainWindow {
    pub(crate) fn cancel_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !cx.stop_active_drag(window) {
            return false;
        }
        self.lift_dropped(cx);
        self.pointer_carried(None, cx);
        cx.notify();
        true
    }

    pub(crate) fn drag_started(
        &mut self,
        row: &DraggedRow,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Entity<DragChip> {
        let (listing, id) = match &row.from {
            DraggedFrom::Entry(listing, id) => (*listing, *id),
            DraggedFrom::Pin(path, name) => {
                row.drag.sources.set(Sources::pin(path.clone())).ok();
                let (drag, origin) = (Rc::clone(&row.drag), cx.entity().downgrade());
                let what = name.clone();
                return cx.new(|_| DragChip { what, drag, origin });
            }
        };
        if !self.selects((listing, id)) {
            self.reselect_in_place(|_, _| Some(Selection::only(listing, id)), cx);
        }
        let selected = self.selected_full_paths();
        let paths: Vec<PathBuf> = selected.iter().map(|(path, _)| path.clone()).collect();
        let what = island::what(&paths).into();
        let selected_kinds = selected.clone();
        if !selected.is_empty() {
            row.drag.outward.set(FileDragPaths::new(selected)).ok();
            self.draw_pile(&row.drag, window, cx);
        }
        row.drag.sources.set(Sources::of_rows(selected_kinds)).ok();
        let (drag, origin) = (Rc::clone(&row.drag), cx.entity().downgrade());
        cx.new(|_| DragChip { what, drag, origin })
    }

    fn selected_full_paths(&self) -> Vec<(PathBuf, bool)> {
        let Some(listing) = &self.listing else {
            return Vec::new();
        };
        self.selected_entries()
            .into_iter()
            .map(|entry| (listing.path_of(entry), entry.kind == EntryKind::Folder))
            .collect()
    }

    pub(super) fn selected_entries(&self) -> Vec<&Entry> {
        let (Some(listing), Some((id, ids))) = (&self.listing, self.selection()) else {
            return Vec::new();
        };
        if id != listing.id {
            return Vec::new();
        }
        ids.into_iter()
            .filter_map(|id| entry_at(&listing.entries, id))
            .collect()
    }

    pub(crate) fn drop_rule(&self) -> impl Fn(&DropAt, &Sources) -> bool + 'static {
        let (picker, rules) = (self.picker(), self.host.rules.clone());
        move |at, sources| !picker && dropping::allowed(at, sources, &rules)
    }

    pub(crate) fn drop_files(
        &self,
        aim: Aim,
        sources: &Sources,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let Aim { at, name } = aim;
        if !self.drop_rule()(&at, sources) {
            return;
        }
        if let DropAt::Gap(gap) = &at {
            (gap.take)(sources.paths.clone(), cx);
            return;
        }
        let Some(place) = dropping::shell_place(&at) else {
            return;
        };
        let over = DropOver {
            at: &place,
            name: &name,
            sources: &sources.paths,
            modifiers,
        };
        if self.parts.drop_at(&over, cx) {
            return;
        }
        if let Some(Dropped::Page { page, paths }) = dropping::dropped(at, sources.paths.clone()) {
            self.parts.drop_on(&page, paths, cx);
        }
    }

    pub(crate) fn tab_drops_at(&self, place: Option<&Location>, name: SharedString) -> Option<Aim> {
        if let Some(Location::Page(page)) = place
            && self.parts.takes_drops(page)
        {
            return Some(Aim::page(page.clone()));
        }
        let folder = place?.folder()?;
        let name = self
            .host
            .rules
            .named(folder)
            .map_or(name, |named| named.label);
        Some(Aim::folder(folder.to_path_buf(), name))
    }
}
