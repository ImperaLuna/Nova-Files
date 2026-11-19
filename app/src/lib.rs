#![forbid(unsafe_code)]

use nova_files_clipboard_ui::ClipboardPart;
use nova_files_delete_ui::DeletePart;
use nova_files_look_ui::LookPart;
use nova_files_picker_ui::PickerPart;
use nova_files_settings_ui::SettingsPart;
use nova_files_shell::Parts;
use nova_files_trash_ui::TrashPart;

#[must_use]
pub fn parts() -> Parts {
    let mut parts = Parts::default();
    parts.add::<ClipboardPart>();
    parts.add::<DeletePart>();
    parts.add::<LookPart>();
    parts.add::<PickerPart>();
    parts.add::<SettingsPart>();
    parts.add::<TrashPart>();
    parts
}
