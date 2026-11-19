use gpui::{AnyView, App, AppContext as _, Entity};

use crate::host::Host;
use crate::part::{Card, Part};
use crate::slot_view::{Draw, Fit, SlotView};

pub(super) fn card_views<P: Part>(
    entity: &Entity<P>,
    host: &Host,
    cards: &[Card<P>],
    fit: Fit,
    cx: &mut App,
) -> Vec<AnyView> {
    cards
        .iter()
        .map(|&card| {
            let (part, host) = (entity.clone(), host.clone());
            let draw: Draw = Box::new(move |window, cx| {
                let scene = host.scene(cx);
                part.update(cx, |part, cx| card(part, &scene, window, cx))
            });
            slot_view(entity, draw, fit, cx)
        })
        .collect()
}

pub(super) fn slot_view<P: Part>(part: &Entity<P>, draw: Draw, fit: Fit, cx: &mut App) -> AnyView {
    let part = part.clone();
    cx.new(|cx| {
        cx.observe(&part, |_, _, cx| cx.notify()).detach();
        SlotView::new(draw, fit)
    })
    .into()
}
