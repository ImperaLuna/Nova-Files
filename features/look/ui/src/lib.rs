#![forbid(unsafe_code)]

use gpui::Context;
use nova_files_look_api as look_api;
use nova_files_shell::theme::Colors;
use nova_files_shell::{Host, Part, Slots};

pub struct LookPart;

impl LookPart {
    #[expect(
        clippy::unused_self,
        reason = "an event slot is a method; the colors live in a global"
    )]
    fn theme_arrived(&mut self, event: look_api::Event, cx: &mut Context<Self>) {
        let look_api::Event::Theme(theme) = event;
        let colors = Colors::new(&theme);
        if Colors::of(cx) != colors {
            cx.set_global(colors);
            cx.refresh_windows();
            cx.notify();
        }
    }
}

impl Part for LookPart {
    fn open(host: &Host, _: &mut Context<Self>) -> Self {
        host.link.send(look_api::Command::Watch).ok();
        Self
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(Self::theme_arrived);
        slots.sends::<look_api::Command>();
    }
}
