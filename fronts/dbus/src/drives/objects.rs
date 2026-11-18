use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};

use nova_files_drives_api::{Drive, DriveFailure, DriveId, DriveList, Volume, VolumeId};
use zbus::fdo::ManagedObjects;
use zbus::names::OwnedInterfaceName;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

type Properties = HashMap<String, OwnedValue>;
type Interfaces = HashMap<OwnedInterfaceName, Properties>;

pub const BLOCK: &str = "org.freedesktop.UDisks2.Block";
pub const FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";
pub const PARTITION: &str = "org.freedesktop.UDisks2.Partition";
pub const DRIVE: &str = "org.freedesktop.UDisks2.Drive";
pub const MOUNT_POINTS: &str = "MountPoints";

const FILESYSTEM_USAGE: &str = "filesystem";
const NO_OBJECT: &str = "/";
const USER_MOUNT_FOLDERS: [&str; 3] = ["/media", "/run/media", "/mnt"];

pub fn drive_list(objects: &ManagedObjects) -> DriveList {
    let mut volumes: Vec<Volume> = objects
        .iter()
        .filter_map(|(path, interfaces)| volume(path, interfaces))
        .collect();
    volumes.sort_by(|a, b| a.id.cmp(&b.id));
    let used: BTreeSet<&DriveId> = volumes.iter().filter_map(|v| v.drive.as_ref()).collect();
    let mut drives: Vec<Drive> = objects
        .iter()
        .filter_map(|(path, interfaces)| drive(objects, path, interfaces))
        .filter(|drive| used.contains(&drive.id))
        .collect();
    drives.sort_by(|a, b| a.id.cmp(&b.id));
    DriveList { drives, volumes }
}

fn volume(path: &OwnedObjectPath, interfaces: &Interfaces) -> Option<Volume> {
    let block = interface(interfaces, BLOCK)?;
    let filesystem = interface(interfaces, FILESYSTEM)?;
    if text(block, "IdUsage") != FILESYSTEM_USAGE || flag(block, "HintIgnore") {
        return None;
    }
    let mount_points = mount_points(filesystem);
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

fn drive(
    objects: &ManagedObjects,
    path: &OwnedObjectPath,
    interfaces: &Interfaces,
) -> Option<Drive> {
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
        ejectable: eject_plan(objects, path).is_ok_and(|plan| plan.eject || plan.power_off),
    })
}

fn mount_points(filesystem: &Properties) -> Vec<PathBuf> {
    paths(property(filesystem, MOUNT_POINTS).unwrap_or_default())
}

pub fn first_mount_point(points: Vec<Vec<u8>>) -> Option<PathBuf> {
    paths(points).into_iter().next()
}

fn paths(sent: Vec<Vec<u8>>) -> Vec<PathBuf> {
    sent.into_iter()
        .map(without_nul)
        .filter(|bytes| !bytes.is_empty())
        .map(|bytes| PathBuf::from(OsString::from_vec(bytes)))
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
pub struct EjectPlan {
    pub mounted: Vec<OwnedObjectPath>,
    pub eject: bool,
    pub power_off: bool,
}

pub fn eject_plan(
    objects: &ManagedObjects,
    drive: &OwnedObjectPath,
) -> Result<EjectPlan, DriveFailure> {
    let properties = objects
        .get(drive)
        .and_then(|interfaces| interface(interfaces, DRIVE))
        .ok_or(DriveFailure::Gone)?;
    let mut plan = EjectPlan {
        mounted: Vec::new(),
        eject: flag(properties, "Ejectable"),
        power_off: flag(properties, "CanPowerOff"),
    };
    let mut mounted = Vec::new();
    for (path, interfaces) in objects {
        let Some(block) = interface(interfaces, BLOCK) else {
            continue;
        };
        if object(block, "Drive").as_deref() != Some(drive.as_str()) {
            continue;
        }
        let system = flag(block, "HintSystem");
        plan.power_off &= !system;
        let points = interface(interfaces, FILESYSTEM)
            .map(mount_points)
            .unwrap_or_default();
        if points.is_empty() {
            continue;
        }
        if system && is_system_mount(&points) {
            return Err(DriveFailure::System);
        }
        let depth = points.iter().map(|point| point.components().count()).max();
        mounted.push((depth, path.clone()));
    }
    mounted.sort_by(|(a_depth, a), (b_depth, b)| {
        b_depth
            .cmp(a_depth)
            .then_with(|| a.as_str().cmp(b.as_str()))
    });
    plan.mounted = mounted.into_iter().map(|(_, path)| path).collect();
    Ok(plan)
}

fn is_system_mount(mount_points: &[PathBuf]) -> bool {
    mount_points.iter().any(|point| {
        !USER_MOUNT_FOLDERS
            .iter()
            .any(|folder| point.starts_with(Path::new(folder)))
    })
}

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

fn object(properties: &Properties, key: &str) -> Option<String> {
    let path: OwnedObjectPath = property(properties, key)?;
    (path.as_str() != NO_OBJECT).then(|| path.as_str().to_owned())
}

fn byte_string(properties: &Properties, key: &str) -> Vec<u8> {
    without_nul(property(properties, key).unwrap_or_default())
}

fn without_nul(mut bytes: Vec<u8>) -> Vec<u8> {
    while bytes.last() == Some(&0) {
        bytes.pop();
    }
    bytes
}
