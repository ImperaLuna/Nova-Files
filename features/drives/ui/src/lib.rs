#![forbid(unsafe_code)]

mod cards;
mod shown;

use std::collections::HashMap;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{AnyElement, App, Context, Subscription, WeakEntity, Window};
use nova_files_drives_api::{
    Command, DiskSpace, DriveAction, DriveFailure, DriveList, Event, VolumeId,
};
use nova_files_shell::assets::Icon;
use nova_files_shell::{
    CardRank, Host, Location, Look, NavEvent, Part, Published, RowItem, Scene, SidebarAt,
    SidebarRow, Slots,
};

pub use shown::{NEARLY_FULL, ShownDrive, free, here, shown};

const ROW_PREFIX: &str = "drive-";
const OPEN_LABEL: &str = "Open";
const NEW_TAB_LABEL: &str = "Open in new tab";
const EJECT_LABEL: &str = "Eject";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnownDrives {
    pub list: Arc<DriveList>,
    pub space: HashMap<PathBuf, DiskSpace>,
}

pub struct DrivesPart {
    host: Host,
    this: WeakEntity<Self>,
    known: KnownDrives,
    opening: Option<VolumeId>,
    asking: bool,
    ask_again: bool,
    _left: Subscription,
}

impl DrivesPart {
    pub fn shown(&self) -> Vec<ShownDrive> {
        shown(&self.known.list)
    }

    pub fn disk_space(&self, folder: &Path) -> Option<DiskSpace> {
        self.known.space.get(folder).copied()
    }

    pub fn open_drive(&mut self, drive: ShownDrive, cx: &mut Context<Self>) {
        match (drive.folder, drive.volume) {
            (Some(folder), _) => self.go(Location::Folder(folder), cx),
            (None, Some(volume)) if self.opening.as_ref() == Some(&volume) => {}
            (None, Some(volume)) => {
                self.opening = Some(volume.clone());
                self.act(DriveAction::Mount(volume));
            }
            (None, None) => {}
        }
    }

    pub fn open_drive_in_new_tab(&mut self, drive: ShownDrive, cx: &mut Context<Self>) {
        if let Some(folder) = drive.folder.clone() {
            self.host.nav.update(cx, |nav, cx| {
                nav.open_in_new_tab(Location::Folder(folder), cx);
            });
        } else {
            self.host.nav.update(cx, |nav, cx| {
                nav.open_in_new_tab(Location::start(), cx);
            });
            self.open_drive(drive, cx);
        }
    }

    fn act(&self, action: DriveAction) {
        self.host.link.send(Command::Drive(action)).ok();
    }

    fn go(&self, place: Location, cx: &mut Context<Self>) {
        self.host.nav.update(cx, |nav, cx| nav.go(place, cx));
    }

    fn arrived(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Drives(list) => {
                self.leave_gone_drive(&list, cx);
                self.known.list = list;
                let shown: Vec<PathBuf> = self
                    .shown()
                    .into_iter()
                    .filter_map(|drive| drive.folder)
                    .collect();
                self.known.space.retain(|folder, _| shown.contains(folder));
                self.ask_disk_space();
            }
            Event::DiskSpace(spaces) => {
                for answer in spaces {
                    match answer.space {
                        Some(space) => self.known.space.insert(answer.path, space),
                        None => self.known.space.remove(&answer.path),
                    };
                }
                self.asking = false;
                if std::mem::take(&mut self.ask_again) {
                    self.ask_disk_space();
                }
            }
            Event::DriveDone { action, result } => self.drive_done(&action, result, cx),
        }
        Published::set(self.known.clone(), cx);
        cx.notify();
    }

    fn drive_done(
        &mut self,
        action: &DriveAction,
        result: Result<Option<PathBuf>, DriveFailure>,
        cx: &mut Context<Self>,
    ) {
        let opening = match action {
            DriveAction::Mount(volume) => self.opening.as_ref() == Some(volume),
            DriveAction::Unmount(_) | DriveAction::Eject(_) => false,
        };
        if opening {
            self.opening = None;
        }
        match result {
            Ok(Some(folder)) if opening => self.go(Location::Folder(folder), cx),
            Ok(_) => {}
            Err(failure) => {
                writeln!(io::stderr(), "nova-files: {action:?} failed: {failure:?}").ok();
            }
        }
    }

    fn ask_disk_space(&mut self) {
        if self.asking {
            self.ask_again = true;
            return;
        }
        let folders = self
            .shown()
            .into_iter()
            .filter_map(|drive| drive.folder)
            .collect();
        self.asking = self.host.link.send(Command::DiskSpace(folders)).is_ok();
    }

    fn leave_gone_drive(&self, next: &DriveList, cx: &mut Context<Self>) {
        let scene = self.host.scene(cx);
        let Some(here) = scene.location.as_ref().and_then(Location::folder) else {
            return;
        };
        let mounted = |list: &DriveList| -> Vec<PathBuf> {
            list.volumes
                .iter()
                .filter_map(|volume| volume.mount_point.clone())
                .collect()
        };
        let still = mounted(next);
        let gone = mounted(&self.known.list)
            .into_iter()
            .any(|folder| here.starts_with(&folder) && !still.contains(&folder));
        if gone {
            self.go(Location::Folder(self.host.home.clone()), cx);
        }
    }

    fn listed(&mut self, _: &nova_files_api::Listing, _: bool, _: &mut Context<Self>) {
        self.ask_disk_space();
    }

    fn drive_at(&self, row: &str) -> Option<ShownDrive> {
        let index: usize = row.strip_prefix(ROW_PREFIX)?.parse().ok()?;
        self.shown().into_iter().nth(index)
    }

    fn run_on(
        &self,
        drive: ShownDrive,
        run: fn(&mut Self, ShownDrive, &mut Context<Self>),
    ) -> impl Fn(&mut App) + 'static {
        let part = self.this.clone();
        move |cx| {
            let drive = drive.clone();
            part.update(cx, |part, cx| run(part, drive, cx)).ok();
        }
    }
}

