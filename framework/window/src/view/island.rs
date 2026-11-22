use gpui::{
    Action, ClickEvent, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, Rgba, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, div,
    prelude::FluentBuilder as _, relative,
};
use nova_files_shell::island::job_words::Words;
use nova_files_shell::island::look::{divider, floating, label, square, worded};
use nova_files_shell::widgets::tick_box;

use nova_files_api::{Answer, Conflict, ConflictKind};
use nova_files_shell::{Look, MenuAt, arrange};

use super::row::icon;
use crate::assets::Icon;
use crate::commands::{ClearSelection, Undo};
use crate::island::progress::Running;
use crate::island::{self, Confirm, Note, asking};
use crate::theme::{Colors, radius, size, text};
use crate::window::MainWindow;

mod several;

const UNDO_LABEL: &str = "Undo";
const CANCEL_LABEL: &str = "Cancel";

impl MainWindow {
    pub(super) fn island(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        let job = self.job_on_island(cx);
        let parts = if self.picker() {
            None
        } else if let Some((id, conflict)) =
            job.and_then(|(id, job)| Some((id, job.asking.as_ref()?)))
        {
            if let ConflictKind::Asked(question) = conflict.kind
                && let Some(view) = self.parts.question(question)
            {
                return view.into_any_element();
            }
            Some(ask_parts(
                conflict,
                (
                    self.island.ticked_for_all(id, conflict.number),
                    self.parts.words(),
                ),
                &colors,
                cx,
            ))
        } else if let Some(confirm) = &self.island.confirm {
            Some(confirm_parts(confirm, &colors, cx))
        } else if let Some(view) = self.parts.island(&self.scene(cx), cx) {
            return view.into_any_element();
        } else if let Some((_, job)) = job {
            let shown = self.jobs_shown(cx);
            if shown.len() > 1 {
                return several::island(
                    &shown,
                    (self.island.jobs_open, self.parts.words()),
                    &colors,
                    cx,
                )
                .into_any_element();
            }
            Some(job_parts(job, self.parts.words(), &colors, cx))
        } else if let Some(note) = &self.island.note {
            Some(note_parts(note, &colors, cx))
        } else if self.selected_count() > 0 {
            Some(self.selection_parts(&colors, cx))
        } else {
            None
        };
        parts.map_or_else(
            || self.quiet_count(&colors).into_any_element(),
            |parts| floating(parts, None, &colors).into_any_element(),
        )
    }

    fn selection_parts(&self, colors: &Colors, cx: &Context<Self>) -> Vec<gpui::AnyElement> {
        let count = self.selected_count();
        let (number, rest) = island::selection_line(count, self.selected_bytes());
        let mut parts = vec![
            label(colors)
                .child(
                    div()
                        .text_color(colors.text)
                        .font_weight(text::STRONG_WEIGHT)
                        .child(number),
                )
                .child(rest)
                .into_any_element(),
        ];
        let placed = self.parts.menu(MenuAt::Island, &self.scene(cx), cx);
        parts.extend(arrange(placed).into_iter().flatten().map(|item| {
            let action = item.run.action();
            if item.icon_only {
                return icon_button(item.selector(), item.icon, &*action, colors)
                    .into_any_element();
            }
            let tint = match item.look {
                Look::Plain => colors.text,
                Look::On => colors.accept,
                Look::Danger => colors.danger,
            };
            text_button(item.selector(), item.label, &*action, tint, colors).into_any_element()
        }));
        parts.extend([
            divider(colors).into_any_element(),
            icon_button("island-clear", Icon::Close, &ClearSelection, colors).into_any_element(),
        ]);
        parts
    }

    fn quiet_count(&self, colors: &Colors) -> impl IntoElement {
        let line = Some(self.counted.clone()).filter(|line| !line.is_empty());
        div().when_some(line, |slot, line| {
            slot.child(
                div()
                    .debug_selector(|| "count".to_owned())
                    .absolute()
                    .left(size::COUNT_LEFT)
                    .bottom(size::COUNT_BOTTOM)
                    .px(size::COUNT_PAD_X)
                    .py(size::COUNT_PAD_Y)
                    .rounded(radius::SMALL)
                    .bg(colors.sheet)
                    .text_size(text::LABEL)
                    .font_weight(text::DETAIL_WEIGHT)
                    .text_color(colors.text_dim)
                    .child(line),
            )
        })
    }
}

