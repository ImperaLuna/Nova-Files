#![forbid(unsafe_code)]

mod gallery;
mod grid;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{App, Context, Pixels, ScrollDelta, px};
use nova_files_folders_api::{Command, Event, FolderView, FolderViews, ViewMode};
use nova_files_places_api::{Known, PlaceKind};
use nova_files_shell::{Host, Part, Published, Slots};

pub use gallery::{GalleryBounds, stage_room, strip};
pub use grid::{Grid, NAME_LINES, icon_side};

const ZOOM_STEP: Pixels = px(40.0);

const PICTURE_PLACES: [PlaceKind; 2] = [PlaceKind::Pictures, PlaceKind::Videos];

pub struct FoldersPart {
    host: Host,
    views: HashMap<PathBuf, FolderView>,
    pending: HashMap<PathBuf, FolderView>,
    zoom_scroll: Pixels,
}

impl FoldersPart {
    pub fn view_of(&self, folder: &Path, cx: &App) -> FolderView {
        if let Some(view) = self.views.get(folder) {
            return *view;
        }
        let pictures = Published::<Known>::get(cx).is_some_and(|known| {
            known.places.iter().any(|place| {
                PICTURE_PLACES.contains(&place.kind) && folder.starts_with(&place.path)
            })
        });
        let mode = if pictures {
            ViewMode::Icons
        } else {
            ViewMode::Details
        };
        FolderView {
            mode,
            ..FolderView::default()
        }
    }

    pub fn set_view(&mut self, folder: PathBuf, view: FolderView, cx: &mut Context<Self>) {
        self.views.insert(folder.clone(), view);
        self.pending.insert(folder.clone(), view);
        self.host.link.send(Command::SetView { folder, view }).ok();
        cx.notify();
    }

    pub fn zoom_steps(&mut self, delta: ScrollDelta) -> isize {
        self.zoom_scroll += delta.pixel_delta(ZOOM_STEP).y;
        let steps = (self.zoom_scroll / ZOOM_STEP).trunc();
        if steps == 0.0 {
            return 0;
        }
        self.zoom_scroll -= ZOOM_STEP * steps;
        #[expect(clippy::cast_possible_truncation, reason = "a few steps of a wheel")]
        let steps = steps as isize;
        steps
    }

    fn arrived(&mut self, event: Event, cx: &mut Context<Self>) {
        let Event::Views(views) = event;
        self.views_arrived(&views);
        cx.notify();
    }

    fn views_arrived(&mut self, views: &FolderViews) {
        let mut known = views.iter().rev().cloned().collect::<HashMap<PathBuf, _>>();
        self.pending
            .retain(|folder, view| known.get(folder) != Some(view));
        known.extend(
            self.pending
                .iter()
                .map(|(folder, view)| (folder.clone(), *view)),
        );
        self.views = known;
    }
}

impl Part for FoldersPart {
    fn open(host: &Host, _: &mut Context<Self>) -> Self {
        host.link.send(Command::Views).ok();
        Self {
            host: host.clone(),
            views: HashMap::new(),
            pending: HashMap::new(),
            zoom_scroll: Pixels::ZERO,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(Self::arrived);
        slots.sends::<Command>();
    }
}
