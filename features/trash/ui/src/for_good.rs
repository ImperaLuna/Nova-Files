use std::path::PathBuf;

use gpui::{App, Context, Window};
use nova_files_shell::actions::{DeleteSelected, TrashSelected};
use nova_files_shell::assets::Icon;
use nova_files_shell::{Location, Look, MenuItem, Scene, command_spec};
use nova_files_trash_api::delete_from_trash;

use crate::TrashPart;

mod actions {
    #![expect(
        clippy::derive_partial_eq_without_eq,
        reason = "GPUI's actions! macro derives PartialEq only"
    )]
    gpui::actions!(nova_files_trash, [DeleteForGood]);
}

pub use actions::DeleteForGood;

command_spec!(
    DELETE_FOR_GOOD,
    DeleteForGood,
    "file.delete-for-good",
    "Delete for good",
    []
);

impl TrashPart {
    fn under(&self, scene: &Scene) -> bool {
        let here = scene.location.as_ref().and_then(Location::folder);
        self.folder()
            .is_some_and(|folder| here.is_some_and(|here| here.starts_with(folder)))
    }

    fn for_good(&self, scene: &Scene, cx: &App) -> Option<Vec<PathBuf>> {
        let items = self.host.entries.read(cx).selected_paths();
        (self.shown(scene) && !items.is_empty()).then_some(items)
    }

    pub fn delete_for_good(&mut self, _: &DeleteForGood, _: &mut Window, cx: &mut Context<Self>) {
        let scene = self.host.scene(cx);
        let Some(items) = self.for_good(&scene, cx) else {
            return;
        };
        let job = delete_from_trash(items);
        self.host
            .island
            .update(cx, |island, cx| island.start_job(job, cx));
    }

    fn key_under(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let under = self.under(&self.host.scene(cx));
        if under {
            self.delete_for_good(&DeleteForGood, window, cx);
        }
        under
    }

    pub fn trash_key(
        &mut self,
        _: &TrashSelected,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.key_under(window, cx)
    }

    pub fn delete_key(
        &mut self,
        _: &DeleteSelected,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.key_under(window, cx)
    }
}

pub fn for_good_items(part: &TrashPart, scene: &Scene, cx: &App) -> Vec<MenuItem> {
    if part.for_good(scene, cx).is_none() {
        return Vec::new();
    }
    vec![MenuItem::run(Icon::Trash, &DELETE_FOR_GOOD).with_look(Look::Danger)]
}
