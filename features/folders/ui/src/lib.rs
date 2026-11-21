#![forbid(unsafe_code)]

mod grid;
mod switch;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{App, Context, Pixels, ScrollEvent, Window, px};
use nova_files_folders_api::{Command, Event, FolderView, FolderViews, ViewMode};
use nova_files_places_api::{Known, PlaceKind};
use nova_files_shell::actions::{BiggerIcons, ShowDetails, ShowGallery, ShowIcons, SmallerIcons};
use nova_files_shell::assets::Icon;
use nova_files_shell::{
    Host, Layout, Location, MenuAt, MenuGroup, MenuItem, Part, Placed, Published, Scene, Slots,
    command_spec,
};

pub use grid::icon_side;
use grid::layout_of;

command_spec!(DETAILS, ShowDetails, "view.details", "Details", ["ctrl-1"]);
command_spec!(ICONS, ShowIcons, "view.icons", "Icons", ["ctrl-2"]);
command_spec!(GALLERY, ShowGallery, "view.gallery", "Gallery", ["ctrl-3"]);
command_spec!(
    BIGGER_ICONS,
    BiggerIcons,
    "view.bigger",
    "Bigger icons",
    ["ctrl-=", "ctrl-+"]
);
command_spec!(
    SMALLER_ICONS,
    SmallerIcons,
    "view.smaller",
    "Smaller icons",
    ["ctrl--"]
);

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

    fn here(&self, cx: &App) -> Option<PathBuf> {
        let entries = self.host.entries.read(cx);
        if entries.details_only() {
            return None;
        }
        entries
            .location()
            .and_then(Location::folder)
            .map(Path::to_path_buf)
    }

    pub fn shown(&self, cx: &App) -> FolderView {
        self.here(cx)
            .map_or_else(FolderView::default, |folder| self.view_of(&folder, cx))
    }

    fn layout(&self, folder: &Path, cx: &App) -> Layout {
        let view = self.view_of(folder, cx);
        let picker = self.host.entries.read(cx).picker();
        if picker && view.mode == ViewMode::Gallery {
            return Layout::Details;
        }
        layout_of(view)
    }

    fn show_as(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        let view = self.shown(cx);
        self.change_view(FolderView { mode, ..view }, cx);
    }

    fn details(&mut self, _: &ShowDetails, _: &mut Window, cx: &mut Context<Self>) {
        self.show_as(ViewMode::Details, cx);
    }

    fn icons(&mut self, _: &ShowIcons, _: &mut Window, cx: &mut Context<Self>) {
        self.show_as(ViewMode::Icons, cx);
    }

    fn gallery(&mut self, _: &ShowGallery, _: &mut Window, cx: &mut Context<Self>) {
        self.show_as(ViewMode::Gallery, cx);
    }

    fn bigger(&mut self, _: &BiggerIcons, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom(true, cx);
    }

    fn smaller(&mut self, _: &SmallerIcons, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom(false, cx);
    }

    fn zoom(&mut self, bigger: bool, cx: &mut Context<Self>) {
        let view = self.shown(cx);
        if view.mode != ViewMode::Icons {
            return;
        }
        let icons = if bigger {
            view.icons.bigger()
        } else {
            view.icons.smaller()
        };
        self.change_view(FolderView { icons, ..view }, cx);
    }

    fn wheel(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) -> bool {
        if self.shown(cx).mode != ViewMode::Icons {
            return false;
        }
        self.zoom_scroll += event.delta.pixel_delta(ZOOM_STEP).y;
        let steps = (self.zoom_scroll / ZOOM_STEP).trunc();
        if steps == 0.0 {
            return true;
        }
        self.zoom_scroll -= ZOOM_STEP * steps;
        #[expect(clippy::cast_possible_truncation, reason = "a few steps of a wheel")]
        let steps = steps as isize;
        let view = self.shown(cx);
        let icons = view.icons.stepped(steps);
        self.change_view(FolderView { icons, ..view }, cx);
        true
    }

    fn change_view(&mut self, view: FolderView, cx: &mut Context<Self>) {
        if view == self.shown(cx) {
            return;
        }
        let Some(folder) = self.here(cx) else {
            return;
        };
        self.set_view(folder, view, cx);
        self.host.entries.update(cx, |entries, cx| {
            if entries.cursor().is_some() {
                entries.reveal_cursor(cx);
            }
        });
    }

    fn view_items(&self, scene: &Scene, cx: &App) -> Vec<MenuItem> {
        let in_folder = scene.location.as_ref().and_then(Location::folder).is_some();
        if !in_folder || scene.rules.replaces_menu {
            return Vec::new();
        }
        let showing = self.shown(cx).mode;
        [
            (ViewMode::Details, Icon::List, &DETAILS),
            (ViewMode::Icons, Icon::Tiles, &ICONS),
            (ViewMode::Gallery, Icon::Pictures, &GALLERY),
        ]
        .into_iter()
        .map(|(mode, icon, spec)| {
            if mode == showing {
                MenuItem::run(Icon::Check, spec).with_look(nova_files_shell::Look::On)
            } else {
                MenuItem::run(icon, spec)
            }
        })
        .collect()
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
        let always = |_: &Self, _: &Scene, _: &App| true;
        slots.on_event(Self::arrived);
        slots.sends::<Command>();
        slots.command(&DETAILS, always, Self::details);
        slots.command(&ICONS, always, Self::icons);
        slots.command(&GALLERY, always, Self::gallery);
        slots.command(&BIGGER_ICONS, always, Self::bigger);
        slots.command(&SMALLER_ICONS, always, Self::smaller);
        slots.layout(|part, folder, cx| Some(part.layout(folder, cx)));
        slots.wheel(Self::wheel);
        slots.menu_in(
            MenuAt::Sheet,
            MenuGroup::View,
            Placed::FIRST,
            Self::view_items,
        );
        slots.bar(|part, _, _, cx| Some(part.view_switch(cx)));
    }
}
