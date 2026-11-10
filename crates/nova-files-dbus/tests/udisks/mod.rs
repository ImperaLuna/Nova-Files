//! A stand-in for udisks2 on a private bus, serving drives and volumes
//! recorded from a real one, so the drive tests never need real disks.
//!
//! It serves udisks2's object manager and, for each object, the
//! interfaces and properties Nova Files reads, under udisks2's own
//! names. Adding or removing an object sends the object manager's signals
//! and a changed mount point sends `PropertiesChanged`, as udisks2 does.
//! Mounting, unmounting, ejecting and switching off do what udisks2 does
//! to the properties (refusing, as udisks2 does, to mount a mounted volume
//! or unmount one that isn't), or fail as set (`Filesystem::fails`), and each call
//! is noted ([`StandIn::calls`]).
//!
//! The recordings ([`recorded`]) come from udisksd 2.10.1
//! (`GetManagedObjects`, 2026-11-10) with loop disks: a FAT one, an ext4
//! one, and a GPT one with two ext4 partitions. udisks2 gives a loop device
//! no drive, so the USB stick and the laptop's `NVMe` disk are written from
//! udisks2's documented properties, with the values a real stick and disk
//! report.

#![expect(dead_code, reason = "each test file uses part of it")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nova_files_dbus::{UDISKS, UDISKS_PATH};
use zbus::blocking::Connection;
use zbus::blocking::connection::Builder;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

/// Checks a call left udisks2's options at their defaults, as Nova Files
/// does.
fn no_options(options: HashMap<String, OwnedValue>) {
    assert!(options.into_keys().next().is_none(), "options were set");
}

/// The calls made to the stand-in, such as `unmount <object path>`.
pub type Calls = Arc<Mutex<Vec<String>>>;

/// The errors udisks2 answers with, by their D-Bus names.
#[derive(Clone, Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.UDisks2.Error")]
pub enum Failure {
    #[zbus(error)]
    ZBus(zbus::Error),
    DeviceBusy(String),
    AlreadyMounted(String),
    NotMounted(String),
    NotAuthorizedDismissed(String),
    NotAuthorized(String),
    Failed(String),
}

/// udisks2's object path for "none".
const NONE: &str = "/";

/// A path as udisks2 sends it: bytes ending in a NUL.
pub fn path_bytes(path: &[u8]) -> Vec<u8> {
    let mut bytes = path.to_vec();
    bytes.push(0);
    bytes
}

fn object_path(path: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(path).unwrap()
}

#[derive(Clone)]
pub struct Block {
    pub device: Vec<u8>,
    pub size: u64,
    pub read_only: bool,
    pub drive: String,
    pub id_usage: String,
    pub id_type: String,
    pub id_label: String,
    pub hint_ignore: bool,
    pub hint_system: bool,
    pub hint_name: String,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Block")]
impl Block {
    #[zbus(property)]
    fn device(&self) -> Vec<u8> {
        self.device.clone()
    }
    #[zbus(property)]
    fn preferred_device(&self) -> Vec<u8> {
        self.device.clone()
    }
    #[zbus(property)]
    const fn size(&self) -> u64 {
        self.size
    }
    #[zbus(property)]
    const fn read_only(&self) -> bool {
        self.read_only
    }
    #[zbus(property)]
    fn drive(&self) -> OwnedObjectPath {
        object_path(&self.drive)
    }
    #[zbus(property)]
    fn id_usage(&self) -> String {
        self.id_usage.clone()
    }
    #[zbus(property)]
    fn id_type(&self) -> String {
        self.id_type.clone()
    }
    #[zbus(property)]
    fn id_label(&self) -> String {
        self.id_label.clone()
    }
    #[zbus(property)]
    const fn hint_ignore(&self) -> bool {
        self.hint_ignore
    }
    #[zbus(property)]
    const fn hint_system(&self) -> bool {
        self.hint_system
    }
    #[zbus(property)]
    fn hint_name(&self) -> String {
        self.hint_name.clone()
    }
}

#[derive(Clone)]
pub struct Filesystem {
    pub path: String,
    pub mount_points: Vec<Vec<u8>>,
    /// Where `Mount` mounts it.
    pub mounts_at: String,
    /// What `Mount` and `Unmount` answer instead of doing it.
    pub fails: Option<Failure>,
    pub calls: Calls,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Filesystem")]
impl Filesystem {
    #[zbus(property)]
    fn mount_points(&self) -> Vec<Vec<u8>> {
        self.mount_points.clone()
    }

