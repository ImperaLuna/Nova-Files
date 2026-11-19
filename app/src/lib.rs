#![forbid(unsafe_code)]

use nova_files_look_ui::LookPart;
use nova_files_places_ui::PlacesPart;
use nova_files_shell::Parts;
use nova_files_trash_ui::TrashPart;

#[must_use]
pub fn parts() -> Parts {
    let mut parts = Parts::default();
    parts.add::<LookPart>();
    parts.add::<PlacesPart>();
    parts.add::<TrashPart>();
    parts
}
