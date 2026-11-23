#![forbid(unsafe_code)]

mod draw;
mod looks;

use std::mem;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{AnyElement, App, Context, Pixels, RenderImage, Size, Subscription, Window};
use nova_files_api::{Entry, EntryId, Fit, Listing, ListingId};
use nova_files_preview_api::{Command, Event, Preview, PreviewContent, PreviewId};
use nova_files_shell::actions::{
    ClearSelection, CursorDown, CursorUp, DeleteSelected, NewFolder, OpenSelected, PreviewNext,
    PreviewPrevious, RenameSelected, TogglePreview, TrashSelected, Undo,
};
use nova_files_shell::device::device_side;
use nova_files_shell::theme::Colors;
use nova_files_shell::{Entries, Host, Move, NavEvent, Part, Scene, Showing, Slots, Staged};

use draw::{meta, room};
pub use looks::Previewed;
use looks::{DEFAULT_FIT, QuickLook, QuickLooks, Shown, moved, render_image, without_pixels};

pub struct PreviewPart {
    host: Host,
    looks: QuickLooks,
    seen: Option<Listing>,
    cursor: Option<usize>,
    _watching: Vec<Subscription>,
}

impl PreviewPart {
    pub const fn previewing(&self) -> bool {
        self.looks.open.is_some() && !self.looks.docked
    }

    pub const fn preview_fit(&self) -> Option<Fit> {
        self.looks.fit
    }

    pub fn previewed(&self, cx: &App) -> Option<Previewed> {
        let open = self.looks.open.as_ref()?;
        let listing = self.host.entries.read(cx).listing()?;
        let entry = listing.entries.get(usize::try_from(open.entry.0).ok()?)?;
        let current = open.current();
        Some(Previewed {
            name: entry.name.to_string_lossy().into_owned(),
            preview: current.and_then(|shown| shown.preview.clone()),
            picture: current.is_some_and(|shown| shown.picture.is_some()),
        })
    }

