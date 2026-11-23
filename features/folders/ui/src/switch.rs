use gpui::{
    Action, AnyElement, ClickEvent, Context, Div, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Point, Radians, Stateful, StatefulInteractiveElement as _,
    Styled as _, Transformation, prelude::FluentBuilder as _, px, svg,
};
use nova_files_folders_api::ViewMode;
use nova_files_shell::actions::{ShowDetails, ShowIcons};
use nova_files_shell::assets::Icon;
use nova_files_shell::theme::{Colors, size};
use nova_files_shell::widgets::{capsule, icon, square};

use crate::FoldersPart;
use crate::slider::size_slider;

const ARROW: Pixels = px(10.0);
const ARROW_PAD: Pixels = px(6.0);
const ARROW_TURN: Radians = Radians(std::f32::consts::FRAC_PI_2);

impl FoldersPart {
    pub(crate) fn view_switch(&mut self, cx: &Context<Self>) -> AnyElement {
        let colors = Colors::of(cx);
        let view = self.shown(cx);
        if view.mode != ViewMode::Icons {
            self.slider.close();
        }
        let details = mode_button(
            ("view-list", Icon::List, ViewMode::Details),
            &ShowDetails,
            view.mode,
            &colors,
        );
        let icons = if view.mode == ViewMode::Icons {
            sizes_button(&colors, cx)
        } else {
            mode_button(
                ("view-tiles", Icon::Tiles, ViewMode::Icons),
                &ShowIcons,
                view.mode,
                &colors,
            )
        };
        let slider = self.slider.is_open().then(|| size_slider(view, cx));
        capsule(&colors)
            .debug_selector(|| "views".to_owned())
            .relative()
            .px(size::CAPSULE_PAD)
            .gap(size::CAPSULE_BUTTON_GAP)
            .child(details)
            .child(icons)
            .children(slider)
            .into_any_element()
    }
}

fn mode_button(
    (name, glyph, mode): (&'static str, Icon, ViewMode),
    action: &dyn Action,
    showing: ViewMode,
    colors: &Colors,
) -> Stateful<Div> {
    let on = showing == mode;
    let tone = if on { colors.text } else { colors.text_faint };
    let action = action.boxed_clone();
    square(name)
        .when(on, |button| button.bg(colors.press))
        .child(icon(glyph, size::BUTTON_ICON, tone))
        .when(!on, |button| {
            button
                .cursor_pointer()
                .hover(|button| button.bg(colors.hover))
                .on_click(move |_: &ClickEvent, window, cx| {
                    window.dispatch_action(action.boxed_clone(), cx);
                })
        })
}

fn sizes_button(colors: &Colors, cx: &Context<FoldersPart>) -> Stateful<Div> {
    let arrow = svg()
        .path(Icon::Chevron.path())
        .flex_none()
        .size(ARROW)
        .text_color(gpui::Hsla::from(colors.text_dim))
        .with_transformation(Transformation::rotate(ARROW_TURN));
    square("view-tiles")
        .w_auto()
        .gap(size::CAPSULE_BUTTON_GAP)
        .px(ARROW_PAD)
        .bg(colors.press)
        .cursor_pointer()
        .child(icon(Icon::Tiles, size::BUTTON_ICON, colors.text))
        .child(arrow)
        .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
            this.slider.toggle(pressed_at(event));
            cx.notify();
        }))
}

const fn pressed_at(event: &ClickEvent) -> Option<Point<Pixels>> {
    match event {
        ClickEvent::Mouse(click) => Some(click.down.position),
        ClickEvent::Keyboard(_) | ClickEvent::Touch(_) => None,
    }
}
