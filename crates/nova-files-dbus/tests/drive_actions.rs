//! Against a private D-Bus daemon with a stand-in udisks2: mounting,
//! unmounting and ejecting through the watcher answer with what udisks2
//! did or why it didn't (mounting what is mounted answers where it is), eject unmounts first and never switches off a
//! drive the system runs from, and a client of the real core hears how its
//! action went.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test setup should fail loudly"
)]

use std::path::PathBuf;
use std::sync::mpsc;

use nova_files_api::{
    Command, DriveAction, DriveFailure, DriveId, DriveService as _, Event, VolumeId,
};
use nova_files_dbus::{Bus, Udisks};

mod support;
mod udisks;

/// A second volume on the stick, mounted inside the first.
const STICK_INNER: &str = "/org/freedesktop/UDisks2/block_devices/sdb2";

use support::{PrivateBus, WAIT};
use udisks::{
    EXT4_LOOP, FAT_LOOP, Failure, NVME, NVME_WINDOWS, STICK, STICK_VOLUME, StandIn, failing,
    laptop_disk, mounted_at, recorded, stick,
};

fn mount(volume: &str) -> DriveAction {
    DriveAction::Mount(VolumeId(volume.to_owned()))
}

fn unmount(volume: &str) -> DriveAction {
    DriveAction::Unmount(VolumeId(volume.to_owned()))
}

fn eject(drive: &str) -> DriveAction {
    DriveAction::Eject(DriveId(drive.to_owned()))
}

fn busy() -> Failure {
    Failure::DeviceBusy("target is busy".to_owned())
}

#[test]
fn mounting_answers_where_the_volume_went() {
    let bus = PrivateBus::start();
    let udisks = StandIn::start(&bus.address, &recorded());
    let result = Udisks::new(bus.bus()).act(&mount(FAT_LOOP));
    assert_eq!(result, Ok(Some(PathBuf::from("/run/media/me/loop0"))));
    assert_eq!(udisks.calls(), vec![format!("mount {FAT_LOOP}")]);
}

#[test]
fn unmounting_answers_nothing_when_done() {
    let bus = PrivateBus::start();
    let udisks = StandIn::start(&bus.address, &recorded());
    assert_eq!(Udisks::new(bus.bus()).act(&unmount(EXT4_LOOP)), Ok(None));
    assert_eq!(udisks.calls(), vec![format!("unmount {EXT4_LOOP}")]);
}

#[test]
fn what_is_already_so_counts_as_done() {
    let bus = PrivateBus::start();
    let _udisks = StandIn::start(&bus.address, &recorded());
    let udisks = Udisks::new(bus.bus());
    assert_eq!(
        udisks.act(&mount(EXT4_LOOP)),
        Ok(Some(PathBuf::from("/media/root/Backup")))
    );
    assert_eq!(udisks.act(&unmount(FAT_LOOP)), Ok(None));
}

#[test]
fn udisks_errors_become_failures_a_client_can_tell_apart() {
    let cases = [
        (busy(), DriveFailure::Busy),
        (
            Failure::NotAuthorizedDismissed("dismissed".to_owned()),
            DriveFailure::Cancelled,
        ),
        (
            Failure::NotAuthorized("not allowed".to_owned()),
            DriveFailure::NotAuthorized,
        ),
        (
            Failure::Failed("Filesystem type vfat not configured in kernel.".to_owned()),
            DriveFailure::Failed("Filesystem type vfat not configured in kernel.".to_owned()),
        ),
    ];
    for (answer, failure) in cases {
        let bus = PrivateBus::start();
        let _udisks = StandIn::start(&bus.address, &failing(recorded(), FAT_LOOP, &answer));
        assert_eq!(
            Udisks::new(bus.bus()).act(&mount(FAT_LOOP)),
            Err(failure),
            "{answer:?}"
        );
    }
}

#[test]
fn a_volume_that_is_not_there_is_gone() {
    let bus = PrivateBus::start();
    let _udisks = StandIn::start(&bus.address, &recorded());
    let udisks = Udisks::new(bus.bus());
    let missing = "/org/freedesktop/UDisks2/block_devices/sdz1";
    assert_eq!(udisks.act(&mount(missing)), Err(DriveFailure::Gone));
    assert_eq!(udisks.act(&unmount("not a path")), Err(DriveFailure::Gone));
    assert_eq!(udisks.act(&eject(missing)), Err(DriveFailure::Gone));
}

