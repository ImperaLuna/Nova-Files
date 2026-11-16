//! Dragging files out of the list and dropping files on the window (the
//! rules are in `dropping`). A press on an entry's name that moves drags
//! the selection; a name not selected is selected alone first. Under the
//! pointer, a chip says what dropping them on the target below would do
//! (see `aiming`), and away from targets what is dragged: the name of one
//! entry, or how many. Files dragged in from another app drop by the same rules, and so
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
    ParentElement as _, Render, SharedString, Styled as _, WeakEntity, Window, div,
};
use nova_files_api::{EntryId, ListingId};

use super::MainWindow;
use super::aiming::{Aim, Aims};
use crate::dropping::{self, DropAt, Dropped, Sources};
use crate::island;
use crate::location::Location;
use crate::selection::{Selection, entry_at};
use crate::theme::{Colors, radius, size, text};

/// What a drag out of the list carries: the row whose name was pressed,
/// and the drag itself once it starts.
#[derive(Debug, Clone)]
pub struct DraggedRow {
    listing: ListingId,
    id: EntryId,
    drag: Rc<Drag>,
}

/// A drag of files: what it carries and where it is aimed.
#[derive(Debug, Default)]
pub struct Drag {
    sources: OnceCell<Sources>,
    pub(super) aims: Aims,
}

impl Drag {
    /// The files dragged; none before the drag starts.
    pub fn sources(&self) -> Option<&Sources> {
        self.sources.get()
    }

    /// What dropping the files on the target they are aimed at does with
    /// `modifiers` held, once known (see `dropping::label`).
    pub fn says(&self, modifiers: Modifiers) -> Option<String> {
        let aim = self.aims.aimed()?;
        let by_drive = match &aim.at {
            DropAt::Folder(into) => self.aims.by_drive(into),
            DropAt::Trash => None,
        };
        dropping::label(&aim.at, &aim.name, by_drive, modifiers)
    }

    /// Aims the drag at `aim`, the chip names it, unless a target nearer
    /// the pointer did on this move (see `aiming`).
    pub fn aim(&self, aim: Aim) {
        self.aims.aim(aim);
    }
}

impl DraggedRow {
    /// The row of entry `id` in `listing`, not dragged yet.
    pub fn new(listing: ListingId, id: EntryId) -> Self {
        Self {
            listing,
            id,
            drag: Rc::default(),
        }
    }

    /// The files dragged; none before the drag starts.
    pub fn sources(&self) -> Option<&Sources> {
        self.drag.sources()
    }

    /// The drag, once it starts.
    pub const fn drag(&self) -> &Rc<Drag> {
        &self.drag
    }
}

/// The chip under the pointer while files are dragged.
pub struct DragChip {
    /// What is dragged: a name, or how many.
    what: SharedString,
    drag: Rc<Drag>,
    /// The window the drag started in, which asks the core.
    origin: WeakEntity<MainWindow>,
}

impl DragChip {
    /// What the chip says this frame: the drop on the target under the
    /// pointer, or what is dragged. The first time a folder is named,
    /// the window the drag came from asks the core what a drop there
    /// does (see `aiming`).
    fn label(&self, modifiers: Modifiers, cx: &mut Context<Self>) -> SharedString {
        if let Some(DropAt::Folder(into)) = self.drag.aims.aimed().map(|aim| aim.at)
            && self.drag.aims.first_ask(&into)
        {
            let (origin, drag) = (self.origin.clone(), Rc::clone(&self.drag));
            // Not while the window draws.
            cx.defer(move |cx| {
                // A window closed asks nothing.
                origin
                    .update(cx, |window, _| window.ask_drop_kind(&drag, into))
                    .ok();
            });
        }
        self.drag
            .says(modifiers)
            .map_or_else(|| self.what.clone(), SharedString::from)
    }
}

impl Render for DragChip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        let label = self.label(window.modifiers(), cx);
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
        let what = island::what(&paths).into();
        row.drag.sources.set(Sources::new(paths)).ok();
        let (drag, origin) = (Rc::clone(&row.drag), cx.entity().downgrade());
        cx.new(|_| DragChip { what, drag, origin })
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
    pub(crate) fn trash_folder(&self) -> &Path {
        &self.trash.folder
    }

    /// Where files dropped on a tab at `place`, named `name`, go: its
    /// folder, or the trash for the trash's tab; nowhere for a Start page.
    pub(crate) fn tab_drops_at(&self, place: Option<&Location>, name: SharedString) -> Option<Aim> {
        let folder = place?.folder()?;
        Some(if folder == self.trash_folder() {
            Aim::trash()
        } else {
            Aim::folder(folder.to_path_buf(), name)
        })
    }
}