fn ask_parts(
    conflict: &Conflict,
    (ticked, words): (bool, &Words),
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> Vec<gpui::AnyElement> {
    let (name, rest) = asking::words(conflict, words);
    let mut parts = vec![
        label(colors)
            .child(
                div()
                    .text_color(colors.text)
                    .font_weight(text::STRONG_WEIGHT)
                    .child(name),
            )
            .child(rest)
            .into_any_element(),
    ];
    parts.push(cancel_button(colors, cx).into_any_element());
    let answer = |answer: Answer| {
        worded(asking::selector(answer), asking::label(answer), colors).on_click(cx.listener(
            move |this, _: &ClickEvent, _, cx| {
                this.answer_conflict(answer, cx);
            },
        ))
    };
    let answers = asking::answers(conflict.kind);
    if let Some((&last, rest)) = answers.split_last() {
        parts.extend(rest.iter().map(|&each| answer(each).into_any_element()));
        parts.push(answer(last).text_color(colors.accept).into_any_element());
    }
    if let Some(words) = asking::for_all(conflict) {
        parts.push(divider(colors).into_any_element());
        parts.push(for_all_tick(words, ticked, colors, cx).into_any_element());
    }
    parts
}

fn cancel_button(colors: &Colors, cx: &Context<MainWindow>) -> Stateful<Div> {
    worded("island-cancel", CANCEL_LABEL, colors)
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_job(cx)))
}

fn for_all_tick(
    words: String,
    ticked: bool,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> impl IntoElement {
    square("island-for-all", colors)
        .w_auto()
        .px(size::TEXT_BUTTON_PAD)
        .gap(size::TICK_GAP)
        .text_color(colors.text_soft)
        .child(tick_box(ticked, colors))
        .child(words)
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_for_all(cx)))
}

fn job_parts(
    job: &Running,
    words: &Words,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> Vec<gpui::AnyElement> {
    let now = cx.background_executor().now();
    vec![
        label(colors).child(job.line(now, words)).into_any_element(),
        bar(job.fraction(), size::PROGRESS_WIDTH, colors)
            .debug_selector(|| "island-progress".to_owned())
            .mr(size::PROGRESS_RIGHT)
            .into_any_element(),
    ]
}

fn bar(fraction: f32, width: Pixels, colors: &Colors) -> Div {
    div()
        .flex_none()
        .w(width)
        .h(size::PROGRESS_HEIGHT)
        .rounded_full()
        .bg(colors.press)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative(fraction))
                .rounded_full()
                .bg(colors.accept),
        )
}

fn confirm_parts(
    confirm: &Confirm,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> Vec<gpui::AnyElement> {
    let answer = |name: &'static str, words: &'static str, yes: bool| {
        square(name, colors)
            .w_auto()
            .px(size::TEXT_BUTTON_PAD)
            .font_weight(text::STRONG_WEIGHT)
            .when(yes, |button| button.text_color(colors.danger))
            .child(SharedString::new_static(words))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.answer_confirm(yes, cx)))
            .into_any_element()
    };
    vec![
        label(colors)
            .child(confirm.label.clone())
            .into_any_element(),
        answer("island-no", CANCEL_LABEL, false),
        answer("island-yes", confirm.yes, true),
    ]
}

fn note_parts(note: &Note, colors: &Colors, cx: &Context<MainWindow>) -> Vec<gpui::AnyElement> {
    let mut parts = vec![label(colors).child(note.label.clone()).into_any_element()];
    if note.undo.is_some() {
        parts.push(
            text_button("island-undo", UNDO_LABEL, &Undo, colors.accept, colors).into_any_element(),
        );
    }
    parts.push(
        square("island-dismiss", colors)
            .child(icon(Icon::Close, size::BUTTON_ICON, colors.text_soft))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.dismiss_note(cx)))
            .into_any_element(),
    );
    parts
}

fn icon_button(
    name: &'static str,
    glyph: Icon,
    action: &dyn Action,
    colors: &Colors,
) -> impl IntoElement {
    let action = action.boxed_clone();
    square(name, colors)
        .child(icon(glyph, size::BUTTON_ICON, colors.text_soft))
        .on_click(move |_: &ClickEvent, window, cx| {
            window.dispatch_action(action.boxed_clone(), cx);
        })
}

fn text_button(
    name: &'static str,
    words: impl Into<SharedString>,
    action: &dyn Action,
    tint: Rgba,
    colors: &Colors,
) -> impl IntoElement {
    let action = action.boxed_clone();
    worded(name, words, colors)
        .text_color(tint)
        .on_click(move |_: &ClickEvent, window, cx| {
            window.dispatch_action(action.boxed_clone(), cx);
        })
}