#[test]
fn without_udisks_there_is_no_service() {
    let bus = PrivateBus::start();
    assert_eq!(
        Udisks::new(bus.bus()).act(&mount(FAT_LOOP)),
        Err(DriveFailure::NoService)
    );
    assert_eq!(
        Udisks::new(bus.bus()).act(&eject(STICK)),
        Err(DriveFailure::NoService)
    );
    let gone = Bus::Address("unix:path=/nonexistent/nova-files-bus".to_owned());
    assert_eq!(
        Udisks::new(gone).act(&mount(FAT_LOOP)),
        Err(DriveFailure::NoService)
    );
}

#[test]
fn ejecting_a_stick_unmounts_it_then_switches_it_off() {
    let bus = PrivateBus::start();
    let udisks = StandIn::start(&bus.address, &stick());
    assert_eq!(Udisks::new(bus.bus()).act(&eject(STICK)), Ok(None));
    assert_eq!(
        udisks.calls(),
        vec![
            format!("unmount {STICK_VOLUME}"),
            format!("power off {STICK}")
        ]
    );
}

#[test]
fn a_volume_mounted_inside_another_is_unmounted_first() {
    let bus = PrivateBus::start();
    let mut objects = stick();
    let mut inner = objects[1].clone();
    inner.path = STICK_INNER.to_owned();
    inner.filesystem = Some(mounted_at(b"/run/media/me/STICK/inner"));
    objects.insert(1, inner);
    let udisks = StandIn::start(&bus.address, &objects);
    assert_eq!(Udisks::new(bus.bus()).act(&eject(STICK)), Ok(None));
    assert_eq!(
        udisks.calls(),
        vec![
            format!("unmount {STICK_INNER}"),
            format!("unmount {STICK_VOLUME}"),
            format!("power off {STICK}"),
        ]
    );
}

#[test]
fn a_busy_volume_stops_the_eject_before_the_drive_is_touched() {
    let bus = PrivateBus::start();
    let udisks = StandIn::start(&bus.address, &failing(stick(), STICK_VOLUME, &busy()));
    assert_eq!(
        Udisks::new(bus.bus()).act(&eject(STICK)),
        Err(DriveFailure::Busy)
    );
    assert_eq!(udisks.calls(), vec![format!("unmount {STICK_VOLUME}")]);
}

#[test]
fn the_disk_the_system_runs_from_is_never_ejected() {
    let bus = PrivateBus::start();
    let udisks = StandIn::start(&bus.address, &laptop_disk());
    assert_eq!(
        Udisks::new(bus.bus()).act(&eject(NVME)),
        Err(DriveFailure::System)
    );
    assert_eq!(udisks.calls(), Vec::<String>::new());
}

#[test]
fn a_drive_with_a_system_volume_is_ejected_but_never_switched_off() {
    let bus = PrivateBus::start();
    let disk: Vec<_> = laptop_disk()
        .into_iter()
        .filter(|object| [NVME, NVME_WINDOWS].contains(&object.path.as_str()))
        .map(|mut object| {
            if let Some(drive) = &mut object.drive {
                drive.ejectable = true;
                drive.can_power_off = true;
            }
            object
        })
        .collect();
    let udisks = StandIn::start(&bus.address, &disk);
    assert_eq!(Udisks::new(bus.bus()).act(&eject(NVME)), Ok(None));
    assert_eq!(udisks.calls(), vec![format!("eject {NVME}")]);
}

#[test]
fn a_client_of_the_core_hears_how_its_action_went() {
    let bus = PrivateBus::start();
    let _udisks = StandIn::start(&bus.address, &stick());
    let core = nova_files_core::spawn().unwrap();
    core.connector.offer_drives(Udisks::new(bus.bus())).unwrap();
    let (events, received) = mpsc::channel();
    let connection = core.connector.connect(events).unwrap();
    connection
        .send(Command::Drive(unmount(STICK_VOLUME)))
        .unwrap();
    assert_eq!(
        received.recv_timeout(WAIT).unwrap(),
        Event::DriveDone {
            action: unmount(STICK_VOLUME),
            result: Ok(None),
        }
    );
}
