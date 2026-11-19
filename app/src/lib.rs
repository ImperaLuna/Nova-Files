#![forbid(unsafe_code)]

use nova_files_clipboard_ui::ClipboardPart;
use nova_files_delete_ui::DeletePart;
use nova_files_drives_ui::DrivesPart;
use nova_files_look_ui::LookPart;
use nova_files_open_ui::OpenPart;
use nova_files_picker_ui::PickerPart;
use nova_files_places_ui::{PlacesPart, TreePart};
use nova_files_settings_ui::SettingsPart;
use nova_files_shell::Parts;
use nova_files_thumbnails_ui::ThumbnailsPart;
use nova_files_transfer_ui::TransferPart;
use nova_files_trash_ui::TrashPart;

#[must_use]
pub fn parts() -> Parts {
    let mut parts = Parts::default();
    parts.add::<ClipboardPart>();
    parts.add::<DeletePart>();
    parts.add::<DrivesPart>();
    parts.add::<LookPart>();
    parts.add::<OpenPart>();
    parts.add::<PickerPart>();
    parts.add::<PlacesPart>();
    parts.add::<SettingsPart>();
    parts.add::<ThumbnailsPart>();
    parts.add::<TransferPart>();
    parts.add::<TrashPart>();
    parts.add::<TreePart>();
    parts
}
