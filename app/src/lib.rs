#![forbid(unsafe_code)]

mod backend;
mod trace;
mod vulkan_drivers;

use nova_files_clipboard_ui::ClipboardPart;
use nova_files_delete_ui::DeletePart;
use nova_files_drives_ui::DrivesPart;
use nova_files_look_ui::LookPart;
use nova_files_open_ui::OpenPart;
use nova_files_picker_ui::PickerPart;
use nova_files_places_ui::{PlacesPart, TreePart};
use nova_files_preview_ui::PreviewPart;
use nova_files_properties_ui::PropertiesPart;
use nova_files_rename_ui::RenamePart;
use nova_files_settings_ui::SettingsPart;
use nova_files_shell::Parts;
use nova_files_start_ui::StartPart;
use nova_files_thumbnails_ui::ThumbnailsPart;
use nova_files_transfer_ui::TransferPart;
use nova_files_trash_ui::TrashPart;
use nova_files_undo_ui::UndoPart;

pub use backend::{
    ArchiveEntry, ArchiveFailure, ArchiveKind, Archiver, Extracting, Flow, Home, IDLE_EXIT,
    IndexError, IndexPlace, MimeTypes, NotDefault, Recipe, RunningCore, Thumbnails, Unusable,
    emitted_types, launch_without, make_default_for_folders, opens_folders, routed_types,
    set_give_back, seven_on, spawn, spawn_with, spawn_with_archiver, spawn_with_idle_exit,
};
pub use trace::{FRAME_TRACE_VARIABLE, STARTUP_TRACE_VARIABLE, TraceError, start_traces};
pub use vulkan_drivers::{FILTER_VARIABLE, absent_vendor_drivers};

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
    parts.add::<PreviewPart>();
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
