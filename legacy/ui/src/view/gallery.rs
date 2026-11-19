use gpui::{
    AnyElement, Bounds, Context, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    SharedString, Styled as _, Window, canvas, div,
};

use super::sheet::rename_box;
use crate::theme::{Colors, size, text};
use crate::window::MainWindow;
use nova_files_folders_ui::Grid;

impl MainWindow {
    pub(super) fn gallery(
        &self,
        grid: Grid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = Colors::of(cx);
        let stage = self.stage(&colors, window, cx);
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
            .child(stage)
            .child(self.strip(grid, cx))
    }

    fn stage(&self, colors: &Colors, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let room = self.stage_room(cx);
        let scene = self.scene(cx);
        let staged = self.parts.staged(&scene, room, window, cx);
        let part_stage = self.parts.stage(&scene, room, window, cx);
        let shown = staged.filter(|_| part_stage.is_none()).and_then(|staged| {
            let index = usize::try_from(staged.entry.0).ok()?;
            let entry = self.listing.as_ref()?.entries.get(index)?;
            Some((staged, entry))
        });
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
            .children(shown.map(|(staged, entry)| {
                let name = SharedString::from(entry.name.to_string_lossy().into_owned());
                let part_name = self.parts.name_cell(entry, cx);
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(size::STAGE_PAD)
                    .min_w_0()
                    .child(staged.art)
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
                                    .child(staged.about),
                            ),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    fn strip(&self, grid: Grid, cx: &Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        let cells = self.listing.as_ref().map(|listing| {
            self.strip_entries(listing.entries.len(), cx)
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
