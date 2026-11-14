//! The sidebar, on the window to the left of the bar and the sheet. For
//! now it holds the places; the Open list, the folder tree and the drives
//! join it as features of their own. The open place is pressed in, its
//! name strong and its icon orange.

use gpui::{
    ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
};
use nova_files_api::{Place, PlaceKind};

use super::row::icon;
use crate::assets::Icon;
use crate::theme::{Colors, radius, size, text};
use crate::window::MainWindow;

const HOME_LABEL: &str = "Home";

impl MainWindow {
    pub(super) fn sidebar(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = Colors::of(cx);
        let open = self.folder();
        let rows = self.places().iter().enumerate().map(|(index, place)| {
            let is_open = open == Some(place.path.as_path());
            let target = place.path.clone();
            let tint = if is_open {
                colors.here
            } else {
                colors.text_dim
            };
            div()
                .id(("place", index))
                .debug_selector(move || format!("place-{index}"))
                .w_full()
                .flex()
                .flex_none()
                .items_center()
                .gap(size::PLACE_PAD)
                .h(size::PLACE_HEIGHT)
                .px(size::PLACE_PAD)
                .rounded(radius::ROW)
                .cursor_pointer()
                .hover(|row| row.bg(colors.hover))
                .when(is_open, |row| {
                    row.bg(colors.press)
                        .text_color(colors.text)
                        .font_weight(text::STRONG_WEIGHT)
                })
                .on_click(cx.listener(move |this, _: &ClickEvent, _, _| {
                    this.open(&target);
                }))
                .child(icon(place_icon(place.kind), size::PLACE_ICON, tint))
                .child(div().min_w_0().truncate().child(place_name(place)))
        });
        div()
            .id("sidebar")
            .debug_selector(|| "sidebar".to_owned())
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(size::PLACE_GAP)
            .w(size::SIDEBAR_WIDTH)
            .pb(size::GAP)
            .overflow_y_scroll()
            .children(rows)
    }
}

/// The icon a place is drawn with.
pub const fn place_icon(kind: PlaceKind) -> Icon {
    match kind {
        PlaceKind::Home => Icon::Home,
        PlaceKind::Desktop => Icon::Desktop,
        PlaceKind::Documents => Icon::Documents,
        PlaceKind::Downloads => Icon::Downloads,
        PlaceKind::Music => Icon::Music,
        PlaceKind::Pictures => Icon::Pictures,
        PlaceKind::Videos => Icon::Videos,
    }
}

/// A place's label: "Home" for the home folder, otherwise the folder's own
/// name, which follows the user's language ("Dokumente", "Images").
fn place_name(place: &Place) -> SharedString {
    match place.kind {
        PlaceKind::Home => SharedString::from(HOME_LABEL),
        PlaceKind::Desktop
        | PlaceKind::Documents
        | PlaceKind::Downloads
        | PlaceKind::Music
        | PlaceKind::Pictures
        | PlaceKind::Videos => place
            .path
            .file_name()
            .map(|name| SharedString::from(name.to_string_lossy().into_owned()))
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn places_are_named_by_their_folder_except_home() {
        let place = |kind, path: &str| Place {
            kind,
            path: PathBuf::from(path),
        };
        assert_eq!(place_name(&place(PlaceKind::Home, "/home/me")), "Home");
        assert_eq!(
            place_name(&place(PlaceKind::Documents, "/home/me/Dokumente")),
            "Dokumente"
        );
    }

    #[test]
    fn each_place_has_its_own_icon() {
        let kinds = [
            PlaceKind::Home,
            PlaceKind::Desktop,
            PlaceKind::Documents,
            PlaceKind::Downloads,
            PlaceKind::Music,
            PlaceKind::Pictures,
            PlaceKind::Videos,
        ];
        let icons: Vec<Icon> = kinds.into_iter().map(place_icon).collect();
        for (index, icon) in icons.iter().enumerate() {
            assert!(!icons[..index].contains(icon), "{icon:?} drawn twice");
        }
        assert_eq!(place_icon(PlaceKind::Home), Icon::Home);
    }
}
