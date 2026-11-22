#![forbid(unsafe_code)]

mod words;

use gpui::{App, Context, Window};
use nova_files_api::Operation;
use nova_files_delete_api::{DeleteJob, delete};
use nova_files_shell::actions::DeleteSelected;
use nova_files_shell::{Host, Part, Scene, Slots, command_spec};

use crate::words::DeleteWords;

command_spec!(
    DELETE,
    DeleteSelected,
    "file.delete",
    "Delete for good",
    ["shift-delete"],
    needs[Change]
);

const DELETE_LABEL: &str = "Delete";
const ITEM: &str = "item";
const ITEMS: &str = "items";

pub struct DeletePart {
    host: Host,
}

impl DeletePart {
    fn deleting(&self, cx: &App) -> Option<Operation> {
        let paths = self.host.entries.read(cx).selected_paths();
        (!paths.is_empty()).then(|| delete(paths))
    }

    fn ask_delete(&mut self, _: &DeleteSelected, _: &mut Window, cx: &mut Context<Self>) {
        let Some(operation) = self.deleting(cx) else {
            return;
        };
        let Some(many) = operation.get::<DeleteJob>().map(|job| job.paths.len()) else {
            return;
        };
        let part = cx.entity().downgrade();
        let on_yes = Box::new(move |cx: &mut App| {
            part.update(cx, |part, cx| part.delete_if_still(operation, cx))
                .ok();
        });
        self.host.island.update(cx, |island, cx| {
            island.ask(
                format!("Delete {} for good?", count_of(many)),
                DELETE_LABEL,
                on_yes,
                cx,
            );
        });
    }

    fn delete_if_still(&self, operation: Operation, cx: &mut Context<Self>) {
        if self.deleting(cx).as_ref() == Some(&operation) {
            self.host
                .island
                .update(cx, |island, cx| island.start_job(operation, cx));
        }
    }
}

fn count_of(number: usize) -> String {
    if number == 1 {
        format!("1 {ITEM}")
    } else {
        format!("{number} {ITEMS}")
    }
}

const fn anywhere(_: &DeletePart, _: &Scene, _: &App) -> bool {
    true
}

impl Part for DeletePart {
    fn open(host: &Host, _: &mut Context<Self>) -> Self {
        Self { host: host.clone() }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.command(&DELETE, anywhere, Self::ask_delete);
        slots.job_words::<DeleteJob>(DeleteWords);
    }
}
