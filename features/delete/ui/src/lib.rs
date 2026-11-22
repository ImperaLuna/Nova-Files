#![forbid(unsafe_code)]

mod words;

use gpui::{App, Context, Window};
use nova_files_api::Operation;
use nova_files_delete_api::{DeleteJob, delete};
use nova_files_shell::actions::TrashSelected;
use nova_files_shell::assets::Icon;
use nova_files_shell::{
    Host, Location, Look, MenuAt, MenuGroup, MenuItem, Part, Placed, Scene, Slots, command_spec,
};
use nova_files_trash_api::{DeleteFromTrashJob, delete_from_trash};

use crate::words::DeleteWords;

mod actions {
    #![expect(
        clippy::derive_partial_eq_without_eq,
        reason = "GPUI's actions! macro derives PartialEq only"
    )]
    gpui::actions!(nova_files_delete, [DeleteSelected]);
}

pub use actions::DeleteSelected;

command_spec!(
    DELETE,
    DeleteSelected,
    "file.delete",
    "Delete for good",
    ["shift-delete"]
);

const DELETE_LABEL: &str = "Delete";
const ITEM: &str = "item";
const ITEMS: &str = "items";

pub struct DeletePart {
    host: Host,
}

impl DeletePart {
    fn deleting(&self, scene: &Scene, cx: &App) -> Option<Operation> {
        let paths = self.host.entries.read(cx).selected_paths();
        if paths.is_empty() {
            return None;
        }
        if !scene.rules.trashed {
            return Some(delete(paths));
        }
        let folder = scene.location.as_ref().and_then(Location::folder)?;
        self.host
            .rules
            .named(folder)
            .map(|_| delete_from_trash(paths))
    }

    fn ask_delete(&mut self, _: &DeleteSelected, _: &mut Window, cx: &mut Context<Self>) {
        let scene = self.host.scene(cx);
        let Some(operation) = self.deleting(&scene, cx) else {
            return;
        };
        let Some(many) = how_many(&operation) else {
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

    fn trash_in_the_trash(
        &mut self,
        _: &TrashSelected,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let trashed = self.host.scene(cx).rules.trashed;
        if trashed {
            self.ask_delete(&DeleteSelected, window, cx);
        }
        trashed
    }

    fn delete_if_still(&self, operation: Operation, cx: &mut Context<Self>) {
        let scene = self.host.scene(cx);
        if self.deleting(&scene, cx).as_ref() == Some(&operation) {
            self.host
                .island
                .update(cx, |island, cx| island.start_job(operation, cx));
        }
    }
}

fn how_many(operation: &Operation) -> Option<usize> {
    operation
        .get::<DeleteJob>()
        .map(|job| job.paths.len())
        .or_else(|| {
            operation
                .get::<DeleteFromTrashJob>()
                .map(|job| job.items.len())
        })
}

fn count_of(number: usize) -> String {
    if number == 1 {
        format!("1 {ITEM}")
    } else {
        format!("{number} {ITEMS}")
    }
}

fn delete_items(part: &DeletePart, scene: &Scene, cx: &App) -> Vec<MenuItem> {
    if !scene.rules.trashed || part.deleting(scene, cx).is_none() {
        return Vec::new();
    }
    vec![MenuItem::run(Icon::Trash, &DELETE).with_look(Look::Danger)]
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
        slots.before(Self::trash_in_the_trash);
        slots.job_words::<DeleteJob>(DeleteWords);
        slots.menu_in(
            MenuAt::Entry,
            MenuGroup::Danger,
            Placed::FIRST,
            delete_items,
        );
    }
}
