use gpui::{Context, Pixels, Point};
use nova_files_api::{App, Command, OpenWith, OpenWithAnswer, OpenWithId};

use super::MainWindow;
use crate::assets::Icon;
use crate::menu::{Look, MenuAction, MenuItem};

#[derive(Debug, Default)]
pub struct OpensWith {
    asked: Option<OpenWithId>,
    mime: Option<String>,
    apps: Vec<App>,
}

impl MainWindow {
    pub(super) fn ask_apps(&mut self) {
        let id = OpenWithId(self.next_ask());
        let Some(page) = self.properties.as_mut() else {
            return;
        };
        let paths = page.paths();
        if paths.len() != 1 {
            return;
        }
        let sent = self.connection.as_ref().is_some_and(|connection| {
            connection
                .send(Command::OpenWith(OpenWith::Apps { id, paths }))
                .is_ok()
        });
        page.opens_with.asked = sent.then_some(id);
    }

    pub(super) fn open_with_answered(&mut self, answer: OpenWithAnswer, cx: &mut Context<Self>) {
        let OpenWithAnswer::Apps { id, mime, apps } = answer else {
            return;
        };
        let Some(opens_with) = self
            .properties
            .as_mut()
            .map(|page| &mut page.opens_with)
            .filter(|opens_with| opens_with.asked == Some(id))
        else {
            return;
        };
        opens_with.asked = None;
        opens_with.mime = mime;
        opens_with.apps = apps;
        cx.notify();
    }

    pub fn apps_to_open_with(&self) -> &[App] {
        self.properties_page()
            .map_or(&[], |page| page.opens_with.apps.as_slice())
    }

    pub fn default_app(&self) -> Option<&App> {
        self.apps_to_open_with().iter().find(|app| app.default)
    }

    pub(crate) fn open_apps_menu(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(opens_with) = self.properties_page().map(|page| &page.opens_with) else {
            return;
        };
        let Some(mime) = opens_with.mime.clone() else {
            return;
        };
        let items = opens_with
            .apps
            .iter()
            .map(|app| {
                let choice = MenuAction::SetDefaultApp {
                    mime: mime.clone(),
                    app: app.id.clone(),
                };
                if app.default {
                    MenuItem::new(Icon::Check, app.name.clone(), choice).with_look(Look::On)
                } else {
                    MenuItem::new(Icon::Open, app.name.clone(), choice)
                }
            })
            .collect();
        self.open_menu(at, items, cx);
    }

    pub(crate) fn set_default_app(&mut self, mime: String, app: String) {
        if let Some(connection) = &self.connection {
            connection
                .send(Command::OpenWith(OpenWith::SetDefault { mime, app }))
                .ok();
        }
        self.ask_apps();
    }
}
