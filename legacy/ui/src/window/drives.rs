use std::path::Path;

use gpui::Context;
use nova_files_drives_api::DiskSpace;
use nova_files_drives_ui::{self as drives, DrivesPart, KnownDrives, ShownDrive};
use nova_files_shell::Published;

use super::{MainWindow, Went};

impl MainWindow {
    pub fn shown_drives(&self) -> Vec<ShownDrive> {
        drives::shown(&self.drives.list)
    }

    pub fn disk_space(&self, folder: &Path) -> Option<DiskSpace> {
        self.drives.space.get(folder).copied()
    }

    pub(super) fn drives_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(known) = Published::<KnownDrives>::get(cx) {
            self.drives.clone_from(known);
        }
        cx.notify();
    }

    pub(super) const fn went_elsewhere(&mut self) {
        self.went = Went::Elsewhere;
    }

    pub fn open_drive(&mut self, drive: ShownDrive, cx: &mut Context<Self>) {
        if let Some(part) = self.part::<DrivesPart>() {
            part.update(cx, |part, cx| part.open_drive(drive, cx));
        }
    }
}
