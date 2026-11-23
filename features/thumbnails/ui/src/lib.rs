#![forbid(unsafe_code)]

mod folder_look;
mod peeks;
mod shown;

use std::ops::Range;
use std::sync::Arc;

use gpui::{AnyElement, App, Context, IntoElement as _, Pixels, RenderImage, Subscription};
use nova_files_api::{Entry, EntryKind, HiddenFiles, Listing, ListingId, ThumbnailSize};
use nova_files_shell::kind::icon_kind;
use nova_files_shell::theme::Colors;
use nova_files_shell::{Host, Part, Published, Slots, ThumbnailsShown};
use nova_files_thumbnails_api::{Event, Peeked};

use nova_files_shell::shrink::shrink;
use peeks::{PeekItem, ShownPeeks};
use shown::ShownThumbnails;

struct Shrunk {
    id: ListingId,
    index: usize,
    side: u32,
    image: RenderImage,
}

pub struct ThumbnailsPart {
    host: Host,
    shown: ShownThumbnails,
    peeks: ShownPeeks,
    listing: Option<ListingId>,
    wanted: Range<usize>,
    side: u32,
    hidden: bool,
    _watching: Vec<Subscription>,
}

impl ThumbnailsPart {
    pub fn thumbnail(&self, index: usize) -> Option<&Arc<RenderImage>> {
        self.shown.get(index)
    }

    fn arrived(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Thumbnail {
                id,
                entry,
                size,
                png,
            } => {
                if let Some(index) = self.wants(entry.0, size) {
                    self.shrink((id, index), png, cx);
                }
            }
            Event::Peek {
                id,
                entry,
                size,
                peeked,
            } => {
                if let Some(index) = self.wants(entry.0, size) {
                    self.shrink_peek((id, index), peeked, cx);
                }
            }
        }
    }

    fn wants(&self, entry: u32, size: ThumbnailSize) -> Option<usize> {
        let index = usize::try_from(entry).ok()?;
        (size == ThumbnailSize::to_fill(self.side) && self.wanted.contains(&index) && !self.hidden)
            .then_some(index)
    }

    fn shrink_peek(
        &self,
        (id, index): (ListingId, usize),
        peeked: Vec<Peeked>,
        cx: &Context<Self>,
    ) {
        let side = self.side;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "a thumbnail side of a few hundred pixels"
        )]
        let peek_side = (side as f32 * folder_look::PEEK_SHARE).ceil() as u32;
        let shrinking = cx.background_executor().spawn(async move {
            peeked
                .into_iter()
                .map(|item| PeekItem {
                    kind: icon_kind(&item.name, item.kind),
                    hidden: item.is_hidden(),
                    image: item
                        .png
                        .and_then(|png| shrink(&png, peek_side))
                        .map(Arc::new),
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let items = shrinking.await;
            this.update(cx, |this, cx| this.peek_ready((id, index, side), items, cx))
                .ok();
        })
        .detach();
    }

    fn peek_ready(
        &mut self,
        (id, index, side): (ListingId, usize, u32),
        items: Vec<PeekItem>,
        cx: &mut Context<Self>,
    ) {
        if self.on_screen(cx) == Some(id)
            && self.wanted.contains(&index)
            && side == self.side
            && !self.hidden
        {
            let replaced = self.peeks.insert(index, items);
            release(replaced, cx);
            cx.notify();
        } else {
            release(items.into_iter().filter_map(|item| item.image), cx);
        }
    }

    fn shrink(&self, (id, index): (ListingId, usize), png: Arc<[u8]>, cx: &Context<Self>) {
        let side = self.side;
        let shrinking = cx
            .background_executor()
            .spawn(async move { shrink(&png, side) });
        cx.spawn(async move |this, cx| {
            let Some(image) = shrinking.await else { return };
            let made = Shrunk {
                id,
                index,
                side,
                image,
            };
            this.update(cx, |this, cx| this.ready(made, cx)).ok();
        })
        .detach();
    }

    fn ready(&mut self, made: Shrunk, cx: &mut Context<Self>) {
        if self.on_screen(cx) == Some(made.id)
            && self.wanted.contains(&made.index)
            && made.side == self.side
            && !self.hidden
        {
            let replaced = self.shown.insert(made.index, Arc::new(made.image));
            release(replaced, cx);
            cx.notify();
        }
    }

    fn on_screen(&self, cx: &App) -> Option<ListingId> {
        self.host
            .entries
            .read(cx)
            .listing()
            .map(|listing| listing.id)
    }

    fn rows_drawn(&mut self, listing: ListingId, wanted: Range<usize>, cx: &mut Context<Self>) {
        release(self.shown.keep_rows(&wanted), cx);
        release(self.peeks.keep_rows(&wanted), cx);
        self.listing = Some(listing);
        self.wanted = wanted;
    }

    fn side_changed(&mut self, side: u32, cx: &mut Context<Self>) {
        self.side = side;
        self.forget(cx);
    }

    fn listed(&mut self, listing: &Listing, _: bool, cx: &mut Context<Self>) {
        self.forget(cx);
        self.listing = Some(listing.id);
    }

    fn forget(&mut self, cx: &mut App) {
        self.wanted = 0..0;
        release(self.shown.take_all(), cx);
        release(self.peeks.take_all(), cx);
    }

    fn entries_changed(&mut self, cx: &mut Context<Self>) {
        if self.on_screen(cx) != self.listing {
            self.listing = self.on_screen(cx);
            self.forget(cx);
        }
    }

    fn shown_changed(&mut self, cx: &mut Context<Self>) {
        let Some(shown) = Published::<ThumbnailsShown>::get(cx).copied() else {
            return;
        };
        let hidden = shown == ThumbnailsShown::Hidden;
        if hidden == self.hidden {
            return;
        }
        self.hidden = hidden;
        if hidden {
            release(self.shown.take_all(), cx);
            release(self.peeks.take_all(), cx);
        }
        self.wanted = 0..0;
    }

    fn picture(&self, entry: &Entry, cx: &App) -> Option<Arc<RenderImage>> {
        if self.on_screen(cx) != self.listing {
            return None;
        }
        self.thumbnail(usize::try_from(entry.id.0).ok()?).cloned()
    }

    fn icon(&self, entry: &Entry, side: Pixels, cx: &App) -> Option<AnyElement> {
        if entry.kind == EntryKind::Folder {
            return self.folder_icon(entry, side, cx);
        }
        let image = self.picture(entry, cx)?;
        Some(folder_look::picture(&image, side).into_any_element())
    }
}

