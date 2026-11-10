//! Turns what udisks2 holds (its managed objects: every block device,
//! drive, partition and filesystem, each with its properties) into the
//! [`DriveList`] clients see.
//!
//! A volume is shown when udisks2 says it holds a filesystem and no rule
//! asks to hide it (`HintIgnore`), unless it is part of the system (the
//! root, `/boot`, `/nix`...): a system volume mounted outside the folders
//! removable media mount in. An unmounted internal partition shows, so it
//! can be mounted. A drive is shown when it holds a shown volume, so an
//! empty card reader or a disk of system volumes stays out.
//!
//! udisks2 sends paths (`Device`, `MountPoints`) as bytes ending in a NUL,
//! which may not be UTF-8; they are kept as bytes. A property missing or of
//! the wrong type reads as its empty value, so an older or newer udisks2
//! still gives a list.

use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};

use nova_files_api::{Drive, DriveId, DriveList, Volume, VolumeId};
use zbus::fdo::ManagedObjects;
use zbus::names::OwnedInterfaceName;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

/// One interface's properties, by name.
type Properties = HashMap<String, OwnedValue>;
/// One object's interfaces, by name.
type Interfaces = HashMap<OwnedInterfaceName, Properties>;

pub const BLOCK: &str = "org.freedesktop.UDisks2.Block";
pub const FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";
pub const PARTITION: &str = "org.freedesktop.UDisks2.Partition";
pub const DRIVE: &str = "org.freedesktop.UDisks2.Drive";

/// `Block.IdUsage` for a block that holds a filesystem.
const FILESYSTEM_USAGE: &str = "filesystem";
/// The object path udisks2 uses for "none", as in a block with no drive.
const NO_OBJECT: &str = "/";
/// Where removable media and volumes a person mounts end up; a system
/// volume mounted anywhere else is part of the running system.
const USER_MOUNT_FOLDERS: [&str; 3] = ["/media", "/run/media", "/mnt"];

/// The drives and volumes in `objects`, each sorted by id.
pub fn drive_list(objects: &ManagedObjects) -> DriveList {
    let mut volumes: Vec<Volume> = objects
        .iter()
        .filter_map(|(path, interfaces)| volume(path, interfaces))
        .collect();
    volumes.sort_by(|a, b| a.id.cmp(&b.id));
    let used: BTreeSet<&DriveId> = volumes.iter().filter_map(|v| v.drive.as_ref()).collect();
    let mut drives: Vec<Drive> = objects
        .iter()
        .filter_map(|(path, interfaces)| drive(path, interfaces))
        .filter(|drive| used.contains(&drive.id))
        .collect();
    drives.sort_by(|a, b| a.id.cmp(&b.id));
    DriveList { drives, volumes }
}

/// The volume `path` is, if it is one a person would open.
fn volume(path: &OwnedObjectPath, interfaces: &Interfaces) -> Option<Volume> {
    let block = interface(interfaces, BLOCK)?;
    let filesystem = interface(interfaces, FILESYSTEM)?;
    if text(block, "IdUsage") != FILESYSTEM_USAGE || flag(block, "HintIgnore") {
        return None;
    }
    let mount_points: Vec<PathBuf> = byte_strings(filesystem, "MountPoints")
        .into_iter()
        .map(|bytes| PathBuf::from(OsString::from_vec(bytes)))
        .collect();
    if flag(block, "HintSystem") && is_system_mount(&mount_points) {
        return None;
    }
    Some(Volume {
        id: VolumeId(path.as_str().to_owned()),
        drive: object(block, "Drive").map(DriveId),
        label: label(block, interface(interfaces, PARTITION)),
        size: number(block, "Size"),
        filesystem: text(block, "IdType"),
        mount_point: mount_points.into_iter().next(),
        device: OsString::from_vec(byte_string(block, "PreferredDevice")),
        read_only: flag(block, "ReadOnly"),
    })
}

/// The drive `path` is, if it is one.
fn drive(path: &OwnedObjectPath, interfaces: &Interfaces) -> Option<Drive> {
    let drive = interface(interfaces, DRIVE)?;
    let name = [text(drive, "Vendor"), text(drive, "Model")]
        .iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Some(Drive {
        id: DriveId(path.as_str().to_owned()),
        name,
        removable: flag(drive, "Removable") || flag(drive, "MediaRemovable"),
        ejectable: flag(drive, "Ejectable") || flag(drive, "CanPowerOff"),
    })
}

/// Whether a volume mounted at `mount_points` is part of the running
/// system: mounted, and nowhere a person's volumes go.
fn is_system_mount(mount_points: &[PathBuf]) -> bool {
    !mount_points.is_empty()
        && !mount_points.iter().any(|point| {
            USER_MOUNT_FOLDERS
                .iter()
                .any(|folder| point.starts_with(Path::new(folder)))
        })
}

/// A volume's name: the one an admin set (`HintName`), else the
/// filesystem's label, else the partition's name; empty without any.
fn label(block: &Properties, partition: Option<&Properties>) -> String {
    [
        text(block, "HintName"),
        text(block, "IdLabel"),
        partition
            .map(|partition| text(partition, "Name"))
            .unwrap_or_default(),
    ]
    .into_iter()
    .find(|name| !name.is_empty())
    .unwrap_or_default()
}

fn interface<'a>(interfaces: &'a Interfaces, name: &str) -> Option<&'a Properties> {
    interfaces
        .iter()
        .find(|(interface, _)| interface.as_str() == name)
        .map(|(_, properties)| properties)
}

/// A property as `T`; `None` when missing or of another type.
fn property<T: TryFrom<OwnedValue>>(properties: &Properties, key: &str) -> Option<T> {
    T::try_from(properties.get(key)?.try_clone().ok()?).ok()
}

fn text(properties: &Properties, key: &str) -> String {
    property(properties, key).unwrap_or_default()
}

fn flag(properties: &Properties, key: &str) -> bool {
    property(properties, key).unwrap_or_default()
}

fn number(properties: &Properties, key: &str) -> u64 {
    property(properties, key).unwrap_or_default()
}

/// An object path property; `None` when it names no object.
fn object(properties: &Properties, key: &str) -> Option<String> {
    let path: OwnedObjectPath = property(properties, key)?;
    (path.as_str() != NO_OBJECT).then(|| path.as_str().to_owned())
}

/// A path property, without the NUL udisks2 ends it with.
fn byte_string(properties: &Properties, key: &str) -> Vec<u8> {
    without_nul(property(properties, key).unwrap_or_default())
}

fn byte_strings(properties: &Properties, key: &str) -> Vec<Vec<u8>> {
    property::<Vec<Vec<u8>>>(properties, key)
        .unwrap_or_default()
        .into_iter()
        .map(without_nul)
        .filter(|bytes| !bytes.is_empty())
        .collect()
}

fn without_nul(mut bytes: Vec<u8>) -> Vec<u8> {
    while bytes.last() == Some(&0) {
        bytes.pop();
    }
    bytes
}