fn rows(part: &DrivesPart, scene: &Scene, _: &App) -> Vec<SidebarRow> {
    let shown = part.shown();
    let at = scene
        .location
        .as_ref()
        .and_then(Location::folder)
        .and_then(|folder| here(&shown, folder, &part.host.home));
    shown
        .into_iter()
        .enumerate()
        .map(|(index, drive)| {
            let space = drive
                .folder
                .as_deref()
                .and_then(|folder| part.disk_space(folder));
            let (number, look) = space.map_or((None, Look::Plain), |space| {
                let (text, full) = free(space);
                (Some(text), if full { Look::Danger } else { Look::Plain })
            });
            let id = format!("{ROW_PREFIX}{index}");
            let row = match drive.folder.clone() {
                Some(folder) => {
                    SidebarRow::new(id, Icon::Drive, drive.name, Location::Folder(folder))
                }
                None => SidebarRow::acting(
                    id,
                    Icon::Drive,
                    drive.name.clone(),
                    RowItem::new(
                        Icon::Open,
                        OPEN_LABEL,
                        part.run_on(drive.clone(), DrivesPart::open_drive),
                    ),
                )
                .on_new_tab(RowItem::new(
                    Icon::Plus,
                    NEW_TAB_LABEL,
                    part.run_on(drive, DrivesPart::open_drive_in_new_tab),
                )),
            };
            row.rank(SidebarRow::OWN)
                .number(number)
                .number_look(look)
                .here(at == Some(index))
        })
        .collect()
}

fn row_menu(part: &DrivesPart, row: &str, _: &App) -> Vec<Vec<RowItem>> {
    let Some(drive) = part.drive_at(row) else {
        return Vec::new();
    };
    let eject = drive.eject.clone().map(|id| {
        let link = part.host.link.clone();
        RowItem::new(Icon::Eject, EJECT_LABEL, move |_| {
            link.send(Command::Drive(DriveAction::Eject(id.clone())))
                .ok();
        })
    });
    vec![
        vec![
            RowItem::new(
                Icon::Open,
                OPEN_LABEL,
                part.run_on(drive.clone(), DrivesPart::open_drive),
            ),
            RowItem::new(
                Icon::Plus,
                NEW_TAB_LABEL,
                part.run_on(drive, DrivesPart::open_drive_in_new_tab),
            ),
        ],
        eject.into_iter().collect(),
    ]
}

impl Part for DrivesPart {
    fn open(host: &Host, cx: &mut Context<Self>) -> Self {
        host.link.send(Command::Drives).ok();
        let left = cx.subscribe(&host.nav, |part, _, event: &NavEvent, _| {
            if *event == NavEvent::Left {
                part.opening = None;
            }
        });
        Self {
            host: host.clone(),
            this: cx.entity().downgrade(),
            known: KnownDrives::default(),
            opening: None,
            asking: false,
            ask_again: false,
            _left: left,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(Self::arrived);
        slots.sends::<Command>();
        slots.listed(Self::listed);
        slots.sidebar(SidebarAt::Drives, rows);
        slots.row_menu(row_menu);
        slots.start_card_at(CardRank::DRIVES, drive_cards);
    }
}

fn drive_cards(
    part: &mut DrivesPart,
    _: &Scene,
    _: &mut Window,
    cx: &mut Context<DrivesPart>,
) -> Option<AnyElement> {
    cards::drive_cards(part, cx)
}