impl ThumbnailsPart {
    fn folder_icon(&self, entry: &Entry, side: Pixels, cx: &App) -> Option<AnyElement> {
        if self.hidden {
            return None;
        }
        let hidden_shown = Published::<HiddenFiles>::get(cx) == Some(&HiddenFiles::Shown);
        let peek = usize::try_from(entry.id.0)
            .ok()
            .filter(|_| self.on_screen(cx) == self.listing)
            .and_then(|index| self.peeks.get(index))
            .unwrap_or_default();
        let items: Vec<&PeekItem> = peek
            .iter()
            .filter(|item| hidden_shown || !item.hidden)
            .collect();
        folder_look::folder(entry.id.0, &items, side, &Colors::of(cx))
    }
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

impl Part for ThumbnailsPart {
    fn open(host: &Host, cx: &mut Context<Self>) -> Self {
        let hidden = Published::<ThumbnailsShown>::get(cx) == Some(&ThumbnailsShown::Hidden);
        let side = host.entries.read(cx).thumbnail_side();
        let watching = vec![
            cx.observe(&host.entries, |part, _, cx| part.entries_changed(cx)),
            cx.observe_global::<Published<ThumbnailsShown>>(Self::shown_changed),
        ];
        Self {
            host: host.clone(),
            shown: ShownThumbnails::default(),
            peeks: ShownPeeks::default(),
            listing: None,
            wanted: 0..0,
            side,
            hidden,
            _watching: watching,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(Self::arrived);
        slots.rows_drawn(Self::rows_drawn);
        slots.thumbnail_side(Self::side_changed);
        slots.listed(Self::listed);
        slots.row_icon(Self::icon);
        slots.row_picture(Self::picture);
    }
}
