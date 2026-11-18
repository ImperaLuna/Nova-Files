//! The Properties page's Access group: one row each for the owner, the
//! group and everyone else, with the level each has on a chip that opens
//! a menu of levels (see `window::access`), then a tick: "Run as a
//! program" for a lone file, "Also everything inside" when there are
//! folders. Several items with different levels show "Mixed". Where the
//! user can't change it, the chips and ticks do nothing.
//!
//! Debug selectors: `access-<n>` for each row's chip, counted from 0 in
//! the order owner, group, everyone else; `access-runs` and
//! `access-inside` for the ticks.

use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, ParentElement as _, StatefulInteractiveElement as _, Styled as _,
    Transformation, div, prelude::FluentBuilder as _, radians, svg,
};
use nova_files_api::{Account, PathKind, Properties};

use crate::assets::Icon;
use crate::theme::{Colors, properties, radius, size, text};
use crate::view::island::tick_box;
use crate::view::row::icon;
use crate::view::start::group;
use crate::window::{MainWindow, Who};

const LABEL: &str = "Access";
const OWNER_LABEL: &str = "Owner";
const GROUP_LABEL: &str = "Group";
const OTHERS_LABEL: &str = "Everyone else";
const MIXED_LABEL: &str = "Mixed";
const RUNS_LABEL: &str = "Run as a program";
const INSIDE_LABEL: &str = "Also everything inside";

impl MainWindow {
    /// The Access group for `read`, what the page shows.
    pub(super) fn access(&self, read: &[Arc<Properties>], cx: &Context<Self>) -> AnyElement {
        let colors = Colors::of(cx);
        let may = self.may_change_access();
        let rows = Who::ALL
            .into_iter()
            .enumerate()
            .map(|(index, who)| access_row(index, who, read, may, cx).into_any_element(&colors));
        let tick = match read {
            [one] if one.kind == PathKind::File => Some(
                Tick {
                    selector: "access-runs",
                    label: RUNS_LABEL,
                    ticked: crate::window::runs(one.mode),
                    may,
                }
                .row(
                    cx.listener(|this, _: &ClickEvent, _, _| this.toggle_runs()),
                    &colors,
                ),
            ),
            _ if read.iter().any(|one| one.kind == PathKind::Folder) => Some(
                Tick {
                    selector: "access-inside",
                    label: INSIDE_LABEL,
                    ticked: self.reaches_inside(),
                    may,
                }
                .row(
                    cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_inside(cx)),
                    &colors,
                ),
            ),
            _ => None,
        };
        let card = div()
            .flex()
            .flex_col()
            .gap(size::PLACE_GAP)
            .p(properties::ACCESS_PAD)
            .rounded(radius::CARD)
            .bg(colors.raised)
            .children(rows)
            .children(tick);
        group(LABEL, card.into_any_element(), &colors).into_any_element()
    }
}

/// The parts of the row for `who`.
struct RowParts {
    name: String,
    role: &'static str,
    chip: gpui::Stateful<gpui::Div>,
}

impl RowParts {
    fn into_any_element(self, colors: &Colors) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap(properties::ACCESS_ROW_GAP)
            .h(size::ROW_HEIGHT)
            .pl(size::ROW_PAD)
            .pr(properties::ACCESS_ROW_END)
            .min_w_0()
            .child(icon(Icon::Person, size::PLACE_ICON, colors.text_dim))
            .child(div().flex_none().text_color(colors.text).child(self.name))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(text::DETAIL)
                    .font_weight(text::DETAIL_WEIGHT)
                    .text_color(colors.text_dim)
                    .child(self.role),
            )
            .child(self.chip)
            .into_any_element()
    }
}

