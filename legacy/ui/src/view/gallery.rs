use gpui::{
    Bounds, Context, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    SharedString, Styled as _, canvas, div,
};

use super::preview::{meta, stage_art};
use super::sheet::rename_box;
use crate::grid::Grid;
use crate::theme::{Colors, size, text};
use crate::window::{MainWindow, QuickLook};
use nova_files_api::{Entry, Listing};

impl MainWindow {
    pub(super) fn gallery(&self, grid: Grid, cx: &Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        let bounds = self.gallery_bounds().clone();
        let view = cx.entity().downgrade();
        div()
            .debug_selector(|| "gallery".to_owned())
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .child(
                canvas(
                    move |drawn: Bounds<Pixels>, _, cx| {
                        if bounds.drawn(drawn) {
                            view.update(cx, |_, cx| cx.notify()).ok();
                        }
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .child(self.stage(&colors, cx))
            .child(self.strip(grid, cx))
    }

    fn on_stage(&self) -> Option<(&QuickLook, &Listing, &Entry)> {
        let open = self.quick_look()?;
        let listing = self.listing.as_ref()?;
        let index = usize::try_from(open.entry.0).ok()?;
        let entry = listing.entries.get(index)?;
        Some((open, listing, entry))
    }

    fn stage(&self, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
        let room = self.stage_room();
        let scale_factor = self.scale_factor;
        let part_stage = self.parts.stage(&self.scene(), room, cx);
        let shown = part_stage.is_none().then(|| self.on_stage()).flatten();
        div()
            .debug_selector(|| "stage".to_owned())
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .items_center()
            .justify_center()
            .gap(size::STAGE_PAD)
            .p(size::STAGE_PAD)
            .children(part_stage)
            .children(shown.map(|(open, listing, entry)| {
                let current = open.current().and_then(|shown| shown.preview.as_deref());
                let name = SharedString::from(entry.name.to_string_lossy().into_owned());
                let about = current.map(meta).unwrap_or_default();
                let part_name = self.parts.name_cell(entry, cx);
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(size::STAGE_PAD)
                    .min_w_0()
                    .child(stage_art(
                        open,
                        listing,
                        entry,
                        (room, scale_factor),
                        colors,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .h(size::STAGE_CAPTION)
                            .w_full()
                            .child(part_name.map_or_else(
                                || {
                                    self.parts.name_field(entry, cx).map_or_else(
                                        || {
                                            div()
                                                .debug_selector(|| "stage-name".to_owned())
                                                .max_w_full()
                                                .truncate()
                                                .text_color(colors.text)
                                                .font_weight(text::STRONG_WEIGHT)
                                                .child(name)
                                                .into_any_element()
                                        },
                                        |field| {
                                            rename_box(field, colors)
                                                .flex_none()
                                                .w_full()
                                                .ml_0()
                                                .into_any_element()
                                        },
                                    )
                                },
                                |cell| div().flex_none().w_full().child(cell).into_any_element(),
                            ))
                            .child(
                                div()
                                    .text_size(text::DETAIL)
                                    .text_color(colors.text_soft)
                                    .child(about),
                            ),
                    )
                    .into_any_element()
            }))
    }

    fn strip(&self, grid: Grid, cx: &Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        let cells = self.listing.as_ref().map(|listing| {
            self.strip_entries(listing.entries.len())
                .map(|index| self.icon_cell(listing, index, grid, cx))
                .collect::<Vec<_>>()
        });
        div()
            .debug_selector(|| "strip".to_owned())
            .flex()
            .flex_none()
            .justify_center()
            .pt(size::STRIP_PAD)
            .pb(size::STRIP_PAD + size::ISLAND_CLEARANCE)
            .border_t(size::LINE)
            .border_color(colors.line)
            .children(cells.unwrap_or_default())
    }
}
