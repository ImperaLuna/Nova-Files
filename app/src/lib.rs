#![forbid(unsafe_code)]

use nova_files_shell::Parts;
use nova_files_trash_ui::TrashPart;

#[must_use]
pub fn parts() -> Parts {
    let mut parts = Parts::default();
    parts.add::<TrashPart>();
    parts
}
