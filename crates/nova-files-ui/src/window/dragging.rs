//! Dragging files out of the list and dropping files on the window (the
//! rules are in `dropping`). A press on an entry's name that moves drags
//! the selection; a name not selected is selected alone first. Under the
//! pointer, a chip says what is dragged: the name of one entry, or how
//! many. Files dragged in from another app drop by the same rules, and so
//! do files dragged from another of this app's windows.
//!
//! GPUI fixes a drag's value when the row is drawn, before anyone knows
//! whether it will be dragged, so the value holds an empty slot the drag
//! fills with the files once it starts. What is dragged thus travels with
//! the drag and goes with it, whichever window it ends in. Nothing is
//! dragged out of a picker or the trash.

use std::cell::OnceCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, Modifiers,
    ParentElement as _, Render, SharedString, Styled as _, Window, div,
};
use nova_files_api::{EntryId, ListingId};

use super::MainWindow;
use crate::dropping::{self, DropAt, Dropped, Sources};
use crate::island;
use crate::location::Location;
use crate::selection::{Selection, entry_at};
use crate::theme::{Colors, radius, size, text};

/// What a drag out of the list carries: the row whose name was pressed,
/// and the files dragged once the drag starts.
#[derive(Debug, Clone)]
pub struct DraggedRow {
    listing: ListingId,
    id: EntryId,
    sources: Rc<OnceCell<Sources>>,
}

impl DraggedRow {
    /// The row of entry `id` in `listing`, not dragged yet.
    pub fn new(listing: ListingId, id: EntryId) -> Self {
        Self {
            listing,
            id,
            sources: Rc::default(),
        }
    }

    /// The files dragged; none before the drag starts.
    pub fn sources(&self) -> Option<&Sources> {
        self.sources.get()
    }
}

/// The chip under the pointer while files are dragged.
pub struct DragChip {
    label: SharedString,
}

impl Render for DragChip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(self.label.clone())
    }
}

impl MainWindow {
    /// A drag starts on `row`'s name (only outside a picker and the
    /// trash, see `view::sheet`): selects it alone unless it is selected,
    /// gives the drag what is selected, and gives the chip to draw under
    /// the pointer.
    pub(crate) fn drag_started(
        &mut self,
        row: &DraggedRow,
        cx: &mut Context<Self>,
    ) -> Entity<DragChip> {
        if !self.selects((row.listing, row.id)) {
            self.reselect(|_, _| Some(Selection::only(row.listing, row.id)), cx);
        }
        let paths = self.selected_full_paths();
        let label = island::what(&paths).into();
        row.sources.set(Sources::new(paths)).ok();
        cx.new(|_| DragChip { label })
    }

    /// The paths of the entries selected, each in its own folder, which
    /// for a search's files isn't the one searched.
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

    /// Whether this window takes `sources` dropped at `at`: never in a
    /// picker; elsewhere as `dropping` says.
    pub(crate) fn drop_rule(&self) -> impl Fn(&DropAt, &Sources) -> bool + 'static {
        let (picker, trash) = (self.pick.is_some(), self.trash_folder().to_path_buf());
        move |at, sources| !picker && dropping::allowed(at, sources, &trash)
    }

    /// Drops `sources` at `at` with `modifiers` held, when this window
    /// takes them there.
    pub(crate) fn drop_files(&mut self, at: DropAt, sources: &Sources, modifiers: Modifiers) {
        if !self.drop_rule()(&at, sources) {
            return;
        }
        match dropping::dropped(at, sources.paths.clone(), modifiers) {
            Dropped::Job(operation) => {
                self.start_job(operation);
            }
            Dropped::ByDrive { sources, into } => {
                self.start_drop(sources, into);
            }
        }
    }

    /// The folder of the trash's entries, where nothing drops.
    fn trash_folder(&self) -> &Path {
        &self.trash.folder
    }

    /// Where files dropped on a tab at `place` go: its folder, or the
    /// trash for the trash's tab; nowhere for a Start page.
    pub(crate) fn tab_drops_at(&self, place: Option<&Location>) -> Option<DropAt> {
        let folder = place?.folder()?;
        Some(if folder == self.trash_folder() {
            DropAt::Trash
        } else {
            DropAt::Folder(folder.to_path_buf())
        })
    }
}