    async fn mount(
        &mut self,
        options: HashMap<String, OwnedValue>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<String, Failure> {
        no_options(options);
        self.calls
            .lock()
            .unwrap()
            .push(format!("mount {}", self.path));
        if let Some(failure) = &self.fails {
            return Err(failure.clone());
        }
        if !self.mount_points.is_empty() {
            return Err(Failure::AlreadyMounted("already mounted".to_owned()));
        }
        self.mount_points = vec![path_bytes(self.mounts_at.as_bytes())];
        self.mount_points_changed(&emitter).await?;
        Ok(self.mounts_at.clone())
    }

    async fn unmount(
        &mut self,
        options: HashMap<String, OwnedValue>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Failure> {
        no_options(options);
        self.calls
            .lock()
            .unwrap()
            .push(format!("unmount {}", self.path));
        if let Some(failure) = &self.fails {
            return Err(failure.clone());
        }
        if self.mount_points.is_empty() {
            return Err(Failure::NotMounted("not mounted".to_owned()));
        }
        self.mount_points.clear();
        self.mount_points_changed(&emitter).await?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct Partition {
    pub number: u32,
    pub name: String,
    pub table: String,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Partition")]
impl Partition {
    #[zbus(property)]
    const fn number(&self) -> u32 {
        self.number
    }
    #[zbus(property)]
    fn name(&self) -> String {
        self.name.clone()
    }
    #[zbus(property)]
    fn table(&self) -> OwnedObjectPath {
        object_path(&self.table)
    }
}

#[derive(Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "udisks2's own properties, one field each"
)]
pub struct Drive {
    pub path: String,
    pub calls: Calls,
    pub vendor: String,
    pub model: String,
    pub removable: bool,
    pub media_removable: bool,
    pub ejectable: bool,
    pub can_power_off: bool,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Drive")]
impl Drive {
    #[zbus(property)]
    fn vendor(&self) -> String {
        self.vendor.clone()
    }
    #[zbus(property)]
    fn model(&self) -> String {
        self.model.clone()
    }
    #[zbus(property)]
    const fn removable(&self) -> bool {
        self.removable
    }
    #[zbus(property)]
    const fn media_removable(&self) -> bool {
        self.media_removable
    }
    #[zbus(property)]
    const fn ejectable(&self) -> bool {
        self.ejectable
    }
    #[zbus(property)]
    const fn can_power_off(&self) -> bool {
        self.can_power_off
    }

    fn eject(&self, options: HashMap<String, OwnedValue>) {
        no_options(options);
        self.calls
            .lock()
            .unwrap()
            .push(format!("eject {}", self.path));
    }

    fn power_off(&self, options: HashMap<String, OwnedValue>) {
        no_options(options);
        self.calls
            .lock()
            .unwrap()
            .push(format!("power off {}", self.path));
    }
}

/// One udisks2 object and the interfaces it has.
#[derive(Clone)]
pub struct Object {
    pub path: String,
    pub block: Option<Block>,
    pub filesystem: Option<Filesystem>,
    pub partition: Option<Partition>,
    pub drive: Option<Drive>,
}

impl Object {
    fn empty(path: &str) -> Self {
        Self {
            path: path.to_owned(),
            block: None,
            filesystem: None,
            partition: None,
            drive: None,
        }
    }
}

/// A block device as the loop recordings show them: no drive, marked as
/// part of the system, nothing in it unless set.
fn block(device: &str, size: u64) -> Block {
    Block {
        device: path_bytes(device.as_bytes()),
        size,
        read_only: false,
        drive: NONE.to_owned(),
        id_usage: String::new(),
        id_type: String::new(),
        id_label: String::new(),
        hint_ignore: false,
        hint_system: true,
        hint_name: String::new(),
    }
}

fn with_filesystem(mut block: Block, kind: &str, label: &str) -> Block {
    "filesystem".clone_into(&mut block.id_usage);
    kind.clone_into(&mut block.id_type);
    label.clone_into(&mut block.id_label);
    block
}

/// A filesystem mounted at `points`; the stand-in fills in the rest when
/// it is added.
fn mounted(points: &[&[u8]]) -> Filesystem {
    Filesystem {
        path: String::new(),
        mount_points: points.iter().map(|point| path_bytes(point)).collect(),
        mounts_at: String::new(),
        fails: None,
        calls: Calls::default(),
    }
}

/// A filesystem mounted at `point`.
pub fn mounted_at(point: &[u8]) -> Filesystem {
    mounted(&[point])
}

/// A drive; the stand-in fills in its path and call log when it is added.
fn drive(vendor: &str, model: &str) -> Drive {
    Drive {
        path: String::new(),
        calls: Calls::default(),
        vendor: vendor.to_owned(),
        model: model.to_owned(),
        removable: false,
        media_removable: false,
        ejectable: false,
        can_power_off: false,
    }
}

pub const FAT_LOOP: &str = "/org/freedesktop/UDisks2/block_devices/loop0";
pub const EXT4_LOOP: &str = "/org/freedesktop/UDisks2/block_devices/loop1";
pub const GPT_LOOP: &str = "/org/freedesktop/UDisks2/block_devices/loop2";
pub const GPT_PHOTOS: &str = "/org/freedesktop/UDisks2/block_devices/loop2p1";
pub const GPT_UNNAMED: &str = "/org/freedesktop/UDisks2/block_devices/loop2p2";
pub const EMPTY_LOOP: &str = "/org/freedesktop/UDisks2/block_devices/loop3";
pub const ZRAM: &str = "/org/freedesktop/UDisks2/block_devices/zram0";
pub const STICK: &str = "/org/freedesktop/UDisks2/drives/SanDisk_Ultra_4C530001";
pub const STICK_VOLUME: &str = "/org/freedesktop/UDisks2/block_devices/sdb1";
pub const NVME: &str = "/org/freedesktop/UDisks2/drives/Samsung_SSD_980_S64DNX0R";
pub const NVME_BOOT: &str = "/org/freedesktop/UDisks2/block_devices/nvme0n1p1";
pub const NVME_ROOT: &str = "/org/freedesktop/UDisks2/block_devices/nvme0n1p2";
pub const NVME_WINDOWS: &str = "/org/freedesktop/UDisks2/block_devices/nvme0n1p3";
pub const NVME_RECOVERY: &str = "/org/freedesktop/UDisks2/block_devices/nvme0n1p4";

/// What udisksd 2.10.1 held with the loop disks set up and two of them
/// mounted, as recorded.
pub fn recorded() -> Vec<Object> {
    const MIB_64: u64 = 67_108_864;
    vec![
        Object {
            block: Some(with_filesystem(
                block("/dev/loop0", MIB_64),
                "vfat",
                "NOVASTICK",
            )),
            filesystem: Some(mounted(&[])),
            ..Object::empty(FAT_LOOP)
        },
        Object {
            block: Some(with_filesystem(
                block("/dev/loop1", MIB_64),
                "ext4",
                "Backup",
            )),
            filesystem: Some(mounted(&[b"/media/root/Backup"])),
            ..Object::empty(EXT4_LOOP)
        },
        Object {
            block: Some(block("/dev/loop2", 134_217_728)),
            ..Object::empty(GPT_LOOP)
        },
        Object {
            block: Some(with_filesystem(
                block("/dev/loop2p1", 50_331_648),
                "ext4",
                "Photos",
            )),
            filesystem: Some(mounted(&[b"/media/root/Photos"])),
            partition: Some(Partition {
                number: 1,
                name: "Photos".to_owned(),
                table: GPT_LOOP.to_owned(),
            }),
            ..Object::empty(GPT_PHOTOS)
        },
        Object {
            block: Some(with_filesystem(
                block("/dev/loop2p2", 59_768_832),
                "ext4",
                "",
            )),
            filesystem: Some(mounted(&[])),
            partition: Some(Partition {
                number: 2,
                name: String::new(),
                table: GPT_LOOP.to_owned(),
            }),
            ..Object::empty(GPT_UNNAMED)
        },
        Object {
            block: Some(block("/dev/loop3", 0)),
            ..Object::empty(EMPTY_LOOP)
        },
        Object {
            block: Some(Block {
                hint_ignore: true,
                ..block("/dev/zram0", 0)
            }),
            ..Object::empty(ZRAM)
        },
    ]
}

/// A USB stick with one FAT volume, mounted where udisks2 mounts it.
pub fn stick() -> Vec<Object> {
    vec![
        Object {
            drive: Some(Drive {
                removable: true,
                can_power_off: true,
                ..drive("SanDisk", "Ultra")
            }),
            ..Object::empty(STICK)
        },
        Object {
            block: Some(Block {
                drive: STICK.to_owned(),
                hint_system: false,
                ..with_filesystem(block("/dev/sdb1", 30_752_636_928), "vfat", "STICK")
            }),
            filesystem: Some(mounted(&[b"/run/media/me/STICK"])),
            partition: Some(Partition {
                number: 1,
                name: String::new(),
                table: "/org/freedesktop/UDisks2/block_devices/sdb".to_owned(),
            }),
            ..Object::empty(STICK_VOLUME)
        },
    ]
}

/// A laptop's `NVMe` disk: the boot and root volumes the system runs from,
/// an unmounted Windows partition, and a recovery partition a system rule
/// hides.
pub fn laptop_disk() -> Vec<Object> {
    let system = |device: &str, kind: &str| Block {
        drive: NVME.to_owned(),
        ..with_filesystem(block(device, 1_073_741_824), kind, "")
    };
    vec![
        Object {
            drive: Some(drive("", "Samsung SSD 980 1TB")),
            ..Object::empty(NVME)
        },
        Object {
            block: Some(system("/dev/nvme0n1p1", "vfat")),
            filesystem: Some(mounted(&[b"/boot"])),
            ..Object::empty(NVME_BOOT)
        },
        Object {
            block: Some(system("/dev/nvme0n1p2", "ext4")),
            filesystem: Some(mounted(&[b"/", b"/nix/store"])),
            ..Object::empty(NVME_ROOT)
        },
        Object {
            block: Some(Block {
                drive: NVME.to_owned(),
                ..with_filesystem(block("/dev/nvme0n1p3", 214_748_364_800), "ntfs", "Windows")
            }),
            filesystem: Some(mounted(&[])),
            ..Object::empty(NVME_WINDOWS)
        },
        Object {
            block: Some(Block {
                drive: NVME.to_owned(),
                hint_ignore: true,
                ..with_filesystem(block("/dev/nvme0n1p4", 1_073_741_824), "ntfs", "Recovery")
            }),
            filesystem: Some(mounted(&[])),
            ..Object::empty(NVME_RECOVERY)
        },
    ]
}

/// udisks2 on a private bus.
pub struct StandIn {
    connection: Connection,
    calls: Calls,
}

impl StandIn {
    /// Serves `objects` on the bus at `address` and takes udisks2's name.
    pub fn start(address: &str, objects: &[Object]) -> Self {
        let connection = Builder::address(address)
            .unwrap()
            .serve_at(UDISKS_PATH, zbus::fdo::ObjectManager)
            .unwrap()
            .build()
            .unwrap();
        let stand_in = Self {
            connection,
            calls: Calls::default(),
        };
        for object in objects {
            stand_in.add(object);
        }
        stand_in.connection.request_name(UDISKS).unwrap();
        stand_in
    }

    /// Adds `object`, as when a drive is plugged in.
    pub fn add(&self, object: &Object) {
        let server = self.connection.object_server();
        let path = object.path.as_str();
        if let Some(block) = &object.block {
            server.at(path, block.clone()).unwrap();
        }
        if let Some(filesystem) = &object.filesystem {
            let filesystem = Filesystem {
                path: path.to_owned(),
                mounts_at: format!("/run/media/me/{}", path.rsplit('/').next().unwrap()),
                calls: Arc::clone(&self.calls),
                ..filesystem.clone()
            };
            server.at(path, filesystem).unwrap();
        }
        if let Some(partition) = &object.partition {
            server.at(path, partition.clone()).unwrap();
        }
        if let Some(drive) = &object.drive {
            let drive = Drive {
                path: path.to_owned(),
                calls: Arc::clone(&self.calls),
                ..drive.clone()
            };
            server.at(path, drive).unwrap();
        }
    }

    /// Removes the object at `path`, as when a drive is unplugged.
    pub fn remove(&self, path: &str) {
        let server = self.connection.object_server();
        server.remove::<Filesystem, _>(path).ok();
        server.remove::<Partition, _>(path).ok();
        server.remove::<Drive, _>(path).ok();
        server.remove::<Block, _>(path).ok();
    }

    /// Mounts or unmounts the volume at `path`, as udisks2 reports it.
    pub fn set_mount_points(&self, path: &str, points: &[&[u8]]) {
        let filesystem = self
            .connection
            .object_server()
            .interface::<_, Filesystem>(path)
            .unwrap();
        filesystem.get_mut().mount_points = mounted(points).mount_points;
        zbus::block_on(
            filesystem
                .get()
                .mount_points_changed(filesystem.signal_emitter()),
        )
        .unwrap();
    }

    /// Stops serving the object manager while keeping udisks2's name, so
    /// reading the objects fails as when udisks2 is stuck.
    pub fn stop_answering(&self) {
        self.connection
            .object_server()
            .remove::<zbus::fdo::ObjectManager, _>(UDISKS_PATH)
            .unwrap();
    }

    /// Says the block device at `path` changed, without changing it.
    pub fn poke(&self, path: &str) {
        let nothing: HashMap<String, zbus::zvariant::Value<'_>> = HashMap::new();
        self.connection
            .emit_signal(
                None::<()>,
                path,
                "org.freedesktop.DBus.Properties",
                "PropertiesChanged",
                &(
                    "org.freedesktop.UDisks2.Block",
                    nothing,
                    Vec::<String>::new(),
                ),
            )
            .unwrap();
    }

    /// Takes udisks2's name without serving anything, as a udisks2 that
    /// can't answer yet.
    pub fn mute(address: &str) -> Self {
        let connection = Builder::address(address).unwrap().build().unwrap();
        connection.request_name(UDISKS).unwrap();
        Self {
            connection,
            calls: Calls::default(),
        }
    }

    /// The calls made so far, in order.
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    /// Lets udisks2's name go, as when it stops.
    pub fn stop(self) {
        drop(self.connection);
    }
}

/// `objects` with the filesystem at `path` answering `failure` to every
/// mount and unmount.
pub fn failing(mut objects: Vec<Object>, path: &str, failure: &Failure) -> Vec<Object> {
    for object in &mut objects {
        if object.path == path
            && let Some(filesystem) = &mut object.filesystem
        {
            filesystem.fails = Some(failure.clone());
        }
    }
    objects
}
