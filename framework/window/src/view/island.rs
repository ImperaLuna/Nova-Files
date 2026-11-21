use gpui::{
    Action, BoxShadow, ClickEvent, Context, Div, ElementId, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Rgba, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, div, point, prelude::FluentBuilder as _, px,
    relative,
};
use nova_files_shell::widgets::tick_box;

use nova_files_api::{Answer, Conflict};
use nova_files_shell::{Look, MenuAt, MenuGroup, MenuItem, Placed, arrange};

use super::row::icon;
use crate::assets::Icon;
use crate::commands::{ClearSelection, TRASH, Undo};
use crate::island::progress::Running;
use crate::island::{self, Confirm, Note, asking};
use crate::theme::{Colors, radius, size, text};
use crate::window::MainWindow;
use crate::window::password::{asks_password, password_label};

mod several;

const UNDO_LABEL: &str = "Undo";
const CANCEL_LABEL: &str = "Cancel";

impl MainWindow {
    pub(super) fn island(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        let job = self.job_on_island(cx);
        let parts = if self.picker() {
            None
        } else if let Some((id, job, conflict)) =
            job.and_then(|(id, job)| Some((id, job, job.asking.as_ref()?)))
        {
            Some(if asks_password(conflict) {
                self.password_parts(conflict, password_label(&job.operation), &colors, cx)
            } else {
                ask_parts(
                    conflict,
                    self.island.ticked_for_all(id, conflict.number),
                    &colors,
                    cx,
                )
            })
        } else if let Some(confirm) = &self.island.confirm {
            Some(confirm_parts(confirm, &colors, cx))
        } else if let Some((_, job)) = job {
            let shown = self.jobs_shown(cx);
            if shown.len() > 1 {
                return several::island(&shown, self.island.jobs_open, &colors, cx)
                    .into_any_element();
            }
            Some(job_parts(job, &colors, cx))
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

    fn password_parts(
        &self,
        conflict: &Conflict,
        accept: &'static str,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let (name, rest) = asking::words(conflict);
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
        if let Some(shown) = &self.island.password {
            parts.push(password_box(shown.field.clone(), colors).into_any_element());
        }
        parts.push(cancel_button(colors, cx).into_any_element());
        parts.push(
            worded("island-skip", asking::label(Answer::Skip), colors)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.answer_conflict(Answer::Skip, cx);
                }))
                .into_any_element(),
        );
        parts.push(
            worded("island-unlock", accept, colors)
                .text_color(colors.accept)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.give_password(cx)))
                .into_any_element(),
        );
        parts
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
        let mut placed = if self.folder_rules().read_only {
            Vec::new()
        } else {
            file_buttons()
        };
        placed.extend(self.parts.menu(MenuAt::Island, &self.scene(cx), cx));
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

fn file_buttons() -> Vec<Placed> {
    vec![Placed::new(
        MenuGroup::Danger,
        Placed::FIRST,
        vec![
            MenuItem::run(Icon::Trash, &TRASH)
                .with_selector("island-trash")
                .icon_only(),
        ],
    )]
}

fn ask_parts(
    conflict: &Conflict,
    ticked: bool,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> Vec<gpui::AnyElement> {
    let (name, rest) = asking::words(conflict);
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

fn worded(
    selector: &'static str,
    words: impl Into<SharedString>,
    colors: &Colors,
) -> Stateful<Div> {
    square(selector, colors)
        .w_auto()
        .px(size::TEXT_BUTTON_PAD)
        .font_weight(text::STRONG_WEIGHT)
        .child(words.into())
}

fn cancel_button(colors: &Colors, cx: &Context<MainWindow>) -> Stateful<Div> {
    worded("island-cancel", CANCEL_LABEL, colors)
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_job(cx)))
}

fn password_box(field: gpui::Entity<nova_files_shell::text::TextField>, colors: &Colors) -> Div {
    div()
        .debug_selector(|| "island-password".to_owned())
        .flex()
        .flex_none()
        .items_center()
        .w(size::PASSWORD_WIDTH)
        .h(size::RENAME_HEIGHT)
        .px(size::RENAME_PAD)
        .rounded(radius::SMALL)
        .bg(colors.window)
        .border(size::RING)
        .border_color(colors.accept)
        .text_color(colors.text)
        .child(field)
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

fn job_parts(job: &Running, colors: &Colors, cx: &Context<MainWindow>) -> Vec<gpui::AnyElement> {
    let now = cx.background_executor().now();
    vec![
        label(colors).child(job.line(now)).into_any_element(),
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

fn floating(
    parts: Vec<gpui::AnyElement>,
    above: Option<gpui::AnyElement>,
    colors: &Colors,
) -> impl IntoElement {
    let row = div()
        .flex()
        .items_center()
        .min_w_0()
        .gap(size::ISLAND_GAP)
        .children(parts);
    div()
        .absolute()
        .left_0()
        .right_0()
        .bottom(size::ISLAND_BOTTOM)
        .flex()
        .justify_center()
        .px(size::ISLAND_MARGIN)
        .child(
            div()
                .id("island")
                .debug_selector(|| "island".to_owned())
                .occlude()
                .flex()
                .min_w_0()
                .map(|island| match above {
                    Some(above) => island
                        .flex_col()
                        .w(size::ISLAND_LIST_WIDTH)
                        .p(size::ISLAND_LIST_PAD)
                        .child(above)
                        .child(row.h(size::ISLAND_LIST_LINE).pl(size::ISLAND_LIST_INSET)),
                    None => island
                        .items_center()
                        .h(size::ISLAND_HEIGHT)
                        .pl(size::ISLAND_PAD_LEFT)
                        .pr(size::ISLAND_PAD_RIGHT)
                        .child(row),
                })
                .rounded(radius::CARD)
                .bg(colors.lifted)
                .border(size::LINE)
                .border_color(colors.line)
                .shadow(vec![BoxShadow {
                    color: Hsla::from(colors.shadow),
                    offset: point(px(0.0), size::ISLAND_SHADOW_Y),
                    blur_radius: size::ISLAND_SHADOW_BLUR,
                    spread_radius: px(0.0),
                }]),
        )
}

fn label(colors: &Colors) -> Div {
    div()
        .flex()
        .min_w_0()
        .mr(size::ISLAND_TEXT_GAP)
        .truncate()
        .text_color(colors.text_soft)
}

fn divider(colors: &Colors) -> impl IntoElement {
    div()
        .flex_none()
        .w(size::LINE)
        .h(size::ISLAND_DIVIDER_HEIGHT)
        .mx(size::ISLAND_GAP)
        .bg(colors.line)
}

fn square(name: impl Into<SharedString>, colors: &Colors) -> Stateful<Div> {
    let name = name.into();
    div()
        .id(ElementId::Name(name.clone()))
        .debug_selector(move || name.to_string())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(size::ICON_BUTTON)
        .rounded(radius::ROW)
        .cursor_pointer()
        .hover(|button| button.bg(colors.hover))
        .active(|button| button.bg(colors.press))
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
