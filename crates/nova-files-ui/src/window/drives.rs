//! The drives as the window takes them: the core's list (asked for once,
//! then sent on each change), how full each mounted one is, and what
//! a click or the drive's menu asks for.
//!
//! How full the drives are is asked again whenever the list changes and
//! whenever a folder's listing arrives, so the numbers follow copies and
//! deletes without polling. One ask is out at a time: asks made meanwhile
//! become one more once it answers, so a drive that stopped answering
//! never piles up the core's threads and answers land in order. A drive
//! that goes away takes its number with it. A click on a volume not mounted mounts it,
//! then opens it. A drive that goes away (ejected, unplugged) while the
//! window is in it sends the window home.

use std::collections::HashMap;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Context, Pixels, Point};
use nova_files_api::{
    Command, DiskSpace, DriveAction, DriveFailure, DriveId, DriveList, Event, VolumeId,
};

use super::MainWindow;
use crate::assets::Icon;
use crate::drives::{self, ShownDrive};
use crate::menu::{MenuAction, MenuItem};

const OPEN_LABEL: &str = "Open";
const EJECT_LABEL: &str = "Eject";

/// What the window knows of the drives.
#[derive(Debug, Default)]
pub struct Drives {
    pub list: Arc<DriveList>,
    /// How full each mounted drive is, by its folder.
    pub space: HashMap<PathBuf, DiskSpace>,
    /// The volume being mounted to be opened.
    opening: Option<VolumeId>,
    /// Whether an ask for the space is out, and whether to ask again once
    /// it answers.
    asking: bool,
    ask_again: bool,
}

impl MainWindow {
    /// The rows of the sidebar's Drives section.
    pub fn shown_drives(&self) -> Vec<ShownDrive> {
        drives::shown(&self.drives.list)
    }

    /// How full the drive at `folder` is, once the core has said.
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
            _ => return,
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
            // Until the island shows notes, stderr is where this goes.
            Err(failure) => {
                writeln!(io::stderr(), "nova-files: {action:?} failed: {failure:?}").ok();
            }
        }
    }

    /// Asks how full the system drive and each mounted volume are, or
    /// asks again once the ask that is out answers.
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
        // A core gone ends the window's events, which shows it.
        self.drives.asking = connection.send(Command::DiskSpace(folders)).is_ok();
    }

    /// Sends the window home when the folder on screen was on a volume
    /// `next` no longer has mounted there.
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

    /// Goes to `drive`, mounting it first when it isn't mounted.
    pub fn open_drive(&mut self, drive: ShownDrive) {
        match (drive.folder, drive.volume) {
            (Some(folder), _) => self.open(&folder),
            // Clicked again while it mounts: the first mount opens it.
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
            // A core gone ends the window's events, which shows it.
            connection.send(Command::Drive(action)).ok();
        }
    }

    /// Opens `drive`'s menu at `at`: Open, and Eject for a drive that can
    /// be unplugged.
    pub(crate) fn open_drive_menu(
        &mut self,
        drive: ShownDrive,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let eject = drive.eject.clone();
        let mut items = vec![MenuItem {
            icon: Icon::Open,
            label: OPEN_LABEL.into(),
            action: MenuAction::OpenDrive(drive),
        }];
        items.extend(eject.map(|id: DriveId| MenuItem {
            icon: Icon::Eject,
            label: EJECT_LABEL.into(),
            action: MenuAction::Eject(id),
        }));
        self.open_menu(at, items, cx);
    }
}
