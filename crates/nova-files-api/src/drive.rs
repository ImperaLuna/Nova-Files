//! Drives and the volumes on them, as the core reports them, and the
//! service that watches them for the core.
//!
//! A client asks with [`crate::Command::Drives`] and gets the whole
//! [`DriveList`] as [`crate::Event::Drives`], then again each time a drive
//! or volume comes, goes or changes (plugged in, mounted, renamed).
//!
//! The core can't speak D-Bus, where the system's mount service
//! (udisks2) lives, so the program hands it a [`DriveService`] that does.
//! The service reads nothing until the first client asks, so start-up
//! never waits on it.

use std::ffi::OsString;
use std::path::PathBuf;

/// Names one volume: something that holds files and can be mounted, such
/// as a partition or a whole USB stick. The service picks it; it stays the
/// same while the volume is plugged in.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VolumeId(pub String);

/// Names one drive: the device the volumes are on, such as a USB stick or
/// a disk. The service picks it; it stays the same while the drive is
/// plugged in.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DriveId(pub String);

/// Every drive and every volume a person would want to open, in the
/// service's order: drives by id, volumes by id.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DriveList {
    pub drives: Vec<Drive>,
    pub volumes: Vec<Volume>,
}

/// A device that holds volumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drive {
    pub id: DriveId,
    /// Its maker and model as the device reports them, such as
    /// "Kingston `DataTraveler`"; empty when it reports neither.
    pub name: String,
    /// Whether it can be unplugged or have its media taken out (a USB
    /// stick, an SD card, a disc).
    pub removable: bool,
    /// Whether it can be ejected or switched off, so that it is safe to
    /// unplug.
    pub ejectable: bool,
}

/// Something that holds files and can be mounted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    pub id: VolumeId,
    /// The drive it is on; `None` for a volume with no device behind it,
    /// such as a disk image set up as a loop device.
    pub drive: Option<DriveId>,
    /// Its name: the one the system's rules give it, else the
    /// filesystem's label, else the partition's name. Empty when it has
    /// none, so a client names it by size.
    pub label: String,
    /// Its size in bytes.
    pub size: u64,
    /// The filesystem's type as the system names it, such as `ext4` or
    /// `vfat`.
    pub filesystem: String,
    /// Where it is mounted; `None` while it isn't. A volume mounted in
    /// several places shows the first.
    pub mount_point: Option<PathBuf>,
    /// The device file, such as `/dev/sdb1`, exactly as the system names
    /// it.
    pub device: OsString,
    /// Whether it can only be read.
    pub read_only: bool,
}

/// Watches the system's drives and volumes for the core. The program
/// hands one to the core at start-up; without one, every client gets an
/// empty list.
pub trait DriveService: Send + Sync + 'static {
    /// Starts watching: `seen` gets the list as it is now, then again each
    /// time it changes, from a thread of the service's own. Called at most
    /// once, when the first client asks. A service that can't reach the
    /// system's mount service sends an empty list.
    fn follow(&self, seen: Box<dyn Fn(DriveList) + Send + Sync>);
}
