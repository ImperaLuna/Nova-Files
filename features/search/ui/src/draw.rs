use gpui::{
    AnyElement, App, ClickEvent, Context, Div, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div,
};
use nova_files_shell::Scene;
use nova_files_shell::actions::CursorDown;
use nova_files_shell::assets::Icon;
use nova_files_shell::theme::{Colors, radius, size, text};
use nova_files_shell::widgets::icon;

use crate::{Scope, SearchPart};

const ONE_MATCH: &str = "1 match";
const EVERYWHERE: &str = "Everywhere";
const HERE_ONLY: &str = "Here only";
const NO_MATCHES: &str = "No matches";
const CANT_SEARCH: &str = "Can't search";
const SEARCH_LABEL: &str = "Search";

impl SearchPart {
    pub fn search_box(
        &self,
        capsule: Div,
        colors: &Colors,
        entity: &Entity<Self>,
        cx: &App,
    ) -> Div {
        if self.host.entries.read(cx).picker() {
            return capsule
                .text_color(colors.text_dim)
                .child(icon(Icon::Search, size::BUTTON_ICON, colors.text_dim))
                .child(div().min_w_0().truncate().child(SEARCH_LABEL));
        }
        let part = entity.downgrade();
        capsule
            .on_action(move |_: &CursorDown, _, cx| {
                part.update(cx, |this, cx| this.focus_results(cx)).ok();
            })
            .child(icon(Icon::Search, size::BUTTON_ICON, colors.text_dim))
            .child(
                div()
                    .debug_selector(|| "search-box".to_owned())
                    .flex_1()
                    .min_w_0()
                    .text_color(colors.text)
                    .children(self.shown_field(cx)),
            )
    }

    pub(crate) fn scope_chip(
        &mut self,
        scene: &Scene,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !scene.searching {
            return None;
        }
        let colors = Colors::of(cx);
        let (label, other) = match self.scope(cx) {
            Scope::Everywhere => (EVERYWHERE, Scope::HereOnly),
            Scope::HereOnly => (HERE_ONLY, Scope::Everywhere),
        };
        Some(
            div()
                .id("scope")
                .debug_selector(|| "scope".to_owned())
                .flex()
                .flex_none()
                .items_center()
                .h(size::CHIP_HEIGHT)
                .px(size::CHIP_PAD)
                .rounded(radius::ROW)
                .bg(colors.press)
                .text_size(text::LABEL)
                .font_weight(text::DETAIL_WEIGHT)
                .text_color(colors.text_soft)
                .cursor_pointer()
                .hover(|chip| chip.bg(colors.selected).text_color(colors.text))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.set_scope(other, cx);
                }))
                .child(label)
                .into_any_element(),
        )
    }

    pub(crate) fn match_count(
        &mut self,
        _: &Scene,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let found = self.found_count(cx).filter(|&count| count > 0)?;
        let colors = Colors::of(cx);
        Some(
            div()
                .debug_selector(|| "match-count".to_owned())
                .absolute()
                .left(size::STATUS_INSET)
                .bottom(size::STATUS_INSET)
                .px(size::STATUS_PAD_X)
                .py(size::STATUS_PAD_Y)
                .rounded(radius::SMALL)
                .bg(colors.sheet)
                .text_size(text::LABEL)
                .font_weight(text::DETAIL_WEIGHT)
                .text_color(colors.text_dim)
                .child(match_label(found))
                .into_any_element(),
        )
    }

    pub(crate) fn no_matches(
        &mut self,
        _: &Scene,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let label = if self.host.entries.read(cx).over_failed() {
            CANT_SEARCH
        } else {
            NO_MATCHES
        };
        let colors = Colors::of(cx);
        (self.found_count(cx) == Some(0)).then(|| {
            div()
                .debug_selector(|| "no-matches".to_owned())
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(size::EMPTY_GAP)
                .text_color(colors.text_dim)
                .child(icon(Icon::Search, size::EMPTY_ICON, colors.text_faint))
                .child(label)
                .into_any_element()
        })
    }

    fn found_count(&self, cx: &App) -> Option<usize> {
        let entries = self.host.entries.read(cx);
        let listing = entries.listing()?;
        (entries.searching() && listing.folders.is_some()).then(|| listing.entries.len())
    }
}

fn match_label(found: usize) -> String {
    if found == 1 {
        ONE_MATCH.to_owned()
    } else {
        format!("{found} matches")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_match_is_singular() {
        assert_eq!(match_label(1), "1 match");
        assert_eq!(match_label(2), "2 matches");
    }
}
