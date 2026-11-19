use std::path::Path;

use gpui::{AnyElement, Context, Div, IntoElement, ParentElement as _, Stateful, Styled as _, div};
use nova_files_api::EntryKind;
use nova_files_shell::path_bar::{self, BarInput, Crumb, Spot};
use nova_files_shell::{MenuAt, TextField, arrange};

use crate::theme::Colors;
use crate::window::{Aim, MainWindow};
use nova_files_shell::Location;

impl MainWindow {
    pub(super) fn path_bar(&self, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
        let input = BarInput {
            on_screen: self.folder().map(Path::to_path_buf),
            asks: self.asks_sent,
            crumbs: self.crumbs(cx),
            arrow_here: self.has_folders() && self.named_here().is_none(),
            chips: self.path_chips(colors, cx),
        };
        path_bar::draw(
            &self.path_bar,
            input,
            &|button, crumb| self.crumb_drops(button, crumb, cx),
            cx,
        )
    }

    pub fn path_menu_names(&self, cx: &gpui::App) -> Option<Vec<String>> {
        self.path_bar.read(cx).menu_names(self.spot())
    }

    pub fn path_field(&self, cx: &gpui::App) -> Option<gpui::Entity<TextField>> {
        self.path_bar.read(cx).field().cloned()
    }

    fn spot(&self) -> Spot<'_> {
        Spot {
            on_screen: self.folder(),
            asks: self.asks_sent,
        }
    }

    fn crumb_drops(
        &self,
        button: Stateful<Div>,
        crumb: &Crumb,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let Some(folder) = crumb.folder() else {
            return button;
        };
        let place = Location::Folder(folder.to_path_buf());
        let at = self
            .tab_drops_at(Some(&place), crumb.label.clone())
            .unwrap_or_else(|| Aim::folder(folder.to_path_buf(), crumb.label.clone()));
        self.drop_target(button, at, cx)
    }

    fn path_chips(&self, colors: &Colors, cx: &Context<Self>) -> Vec<AnyElement> {
        let mut chips: Vec<AnyElement> = self.scope_chip(colors, cx).into_iter().collect();
        let drops_here = match self.location() {
            Some(Location::Page(page)) if self.parts.takes_drops(page) => {
                Some(Aim::page(page.clone()))
            }
            _ => None,
        };
        chips.extend(self.parts.path_chips().into_iter().map(|chip| {
            match drops_here.clone() {
                Some(aim) => self
                    .drop_target(div().flex_none().child(chip), aim, cx)
                    .into_any_element(),
                None => chip.into_any_element(),
            }
        }));
        let items = self.parts.menu(MenuAt::PathBarEnd, &self.scene(cx), cx);
        chips.extend(
            arrange(items)
                .into_iter()
                .flatten()
                .map(|item| path_bar::menu_chip(item, colors).into_any_element()),
        );
        chips
    }

    fn crumbs(&self, cx: &gpui::App) -> Vec<Crumb> {
        if let Some(folder) = self.folder() {
            return self.named_here().map_or_else(
                || path_bar::crumbs(folder, &self.home, self.known_folders()),
                |named| path_bar::named(folder, named),
            );
        }
        match self.location() {
            Some(Location::Page(page)) => self
                .parts
                .crumbs(page, cx)
                .unwrap_or_else(|| path_bar::page_crumbs(page, &self.home, self.known_folders())),
            _ => Vec::new(),
        }
    }

    pub(crate) fn has_folders(&self) -> bool {
        if self.searching() {
            return self.search.from_has_folders;
        }
        self.listing
            .as_ref()
            .and_then(|listing| listing.entries.first())
            .is_some_and(|entry| entry.kind == EntryKind::Folder)
    }
}
