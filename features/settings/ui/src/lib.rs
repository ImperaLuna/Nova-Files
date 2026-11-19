#![forbid(unsafe_code)]

use std::sync::Arc;

use gpui::{App, Context, Window};
use nova_files_api::HiddenFiles;
use nova_files_settings_api::{self as settings_api, SettingChange, Thumbnails};
use nova_files_shell::actions::ToggleHidden;
use nova_files_shell::assets::Icon;
use nova_files_shell::{
    Host, Look, MenuAt, MenuGroup, MenuItem, Part, Placed, Published, Scene, Slots,
    ThumbnailsShown, command_spec,
};

command_spec!(
    HIDDEN,
    ToggleHidden,
    "view.hidden",
    "Show hidden files",
    ["ctrl-h"]
);

pub struct SettingsPart {
    host: Host,
}

impl SettingsPart {
    #[expect(
        clippy::unused_self,
        reason = "an event slot is a method; the settings live in globals"
    )]
    fn settings_arrived(&mut self, event: settings_api::Event, cx: &mut Context<Self>) {
        let settings_api::Event::Settings(settings) = event;
        let shown = match settings.thumbnails {
            Thumbnails::Shown => ThumbnailsShown::Shown,
            Thumbnails::Hidden => ThumbnailsShown::Hidden,
        };
        Published::set(settings.hidden_files, cx);
        Published::set(shown, cx);
        Published::set(Arc::unwrap_or_clone(settings), cx);
        cx.notify();
    }

    fn toggle_hidden(&mut self, _: &ToggleHidden, _: &mut Window, cx: &mut Context<Self>) {
        let hidden = Published::<HiddenFiles>::get(cx)
            .copied()
            .unwrap_or_default()
            .toggled();
        self.host
            .link
            .send(settings_api::Command::Change(SettingChange::HiddenFiles(
                hidden,
            )))
            .ok();
    }

    #[expect(
        clippy::unused_self,
        reason = "a menu slot is a method; the setting lives in a global"
    )]
    fn hidden_item(&self, scene: &Scene, cx: &App) -> Vec<MenuItem> {
        if scene.rules.replaces_menu {
            return Vec::new();
        }
        let item = if Published::<HiddenFiles>::get(cx) == Some(&HiddenFiles::Shown) {
            MenuItem::run(Icon::Check, &HIDDEN).with_look(Look::On)
        } else {
            MenuItem::run(Icon::Eye, &HIDDEN)
        };
        vec![item]
    }
}

const fn always(_: &SettingsPart, _: &Scene, _: &App) -> bool {
    true
}

impl Part for SettingsPart {
    fn open(host: &Host, _: &mut Context<Self>) -> Self {
        host.link.send(settings_api::Command::Watch).ok();
        Self { host: host.clone() }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(Self::settings_arrived);
        slots.sends::<settings_api::Command>();
        slots.command(&HIDDEN, always, Self::toggle_hidden);
        slots.menu_in(
            MenuAt::Sheet,
            MenuGroup::Show,
            Placed::FIRST,
            Self::hidden_item,
        );
    }
}