    fn stage(
        &mut self,
        _: &Scene,
        room: Size<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Staged> {
        let scale_factor = window.scale_factor();
        if !self.previewing() {
            self.set_room(room, scale_factor, cx);
        }
        let cursor = self.host.entries.read(cx).cursor();
        if let Some(cursor) = cursor {
            self.dock(cursor, cx);
        }
        let open = self.looks.open.as_ref()?;
        let listing = self.host.entries.read(cx).listing()?;
        let entry = entry_at(listing, open.entry)?;
        let about = open
            .current()
            .and_then(|shown| shown.preview.as_deref())
            .map(meta)
            .unwrap_or_default();
        Some(Staged {
            entry: open.entry,
            art: draw::stage_art(open, listing, entry, (room, scale_factor), &Colors::of(cx)),
            about: about.into(),
        })
    }

    fn layer(
        &mut self,
        scene: &Scene,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let viewport = window.viewport_size();
        let room = room(viewport.width, viewport.height);
        let scale_factor = window.scale_factor();
        if scene.showing != Showing::Gallery || self.previewing() {
            self.set_room(room, scale_factor, cx);
        }
        let open = self.looks.open.as_ref().filter(|_| self.previewing())?;
        let listing = self
            .host
            .entries
            .read(cx)
            .listing()
            .filter(|listing| listing.id == open.listing)?;
        let index = usize::try_from(open.entry.0).ok()?;
        let entry = listing.entries.get(index)?;
        Some(draw::layer(
            (open, listing, entry, index),
            (room, scale_factor),
            cx,
        ))
    }

    fn dock(&mut self, cursor: usize, cx: &mut Context<Self>) {
        if self.looks.open.is_some() {
            return;
        }
        let Some(listing) = self
            .host
            .entries
            .read(cx)
            .listing()
            .map(|listing| listing.id)
        else {
            return;
        };
        let Ok(cursor) = u32::try_from(cursor) else {
            return;
        };
        self.show_entry(listing, EntryId(cursor), None, cx);
        self.looks.docked = true;
    }

    fn undock(&mut self, cx: &mut Context<Self>) {
        if self.looks.docked {
            self.close(cx);
        }
    }

    fn set_room(&mut self, room: Size<Pixels>, scale_factor: f32, cx: &mut Context<Self>) {
        let device = |side: Pixels| device_side(side, scale_factor);
        let fit = Some(Fit {
            width: device(room.width),
            height: device(room.height),
        });
        if self.looks.fit == fit {
            return;
        }
        self.looks.fit = fit;
        if let Some(open) = self.looks.open.take() {
            self.show_entry(open.listing, open.entry, open.shown, cx);
        }
    }

    fn toggle(&mut self, _: &TogglePreview, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let now = cx.background_executor().now();
        let entries = self.host.entries.read(cx);
        if !self.host.list.is_focused(window) || (!entries.picker() && entries.jumping(now)) {
            return false;
        }
        if self.looks.open.is_some() {
            if self.looks.docked {
                self.looks.docked = false;
                cx.notify();
            } else {
                self.dismiss(cx);
            }
            return true;
        }
        if let Some((listing, entry)) = to_look_at(entries) {
            self.show_entry(listing, entry, None, cx);
        }
        true
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.host.entries.read(cx).showing() == Showing::Gallery {
            self.looks.docked = true;
            cx.notify();
        } else {
            self.close(cx);
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.looks.docked = false;
        if let Some(open) = self.looks.open.take() {
            release(open.shown.and_then(|shown| shown.picture), cx);
            cx.notify();
        }
    }

    fn step(&self, step: Move, cx: &mut Context<Self>) -> bool {
        let Some(open) = &self.looks.open else {
            return false;
        };
        let shown = usize::try_from(open.entry.0).ok();
        self.host
            .entries
            .update(cx, |entries, cx| entries.move_cursor(shown, step, cx));
        true
    }

    fn previous(&mut self, _: &PreviewPrevious, _: &mut Window, cx: &mut Context<Self>) -> bool {
        self.step(Move::Left, cx)
    }

    fn next(&mut self, _: &PreviewNext, _: &mut Window, cx: &mut Context<Self>) -> bool {
        self.step(Move::Right, cx)
    }

    fn up(&mut self, _: &CursorUp, _: &mut Window, cx: &mut Context<Self>) -> bool {
        self.step(Move::Up, cx)
    }

    fn down(&mut self, _: &CursorDown, _: &mut Window, cx: &mut Context<Self>) -> bool {
        self.step(Move::Down, cx)
    }

    fn enter(&mut self, _: &OpenSelected, _: &mut Window, cx: &mut Context<Self>) -> bool {
        self.open_previewed(cx)
    }

    fn escape(&mut self, _: &ClearSelection, _: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.previewing() {
            return false;
        }
        self.dismiss(cx);
        true
    }

    const fn holds<A>(&mut self, _: &A, _: &mut Window, _: &mut Context<Self>) -> bool {
        self.previewing()
    }

    fn open_previewed(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.previewing() {
            return false;
        }
        let Some(entry) = self.looks.open.as_ref().map(|open| open.entry) else {
            return false;
        };
        self.close(cx);
        self.host
            .entries
            .update(cx, |entries, cx| entries.open(entry, cx));
        true
    }

    fn entries_changed(&mut self, cx: &mut Context<Self>) {
        if self.host.entries.read(cx).showing() != Showing::Gallery {
            self.undock(cx);
        }
        let entries = self.host.entries.read(cx);
        let cursor = entries.cursor();
        let listing = entries.listing();
        if listing.map(|listing| listing.id) != self.seen.as_ref().map(|seen| seen.id) {
            let listing = listing.cloned();
            let before = mem::replace(&mut self.seen, listing);
            self.cursor = cursor;
            self.listing_arrived(before.as_ref(), cx);
            return;
        }
        if cursor != self.cursor {
            self.cursor = cursor;
            self.follow_cursor(cx);
        }
    }

    fn follow_cursor(&mut self, cx: &mut Context<Self>) {
        let Some(open) = &self.looks.open else {
            return;
        };
        let listing = open.listing;
        let Some(entry) = self
            .cursor
            .and_then(|cursor| u32::try_from(cursor).ok())
            .map(EntryId)
            .filter(|&entry| entry != open.entry)
        else {
            return;
        };
        let shown = self.looks.open.take().and_then(|open| open.shown);
        self.show_entry(listing, entry, shown, cx);
    }

    fn listing_arrived(&mut self, before: Option<&Listing>, cx: &mut Context<Self>) {
        let Some(open) = &self.looks.open else {
            return;
        };
        let name = before
            .filter(|before| before.id == open.listing)
            .and_then(|before| entry_at(before, open.entry))
            .map(|entry| entry.name.clone());
        let found = self.seen.as_ref().and_then(|listing| {
            let name = name.as_deref()?;
            let same_folder = before.is_some_and(|before| before.path == listing.path);
            let index = listing
                .entries
                .iter()
                .position(|entry| same_folder && entry.name == name)?;
            Some((listing.id, EntryId(u32::try_from(index).ok()?)))
        });
        match (found, self.looks.open.take()) {
            (Some((listing, entry)), Some(open)) => {
                let shown = open.shown.map(|shown| moved(shown, open.entry, entry));
                self.show_entry(listing, entry, shown, cx);
            }
            (_, open) => {
                self.looks.open = open;
                self.close(cx);
            }
        }
    }

    fn show_entry(
        &mut self,
        listing: ListingId,
        entry: EntryId,
        shown: Option<Shown>,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.entry_path(listing, entry, cx) else {
            release(shown.and_then(|shown| shown.picture), cx);
            cx.notify();
            return;
        };
        let asked = self.looks.next_id();
        self.looks.open = Some(QuickLook {
            listing,
            entry,
            asked,
            shown,
        });
        let fit = self.looks.fit.unwrap_or(DEFAULT_FIT);
        self.host
            .link
            .send(Command::Preview {
                id: asked,
                path,
                fit,
            })
            .ok();
        cx.notify();
    }

    fn entry_path(&self, listing: ListingId, entry: EntryId, cx: &App) -> Option<PathBuf> {
        let entries = self.host.entries.read(cx);
        let on_screen = entries.listing().filter(|shown| shown.id == listing)?;
        let entry = entry_at(on_screen, entry)?;
        Some(on_screen.path_of(entry))
    }

    fn arrived(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Previewed { id, preview } => self.preview_arrived(id, Some(preview), cx),
            Event::Unreadable { id, .. } => self.preview_arrived(id, None, cx),
        }
    }

    fn preview_arrived(
        &mut self,
        id: PreviewId,
        preview: Option<Arc<Preview>>,
        cx: &mut Context<Self>,
    ) {
        if self.looks.open.as_ref().map(|open| open.asked) != Some(id) {
            return;
        }
        let Some(PreviewContent::Picture(picture)) =
            preview.as_deref().map(|preview| &preview.content)
        else {
            self.show_preview(id, preview, None, cx);
            return;
        };
        let picture = picture.clone();
        let preview = preview.map(|preview| Arc::new(without_pixels(&preview)));
        let converting = cx
            .background_executor()
            .spawn(async move { render_image(&picture) });
        cx.spawn(async move |this, cx| {
            let image = converting.await;
            this.update(cx, |this, cx| {
                this.show_preview(id, preview, image.map(Arc::new), cx);
            })
            .ok();
        })
        .detach();
    }

    fn show_preview(
        &mut self,
        id: PreviewId,
        preview: Option<Arc<Preview>>,
        picture: Option<Arc<RenderImage>>,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self.looks.open.as_mut().filter(|open| open.asked == id) else {
            release(picture, cx);
            return;
        };
        let shown = Shown {
            entry: open.entry,
            preview,
            picture,
        };
        let before = open.shown.replace(shown);
        release(before.and_then(|before| before.picture), cx);
        cx.notify();
    }
}

fn to_look_at(entries: &Entries) -> Option<(ListingId, EntryId)> {
    if entries.picker() {
        return None;
    }
    let listing = entries.listing()?.id;
    let ids = entries.selected_ids();
    let cursor = entries
        .cursor()
        .and_then(|cursor| u32::try_from(cursor).ok())
        .map(EntryId)
        .filter(|cursor| ids.contains(cursor));
    Some((listing, cursor.or_else(|| ids.first().copied())?))
}

fn entry_at(listing: &Listing, entry: EntryId) -> Option<&Entry> {
    listing.entries.get(usize::try_from(entry.0).ok()?)
}

fn release(images: impl IntoIterator<Item = Arc<RenderImage>>, cx: &mut App) {
    let images: Vec<_> = images.into_iter().collect();
    if images.is_empty() {
        return;
    }
    cx.defer(move |cx| {
        for image in images {
            cx.drop_image(image, None);
        }
    });
}

impl Part for PreviewPart {
    fn open(host: &Host, cx: &mut Context<Self>) -> Self {
        let watching = vec![
            cx.observe(&host.entries, |part, _, cx| part.entries_changed(cx)),
            cx.subscribe(&host.nav, |part, _, event: &NavEvent, cx| {
                if matches!(event, NavEvent::TabShown(_)) {
                    part.close(cx);
                }
            }),
        ];
        Self {
            host: host.clone(),
            looks: QuickLooks::default(),
            seen: None,
            cursor: None,
            _watching: watching,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(Self::arrived);
        slots.sends::<Command>();
        slots.before(Self::toggle);
        slots.before(Self::previous);
        slots.before(Self::next);
        slots.before(Self::up);
        slots.before(Self::down);
        slots.before(Self::enter);
        slots.before(Self::escape);
        slots.before(Self::holds::<TrashSelected>);
        slots.before(Self::holds::<DeleteSelected>);
        slots.before(Self::holds::<RenameSelected>);
        slots.before(Self::holds::<NewFolder>);
        slots.before(Self::holds::<Undo>);
        slots.staged(Self::stage);
        slots.layer(Self::layer);
    }
}
