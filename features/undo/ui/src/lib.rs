#![forbid(unsafe_code)]

use gpui::{App, Context, Window};
use nova_files_api::Operation;
use nova_files_shell::{Host, Island, Part, Scene, Slots, command_spec};

pub use nova_files_shell::actions::Undo;

command_spec!(UNDO, Undo, "edit.undo", "Undo", ["ctrl-z"]);

pub struct UndoPart {
    host: Host,
}

impl UndoPart {
    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        let island = self.host.island.clone();
        match island.update(cx, Island::take_undo) {
            Some(nova_files_shell::Undo::Job(job)) => {
                island.update(cx, |island, cx| {
                    island.start_job(Operation::Undo { job }, cx);
                });
            }
            Some(nova_files_shell::Undo::Run(run)) => cx.defer(move |cx| run(cx)),
            None => {}
        }
    }
}

const fn anywhere(_: &UndoPart, _: &Scene, _: &App) -> bool {
    true
}

impl Part for UndoPart {
    fn open(host: &Host, _: &mut Context<Self>) -> Self {
        Self { host: host.clone() }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.command(&UNDO, anywhere, Self::undo);
    }
}
