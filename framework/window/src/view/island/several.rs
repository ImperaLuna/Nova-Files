use gpui::{
    ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div,
};
use nova_files_api::JobId;

use nova_files_shell::island::look::{chevron, floating, label, square};

use super::bar;
use crate::assets::Icon;
use crate::island::progress::Running;
use crate::island::several::{fraction, left, line, title};
use crate::theme::{Colors, size, text};
use crate::view::row::icon;
use crate::window::MainWindow;

pub(super) fn island(
    jobs: &[(JobId, &Running)],
    open: bool,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> impl IntoElement {
    let now = cx.background_executor().now();
    let running: Vec<&Running> = jobs.iter().map(|(_, job)| *job).collect();
    let (many, rest) = line(&running, now);
    let mut parts = vec![
        label(colors)
            .flex_1()
            .child(
                div()
                    .text_color(colors.text)
                    .font_weight(text::STRONG_WEIGHT)
                    .child(many),
            )
            .child(rest)
            .into_any_element(),
    ];
    if !open {
        parts.push(
            bar(fraction(&running), size::PROGRESS_WIDTH, colors)
                .debug_selector(|| "island-jobs-bar".to_owned())
                .mr(size::PROGRESS_RIGHT)
                .into_any_element(),
        );
    }
    parts.push(arrow(open, colors, cx).into_any_element());
    let rows = open.then(|| {
        div()
            .id("island-jobs")
            .debug_selector(|| "island-jobs".to_owned())
            .flex()
            .flex_col()
            .max_h(size::ISLAND_LIST_ROW * size::ISLAND_LIST_ROWS)
            .overflow_y_scroll()
            .pb(size::ISLAND_LIST_PAD)
            .mb(size::ISLAND_LIST_PAD)
            .border_b(size::LINE)
            .border_color(colors.line)
            .children(
                jobs.iter()
                    .enumerate()
                    .map(|(index, (id, job))| row(index, *id, job, colors, cx)),
            )
            .into_any_element()
    });
    floating(parts, rows, colors)
}

fn arrow(open: bool, colors: &Colors, cx: &Context<MainWindow>) -> impl IntoElement {
    square("island-jobs-open", colors)
        .child(chevron(open, colors))
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_jobs_open(cx)))
}

fn row(
    index: usize,
    id: JobId,
    job: &Running,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> impl IntoElement {
    let now = cx.background_executor().now();
    div()
        .debug_selector(move || format!("island-job-{index}"))
        .flex()
        .items_center()
        .min_w_0()
        .h(size::ISLAND_LIST_ROW)
        .pl(size::ISLAND_LIST_INSET)
        .gap(size::ISLAND_LIST_GAP)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(colors.text)
                .child(title(job, now)),
        )
        .child(
            div()
                .flex_none()
                .w(size::ISLAND_LIST_LEFT)
                .text_right()
                .text_size(text::LABEL)
                .font_weight(text::DETAIL_WEIGHT)
                .text_color(colors.text_dim)
                .children(left(job, now)),
        )
        .child(bar(job.fraction(), size::ISLAND_LIST_BAR, colors))
        .child(
            square(format!("island-stop-{index}"), colors)
                .child(icon(Icon::Close, size::BUTTON_ICON, colors.text_soft))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, _| this.stop_job(id))),
        )
}
