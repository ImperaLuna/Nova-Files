#![forbid(unsafe_code)]

mod shown;
mod shrink;

use std::ops::Range;
use std::sync::Arc;

use gpui::{
    AnyElement, App, Context, IntoElement as _, ObjectFit, ParentElement as _, Pixels, RenderImage,
    Styled as _, StyledImage as _, Subscription, img,
};
use nova_files_api::{Entry, Listing, ListingId, ThumbnailSize};
use nova_files_shell::theme::radius;
use nova_files_shell::widgets::icon_slot;
use nova_files_shell::{Host, Part, Published, Slots, ThumbnailsShown};
use nova_files_thumbnails_api::Event;

use shown::ShownThumbnails;
pub use shrink::{device_side, shrink};

struct Shrunk {
    id: ListingId,
    index: usize,
    side: u32,
    image: RenderImage,
}

pub struct ThumbnailsPart {
    host: Host,
    shown: ShownThumbnails,
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
        let Event::Thumbnail {
            id,
            entry,
            size,
            png,
        } = event;
        if let Ok(index) = usize::try_from(entry.0)
            && size == ThumbnailSize::to_fill(self.side)
            && self.wanted.contains(&index)
            && !self.hidden
        {
            self.shrink((id, index), png, cx);
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
        self.listing = Some(listing);
        self.wanted = wanted;
    }

    fn side_changed(&mut self, side: u32, cx: &mut Context<Self>) {
        self.side = side;
        self.wanted = 0..0;
        release(self.shown.take_all(), cx);
    }

    fn listed(&mut self, listing: &Listing, _: bool, cx: &mut Context<Self>) {
        self.forget(cx);
        self.listing = Some(listing.id);
    }

    fn forget(&mut self, cx: &mut App) {
        self.wanted = 0..0;
        release(self.shown.take_all(), cx);
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
        }
        self.wanted = 0..0;
    }

    fn icon(&self, entry: &Entry, side: Pixels, cx: &App) -> Option<AnyElement> {
        if self.on_screen(cx) != self.listing {
            return None;
        }
        let image = self.thumbnail(usize::try_from(entry.id.0).ok()?)?;
        Some(
            icon_slot(side)
                .child(
                    img(Arc::clone(image))
                        .size(side)
                        .object_fit(ObjectFit::Contain)
                        .rounded(radius::SMALL),
                )
                .into_any_element(),
        )
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
    }
}
