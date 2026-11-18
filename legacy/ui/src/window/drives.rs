use std::collections::HashMap;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Context, Pixels, Point};
use nova_files_drives_api::{
    Command, DiskSpace, DriveAction, DriveFailure, DriveId, DriveList, Event, VolumeId,
};

use super::MainWindow;
use crate::assets::Icon;
use crate::drives::{self, ShownDrive};
use crate::menu::{MenuAction, MenuItem};

const OPEN_LABEL: &str = "Open";
const EJECT_LABEL: &str = "Eject";

#[derive(Debug, Default)]
pub struct Drives {
    pub list: Arc<DriveList>,
    pub space: HashMap<PathBuf, DiskSpace>,
    opening: Option<VolumeId>,
    asking: bool,
    ask_again: bool,
}

impl MainWindow {
    pub fn shown_drives(&self) -> Vec<ShownDrive> {
        drives::shown(&self.drives.list)
    }

    pub fn disk_space(&self, folder: &Path) -> Option<DiskSpace> {
        self.drives.space.get(folder).copied()
    }

    pub(super) fn drive_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Drives(list) => {
                self.leave_gone_drive(&list);
                self.drives.list = list;
                let shown: Vec<PathBuf> = self
                    .shown_drives()
                    .into_iter()
                    .filter_map(|drive| drive.folder)
                    .collect();
                self.drives.space.retain(|folder, _| shown.contains(folder));
                self.ask_disk_space();
            }
            Event::DiskSpace(spaces) => {
                for answer in spaces {
                    match answer.space {
                        Some(space) => self.drives.space.insert(answer.path, space),
                        None => self.drives.space.remove(&answer.path),
                    };
                }
                self.drives.asking = false;
                if std::mem::take(&mut self.drives.ask_again) {
                    self.ask_disk_space();
                }
            }
            Event::DriveDone { action, result } => self.drive_done(&action, result),
        }
        cx.notify();
    }

    fn drive_done(&mut self, action: &DriveAction, result: Result<Option<PathBuf>, DriveFailure>) {
        let opening = match action {
            DriveAction::Mount(volume) => self.drives.opening.as_ref() == Some(volume),
            DriveAction::Unmount(_) | DriveAction::Eject(_) => false,
        };
        if opening {
            self.drives.opening = None;
        }
        match result {
            Ok(Some(folder)) if opening => self.open(&folder),
            Ok(_) => {}
            Err(failure) => {
                writeln!(io::stderr(), "nova-files: {action:?} failed: {failure:?}").ok();
            }
        }
    }

    pub(super) fn ask_disk_space(&mut self) {
        if self.drives.asking {
            self.drives.ask_again = true;
            return;
        }
        let Some(connection) = &self.connection else {
            return;
        };
        let folders = self
            .shown_drives()
            .into_iter()
            .filter_map(|drive| drive.folder)
            .collect();
        self.drives.asking = connection.send(Command::DiskSpace(folders)).is_ok();
    }

    fn leave_gone_drive(&mut self, next: &DriveList) {
        let Some(here) = self.folder().map(Path::to_owned) else {
            return;
        };
        let mounted = |list: &DriveList| -> Vec<PathBuf> {
            list.volumes
                .iter()
                .filter_map(|volume| volume.mount_point.clone())
                .collect()
        };
        let still = mounted(next);
        let gone = mounted(&self.drives.list)
            .into_iter()
            .any(|folder| here.starts_with(&folder) && !still.contains(&folder));
        if gone {
            let home = self.home.clone();
            self.open(&home);
        }
    }

    pub(super) fn forget_drive_to_open(&mut self) {
        self.drives.opening = None;
    }

    pub fn open_drive(&mut self, drive: ShownDrive) {
        match (drive.folder, drive.volume) {
            (Some(folder), _) => self.open(&folder),
            (None, Some(volume)) if self.drives.opening.as_ref() == Some(&volume) => {}
            (None, Some(volume)) => {
                self.drives.opening = Some(volume.clone());
                self.act_on_drive(DriveAction::Mount(volume));
            }
            (None, None) => {}
        }
    }

    pub(crate) fn act_on_drive(&self, action: DriveAction) {
        if let Some(connection) = &self.connection {
            connection.send(Command::Drive(action)).ok();
        }
    }

    pub(crate) fn open_drive_menu(
        &mut self,
        drive: ShownDrive,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let eject = drive.eject.clone();
        let opens = vec![
            MenuItem::new(Icon::Open, OPEN_LABEL, MenuAction::OpenDrive(drive.clone())),
            MenuItem::new(
                Icon::Plus,
                super::tabs::OPEN_IN_NEW_TAB_LABEL,
                MenuAction::OpenDriveInNewTab(drive),
            ),
        ];
        let eject =
            eject.map(|id: DriveId| MenuItem::new(Icon::Eject, EJECT_LABEL, MenuAction::Eject(id)));
        self.open_menu_groups(at, vec![opens, eject.into_iter().collect()], false, cx);
    }
}
