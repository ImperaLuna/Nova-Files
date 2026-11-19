#![forbid(unsafe_code)]

use std::sync::Arc;

use gpui::Context;
use nova_files_settings_api::{self as settings_api, Settings, Thumbnails};
use nova_files_shell::{Host, Part, Published, Slots, ThumbnailsShown};

pub struct SettingsPart;

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
}

impl Part for SettingsPart {
    fn open(host: &Host, _: &mut Context<Self>) -> Self {
        host.link.send(settings_api::Command::Watch).ok();
        Self
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(Self::settings_arrived);
        slots.sends::<settings_api::Command>();
    }
}