/// The row for `who`, the `index`th: who they are, and their level on a
/// chip that opens its menu when the user `may` change it.
fn access_row(
    index: usize,
    who: Who,
    read: &[Arc<Properties>],
    may: bool,
    cx: &Context<MainWindow>,
) -> RowParts {
    let colors = Colors::of(cx);
    let (name, role) = names(who, read);
    let level = crate::window::shared_level(read, who)
        .map_or(MIXED_LABEL, crate::window::AccessLevel::label);
    let chip = div()
        .id(("access", index))
        .debug_selector(move || format!("access-{index}"))
        .flex()
        .flex_none()
        .items_center()
        .gap(properties::CHIP_GAP)
        .h(size::CHIP_HEIGHT)
        .px(size::CHIP_PAD)
        .rounded(radius::ROW)
        .text_color(colors.text_soft)
        .when(may, |chip| {
            chip.cursor_pointer()
                .hover(|chip| chip.bg(colors.hover).text_color(colors.text))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        this.open_access_menu(who, event.position, cx);
                    }),
                )
        })
        .child(level)
        .child(
            svg()
                .path(Icon::Chevron.path())
                .flex_none()
                .size(properties::CHIP_ARROW)
                .text_color(gpui::Hsla::from(colors.text_faint))
                .with_transformation(Transformation::rotate(radians(FRAC_PI_2))),
        );
    RowParts { name, role, chip }
}

/// A row's name and the dim word after it: the account and its role
/// when every item shares it, else the role alone.
fn names(who: Who, read: &[Arc<Properties>]) -> (String, &'static str) {
    let account = |one: &Properties| match who {
        Who::Owner => Some(one.owner.clone()),
        Who::Group => Some(one.group.clone()),
        Who::Others => None,
    };
    let role = match who {
        Who::Owner => OWNER_LABEL,
        Who::Group => GROUP_LABEL,
        Who::Others => return (OTHERS_LABEL.to_owned(), ""),
    };
    let mut accounts = read.iter().map(|one| account(one));
    let first = accounts.next().flatten();
    match first {
        Some(first) if accounts.all(|other| other.as_ref() == Some(&first)) => {
            (account_name(&first), role)
        }
        _ => (role.to_owned(), ""),
    }
}

/// An account's name, or its number when the system has no name for it.
fn account_name(account: &Account) -> String {
    account.name.as_ref().map_or_else(
        || account.id.to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// A tick row: its box and its label.
struct Tick {
    selector: &'static str,
    label: &'static str,
    ticked: bool,
    /// Whether the user may change it.
    may: bool,
}

impl Tick {
    /// The row, which `toggle` flips when the user may change it.
    fn row(
        self,
        toggle: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
        colors: &Colors,
    ) -> AnyElement {
        let selector = self.selector;
        div()
            .id(selector)
            .debug_selector(move || selector.to_owned())
            .flex()
            .items_center()
            .gap(size::TICK_GAP)
            .h(size::ROW_HEIGHT)
            .px(size::ROW_PAD)
            .rounded(radius::ROW)
            .text_color(colors.text_soft)
            .when(self.may, |row| {
                row.cursor_pointer()
                    .hover(|row| row.bg(colors.hover))
                    .on_click(toggle)
            })
            .child(tick_box(self.ticked, colors))
            .child(self.label)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::*;

    fn owned_by(owner: Option<&str>, id: u32) -> Arc<Properties> {
        let account = Account {
            id,
            name: owner.map(OsString::from),
        };
        Arc::new(Properties {
            path: PathBuf::from("/a"),
            kind: PathKind::File,
            bytes: 0,
            on_disk: 0,
            modified: None,
            accessed: None,
            changed: None,
            created: None,
            mode: 0o644,
            owner: account.clone(),
            group: account,
            may_change: true,
            groups: Vec::new(),
            read_only: None,
        })
    }

    #[test]
    fn a_row_names_the_account_its_items_share_else_only_the_role() {
        let luna = owned_by(Some("luna"), 1000);
        assert_eq!(
            names(Who::Owner, std::slice::from_ref(&luna)),
            ("luna".to_owned(), OWNER_LABEL)
        );
        assert_eq!(
            names(Who::Group, std::slice::from_ref(&luna)),
            ("luna".to_owned(), GROUP_LABEL)
        );
        assert_eq!(
            names(Who::Others, std::slice::from_ref(&luna)),
            (OTHERS_LABEL.to_owned(), "")
        );
        let nameless = owned_by(None, 1234);
        assert_eq!(names(Who::Owner, std::slice::from_ref(&nameless)).0, "1234");
        assert_eq!(
            names(Who::Owner, &[luna, nameless]),
            (OWNER_LABEL.to_owned(), "")
        );
    }
}
