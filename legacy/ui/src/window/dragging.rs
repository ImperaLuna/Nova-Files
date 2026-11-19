// GPUI fixes a drag's value when the row is drawn, before any drag starts.
// https://github.com/ImperaLuna/zed/tree/nova/v0.233.10/crates/gpui
use std::cell::OnceCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Div, Entity, InteractiveElement as _, IntoElement, Modifiers,
    ParentElement as _, Render, SharedString, Styled as _, WeakEntity, Window, div,
};
use nova_files_api::{EntryId, ListingId};
use nova_files_shell::{DropOver, WindowParts};

use super::MainWindow;
use super::aiming::{Aim, Aims};
use crate::dropping::{self, DropAt, Dropped, Sources};
use crate::island;
use crate::location::Location;
use crate::selection::{Selection, entry_at};
use crate::theme::{Colors, radius, size, text};

#[derive(Debug, Clone)]
pub struct DraggedRow {
    listing: ListingId,
    id: EntryId,
    drag: Rc<Drag>,
}

#[derive(Debug, Default)]
pub struct Drag {
    sources: OnceCell<Sources>,
    pub(super) aims: Aims,
}

impl Drag {
    pub fn sources(&self) -> Option<&Sources> {
        self.sources.get()
    }

    pub fn says(
        &self,
        modifiers: Modifiers,
        parts: &WindowParts,
        cx: &App,
    ) -> Option<SharedString> {
        let aim = self.aims.aimed()?;
        if let Some(at) = dropping::shell_place(&aim.at) {
            let over = DropOver {
                at: &at,
                name: &aim.name,
                sources: &self.sources()?.paths,
                modifiers,
            };
            if let Some(said) = parts.drop_label(&over, cx) {
                return Some(said);
            }
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
            listing,
            id,
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
    pub(crate) fn drag_started(
        &mut self,
        row: &DraggedRow,
        cx: &mut Context<Self>,
    ) -> Entity<DragChip> {
        if !self.selects((row.listing, row.id)) {
            self.reselect(|_, _| Some(Selection::only(row.listing, row.id)), cx);
        }
        let paths = self.selected_full_paths();
        let what = island::what(&paths).into();
        row.drag.sources.set(Sources::new(paths)).ok();
        let (drag, origin) = (Rc::clone(&row.drag), cx.entity().downgrade());
        cx.new(|_| DragChip { what, drag, origin })
    }

    fn selected_full_paths(&self) -> Vec<PathBuf> {
        let (Some(listing), Some((id, ids))) = (&self.listing, self.selection()) else {
            return Vec::new();
        };
        if id != listing.id {
            return Vec::new();
        }
        ids.into_iter()
            .filter_map(|id| entry_at(&listing.entries, id))
            .map(|entry| listing.path_of(entry))
            .collect()
    }

    pub(crate) fn drop_rule(&self) -> impl Fn(&DropAt, &Sources) -> bool + 'static {
        let (picker, rules) = (self.pick.is_some(), self.host.rules.clone());
        move |at, sources| {
            !picker && dropping::allowed(at, sources, |folder| rules.for_folder(folder).trashed)
        }
    }

    pub(crate) fn drop_files(
        &mut self,
        aim: Aim,
        sources: &Sources,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let Aim { at, name } = aim;
        if !self.drop_rule()(&at, sources) {
            return;
        }
        if let Some(place) = dropping::shell_place(&at) {
            let over = DropOver {
                at: &place,
                name: &name,
                sources: &sources.paths,
                modifiers,
            };
            if self.parts.drop_at(&over, cx) {
                return;
            }
        }
        match dropping::dropped(at, sources.paths.clone()) {
            None => {}
            Some(Dropped::Job(operation)) => {
                self.start_job(operation);
            }
            Some(Dropped::Gather { place, folders }) => self.gather(place, folders, cx),
            Some(Dropped::Page { page, paths }) => self.parts.drop_on(&page, paths, cx),
        }
    }

    pub(crate) fn tab_drops_at(&self, place: Option<&Location>, name: SharedString) -> Option<Aim> {
        if let Some(Location::Page(page)) = place
            && self.parts.takes_drops(page)
        {
            return Some(Aim::page(page.clone()));
        }
        let folder = place?.folder()?;
        Some(match self.host.rules.named(folder) {
            Some(named) if self.host.rules.for_folder(folder).trashed => Aim::trash(named.label),
            _ => Aim::folder(folder.to_path_buf(), name),
        })
    }
}
