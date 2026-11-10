//! Against a private D-Bus daemon with a stand-in udisks2 serving recorded
//! drives: the watcher lists the volumes a person would open and hides the
//! system's own, and follows drives being plugged in and out, volumes
//! being mounted, and udisks2 starting late. Through the real core, a
//! client that asks gets the list.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use nova_files_api::{
    Command, Drive, DriveId, DriveList, DriveService as _, Event, Volume, VolumeId,
};
use nova_files_dbus::Udisks;

mod support;
mod udisks;

use support::{PrivateBus, WAIT};
use udisks::{
    EXT4_LOOP, FAT_LOOP, GPT_PHOTOS, GPT_UNNAMED, NVME, NVME_WINDOWS, STICK, STICK_VOLUME, StandIn,
    core_client, laptop_disk, recorded, stick,
};

/// How long a test waits for a list that should not come.
const QUIET: Duration = Duration::from_millis(300);

/// Follows udisks2 on `bus`; each list arrives on the receiver.
fn follow(bus: &PrivateBus) -> Receiver<DriveList> {
    let (sender, lists) = mpsc::channel();
    Udisks::new(bus.bus()).follow(Box::new(move |list| {
        sender.send(list).ok();
    }));
    lists
}

/// The next list.
fn next(lists: &Receiver<DriveList>) -> DriveList {
    lists.recv_timeout(WAIT).expect("a drive list")
}

/// The first list for which `wanted` holds, skipping earlier ones.
fn until(lists: &Receiver<DriveList>, wanted: impl Fn(&DriveList) -> bool) -> DriveList {
    loop {
        let list = next(lists);
        if wanted(&list) {
            return list;
        }
    }
}

fn ids(list: &DriveList) -> Vec<&str> {
    list.volumes
        .iter()
        .map(|volume| volume.id.0.as_str())
        .collect()
}

fn volume<'a>(list: &'a DriveList, id: &str) -> &'a Volume {
    list.volumes
        .iter()
        .find(|volume| volume.id.0 == id)
        .unwrap_or_else(|| panic!("no volume {id} in {list:?}"))
}

#[test]
fn loop_disks_list_their_filesystems_and_nothing_else() {
    let bus = PrivateBus::start();
    let _udisks = StandIn::start(&bus.address, &recorded());
    let list = next(&follow(&bus));
    assert_eq!(
        ids(&list),
        vec![FAT_LOOP, EXT4_LOOP, GPT_PHOTOS, GPT_UNNAMED],
        "the GPT disk itself, the empty loop and zram (HintIgnore) stay out"
    );
    assert!(list.drives.is_empty(), "loop devices have no drive");
    assert_eq!(
        volume(&list, EXT4_LOOP),
        &Volume {
            id: VolumeId(EXT4_LOOP.to_owned()),
            drive: None,
            label: "Backup".to_owned(),
            size: 67_108_864,
            filesystem: "ext4".to_owned(),
            mount_point: Some(PathBuf::from("/media/root/Backup")),
            device: OsString::from("/dev/loop1"),
            read_only: false,
        }
    );
    let fat = volume(&list, FAT_LOOP);
    assert_eq!(
        (
            fat.label.as_str(),
            fat.filesystem.as_str(),
            &fat.mount_point
        ),
        ("NOVASTICK", "vfat", &None)
    );
    assert_eq!(volume(&list, GPT_UNNAMED).label, "");
}

#[test]
fn a_stick_shows_its_drive_and_volume() {
    let bus = PrivateBus::start();
    let _udisks = StandIn::start(&bus.address, &stick());
    let list = next(&follow(&bus));
    assert_eq!(
        list.drives,
        vec![Drive {
            id: DriveId(STICK.to_owned()),
            name: "SanDisk Ultra".to_owned(),
            removable: true,
            ejectable: true,
        }]
    );
    let volume = volume(&list, STICK_VOLUME);
    assert_eq!(volume.drive, Some(DriveId(STICK.to_owned())));
    assert_eq!(volume.label, "STICK");
    assert_eq!(
        volume.mount_point,
        Some(PathBuf::from("/run/media/me/STICK"))
    );
}

#[test]
fn the_system_volumes_stay_out_and_an_unmounted_partition_shows() {
    let bus = PrivateBus::start();
    let _udisks = StandIn::start(&bus.address, &laptop_disk());
    let list = next(&follow(&bus));
    assert_eq!(ids(&list), vec![NVME_WINDOWS]);
    assert_eq!(
        list.drives,
        vec![Drive {
            id: DriveId(NVME.to_owned()),
            name: "Samsung SSD 980 1TB".to_owned(),
            removable: false,
            ejectable: false,
        }]
    );
}

#[test]
fn a_drive_that_could_only_be_switched_off_with_a_system_volume_is_not_ejectable() {
    let bus = PrivateBus::start();
    let disk: Vec<_> = laptop_disk()
        .into_iter()
        .map(|mut object| {
            if let Some(drive) = &mut object.drive {
                drive.can_power_off = true;
            }
            object
        })
        .collect();
    let _udisks = StandIn::start(&bus.address, &disk);
    let list = next(&follow(&bus));
    assert_eq!(list.drives.len(), 1);
    assert!(!list.drives[0].ejectable);
}

#[test]
fn a_drive_with_no_volume_to_show_stays_out() {
    let bus = PrivateBus::start();
    let disk: Vec<_> = laptop_disk()
        .into_iter()
        .filter(|object| object.path != NVME_WINDOWS)
        .collect();
    let _udisks = StandIn::start(&bus.address, &disk);
    assert_eq!(next(&follow(&bus)), DriveList::default());
}

