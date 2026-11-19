#![forbid(unsafe_code)]

use nova_files_clipboard_ui::ClipboardPart;
use nova_files_delete_ui::DeletePart;
use nova_files_drives_ui::DrivesPart;
use nova_files_gathered_ui::GatheredPart;
use nova_files_look_ui::LookPart;
use nova_files_open_ui::OpenPart;
use nova_files_picker_ui::PickerPart;
use nova_files_places_ui::{PlacesPart, TreePart};
use nova_files_properties_ui::PropertiesPart;
use nova_files_rename_ui::RenamePart;
use nova_files_settings_ui::SettingsPart;
use nova_files_shell::Parts;
use nova_files_start_ui::StartPart;
use nova_files_thumbnails_ui::ThumbnailsPart;
use nova_files_transfer_ui::TransferPart;
use nova_files_trash_ui::TrashPart;
use nova_files_undo_ui::UndoPart;

#[must_use]
pub fn parts() -> Parts {
    let mut parts = Parts::default();
    parts.add::<ClipboardPart>();
    parts.add::<DeletePart>();
    parts.add::<DrivesPart>();
    parts.add::<GatheredPart>();
    parts.add::<LookPart>();
    parts.add::<OpenPart>();
    parts.add::<PickerPart>();
    parts.add::<PlacesPart>();
    parts.add::<PropertiesPart>();
    parts.add::<RenamePart>();
    parts.add::<SettingsPart>();
    parts.add::<StartPart>();
    parts.add::<ThumbnailsPart>();
    parts.add::<TransferPart>();
    parts.add::<TrashPart>();
    parts.add::<TreePart>();
    parts.add::<UndoPart>();
    parts
}
