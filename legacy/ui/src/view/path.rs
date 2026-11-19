use std::path::Path;

use gpui::{AnyElement, Context, Div, IntoElement, ParentElement as _, Stateful, Styled as _, div};
use nova_files_api::EntryKind;
use nova_files_gathered_ui::GatheredPage;
use nova_files_properties_ui::{PropertiesPage, PropertiesPart};
use nova_files_shell::path_bar::{self, BarInput, Crumb, Spot};
use nova_files_shell::{self as shell, ChipRank, MenuAt, TextField, arrange};

use crate::location::Location;
use crate::theme::Colors;
use crate::window::{Aim, MainWindow};

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
        let mut chips: Vec<AnyElement> = Vec::new();
        let drops_here = match self.location() {
            Some(Location::Page(page)) if self.parts.takes_drops(page) => {
                Some(Aim::page(page.clone()))
            }
            _ => None,
        };
        for (rank, chip) in self.parts.path_chips() {
            let chip = match drops_here.clone() {
                Some(aim) if rank != ChipRank::FIRST => self
                    .drop_target(div().flex_none().child(chip), aim, cx)
                    .into_any_element(),
                _ => chip.into_any_element(),
            };
            chips.push(chip);
        }
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
            Some(Location::Page(page)) if let Some(crumbs) = self.parts.crumbs(page, cx) => crumbs,
            Some(Location::Page(page)) if page.get::<GatheredPage>().is_some() => {
                vec![Crumb {
                    opens: Some(shell::Location::Page(page.clone())),
                    label: page.title(),
                    icon: page.icon(),
                    shows_icon: true,
                    here: true,
                }]
            }
            Some(Location::Page(page)) => page.get::<PropertiesPage>().map_or_else(
                || path_bar::page_crumbs(page, &self.home, self.known_folders()),
                |shown| self.properties_crumbs(shown, cx),
            ),
            _ => Vec::new(),
        }
    }

    fn properties_crumbs(&self, page: &PropertiesPage, cx: &gpui::App) -> Vec<Crumb> {
        let shown = match page.paths() {
            [one] => one.as_path(),
            _ => match page.folder() {
                Some(folder) => folder,
                None => return Vec::new(),
            },
        };
        let mut crumbs = path_bar::crumbs(shown, &self.home, self.known_folders());
        if let Some(last) = crumbs.last_mut() {
            let part = self.part::<PropertiesPart>();
            if let Some(icon) = part.and_then(|part| part.read(cx).icon()) {
                last.icon = icon;
            }
            last.opens = None;
        }
        crumbs
    }

    pub(crate) fn has_folders(&self) -> bool {
        self.listing
            .as_ref()
            .and_then(|listing| listing.entries.first())
            .is_some_and(|entry| entry.kind == EntryKind::Folder)
    }
}