#[test]
fn plugging_a_stick_in_and_out_is_followed() {
    let bus = PrivateBus::start();
    let udisks = StandIn::start(&bus.address, &recorded());
    let lists = follow(&bus);
    let before = next(&lists);

    for object in stick() {
        udisks.add(&object);
    }
    let plugged = until(&lists, |list| {
        list.volumes.len() == before.volumes.len() + 1
    });
    assert_eq!(volume(&plugged, STICK_VOLUME).label, "STICK");
    assert_eq!(plugged.drives.len(), 1);

    udisks.remove(STICK_VOLUME);
    udisks.remove(STICK);
    let unplugged = until(&lists, |list| list.drives.is_empty());
    assert_eq!(unplugged, before);
}

#[test]
fn mounting_a_volume_is_followed() {
    let bus = PrivateBus::start();
    let udisks = StandIn::start(&bus.address, &recorded());
    let lists = follow(&bus);
    next(&lists);
    udisks.set_mount_points(FAT_LOOP, &[b"/media/root/NOVASTICK"]);
    let list = until(&lists, |list| volume(list, FAT_LOOP).mount_point.is_some());
    assert_eq!(
        volume(&list, FAT_LOOP).mount_point,
        Some(PathBuf::from("/media/root/NOVASTICK"))
    );
}

#[test]
fn a_mount_point_that_is_not_utf8_keeps_its_bytes() {
    let bus = PrivateBus::start();
    let udisks = StandIn::start(&bus.address, &recorded());
    let lists = follow(&bus);
    next(&lists);
    let point = b"/run/media/me/caf\xe9";
    udisks.set_mount_points(FAT_LOOP, &[point]);
    let list = until(&lists, |list| volume(list, FAT_LOOP).mount_point.is_some());
    assert_eq!(
        volume(&list, FAT_LOOP).mount_point,
        Some(PathBuf::from(OsString::from_vec(point.to_vec())))
    );
}

#[test]
fn a_system_volume_mounted_elsewhere_leaves_the_list() {
    let bus = PrivateBus::start();
    let udisks = StandIn::start(&bus.address, &recorded());
    let lists = follow(&bus);
    next(&lists);
    udisks.set_mount_points(FAT_LOOP, &[b"/srv/data"]);
    let list = until(&lists, |list| !ids(list).contains(&FAT_LOOP));
    assert_eq!(ids(&list), vec![EXT4_LOOP, GPT_PHOTOS, GPT_UNNAMED]);
}

#[test]
fn a_system_volume_also_mounted_under_mnt_stays_out() {
    let bus = PrivateBus::start();
    let udisks = StandIn::start(&bus.address, &recorded());
    let lists = follow(&bus);
    next(&lists);
    udisks.set_mount_points(FAT_LOOP, &[b"/", b"/mnt/snapshots"]);
    let list = until(&lists, |list| !ids(list).contains(&FAT_LOOP));
    assert_eq!(ids(&list), vec![EXT4_LOOP, GPT_PHOTOS, GPT_UNNAMED]);
}

#[test]
fn without_udisks_the_list_is_empty_until_it_starts() {
    let bus = PrivateBus::start();
    let lists = follow(&bus);
    assert_eq!(next(&lists), DriveList::default());
    let _udisks = StandIn::start(&bus.address, &stick());
    let list = until(&lists, |list| !list.volumes.is_empty());
    assert_eq!(ids(&list), vec![STICK_VOLUME]);
}

#[test]
fn a_udisks_that_cannot_answer_still_gives_a_first_list() {
    let bus = PrivateBus::start();
    let _udisks = StandIn::mute(&bus.address);
    assert_eq!(next(&follow(&bus)), DriveList::default());
}

#[test]
fn a_read_that_fails_while_udisks_runs_keeps_the_last_list() {
    let bus = PrivateBus::start();
    let udisks = StandIn::start(&bus.address, &stick());
    let lists = follow(&bus);
    next(&lists);
    udisks.stop_answering();
    udisks.poke(STICK_VOLUME);
    assert!(
        lists.recv_timeout(QUIET).is_err(),
        "no empty list flashes by"
    );
}

#[test]
fn udisks_stopping_empties_the_list() {
    let bus = PrivateBus::start();
    let udisks = StandIn::start(&bus.address, &stick());
    let lists = follow(&bus);
    assert_eq!(ids(&next(&lists)), vec![STICK_VOLUME]);
    udisks.stop();
    assert_eq!(
        until(&lists, |list| list.volumes.is_empty()),
        DriveList::default()
    );
}

#[test]
fn without_a_bus_the_list_is_empty() {
    let (sender, lists) = mpsc::channel();
    let gone = nova_files_dbus::Bus::Address("unix:path=/nonexistent/nova-files-bus".to_owned());
    Udisks::new(gone).follow(Box::new(move |list| {
        sender.send(list).ok();
    }));
    assert_eq!(next(&lists), DriveList::default());
    assert!(lists.recv_timeout(QUIET).is_err());
}

#[test]
fn a_client_of_the_core_gets_the_drives() {
    let bus = PrivateBus::start();
    let _udisks = StandIn::start(&bus.address, &stick());
    let (connection, received) = core_client(bus.bus());
    connection.send(Command::Drives).unwrap();
    match received.recv_timeout(WAIT).unwrap() {
        Event::Drives(list) => assert_eq!(ids(&list), vec![STICK_VOLUME]),
        other => panic!("not drives: {other:?}"),
    }
}
