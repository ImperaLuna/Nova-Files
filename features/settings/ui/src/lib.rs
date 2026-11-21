#![forbid(unsafe_code)]

use std::sync::Arc;

use gpui::{App, Context, Window};
use nova_files_api::{FolderSizes, HiddenFiles};
use nova_files_settings_api::{self as settings_api, SettingChange, Thumbnails};
use nova_files_shell::actions::{ToggleFolderSizes, ToggleHidden};
use nova_files_shell::assets::Icon;
use nova_files_shell::{
    CommandSpec, Host, Location, Look, MenuAt, MenuGroup, MenuItem, Part, Placed, Published, Scene,
    Slots, ThumbnailsShown, command_spec,
};

command_spec!(
    HIDDEN,
    ToggleHidden,
    "view.hidden",
    "Show hidden files",
    ["ctrl-h"]
);

command_spec!(
    FOLDER_SIZES,
    ToggleFolderSizes,
    "view.folder-sizes",
    "Folder sizes",
    []
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
        Published::set(settings.folder_sizes, cx);
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

    fn toggle_folder_sizes(
        &mut self,
        _: &ToggleFolderSizes,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sizes = Published::<FolderSizes>::get(cx)
            .copied()
            .unwrap_or_default()
            .toggled();
        self.host
            .link
            .send(settings_api::Command::Change(SettingChange::FolderSizes(
                sizes,
            )))
            .ok();
    }

    #[expect(
        clippy::unused_self,
        reason = "a menu slot is a method; the settings live in globals"
    )]
    fn show_items(&self, scene: &Scene, cx: &App) -> Vec<MenuItem> {
        let in_folder = scene.location.as_ref().and_then(Location::folder).is_some();
        if scene.rules.replaces_menu || !in_folder {
            return Vec::new();
        }
        let hidden = Published::<HiddenFiles>::get(cx) == Some(&HiddenFiles::Shown);
        let measured = Published::<FolderSizes>::get(cx) == Some(&FolderSizes::Measured);
        vec![
            tick(hidden, Icon::Eye, &HIDDEN),
            tick(measured, Icon::Drive, &FOLDER_SIZES),
        ]
    }
}

fn tick(on: bool, off_icon: Icon, spec: &'static CommandSpec) -> MenuItem {
    if on {
        MenuItem::run(Icon::Check, spec).with_look(Look::On)
    } else {
        MenuItem::run(off_icon, spec)
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
        slots.command(&FOLDER_SIZES, always, Self::toggle_folder_sizes);
        slots.menu_in(
            MenuAt::Sheet,
            MenuGroup::Show,
            Placed::FIRST,
            Self::show_items,
        );
    }
}
