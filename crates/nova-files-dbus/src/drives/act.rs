//! Mounting, unmounting and ejecting through udisks2, for the core (see
//! [`nova_files_api::DriveService::act`]).
//!
//! Each action connects to the system bus for itself and waits up to
//! [`ACTION_TIMEOUT`] for udisks2: mounting may ask the person for their
//! password through polkit, and that dialog waits on them. udisks2 picks
//! where a volume mounts (under `/run/media/<user>/`) and its options.
//!
//! Ejecting makes a drive safe to unplug: every filesystem on it is
//! unmounted, stopping at the first that won't (`Busy` when something
//! still has files open on it), then the drive's media is ejected if it
//! can be, then the drive is switched off if it can be. A drive holding a
//! system volume is never switched off.
//!
//! udisks2's errors become [`DriveFailure`]s by their D-Bus names.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use nova_files_api::{DriveAction, DriveFailure, DriveId, VolumeId};
use zbus::blocking::fdo::ObjectManagerProxy;
use zbus::blocking::{Connection, Proxy, proxy};
use zbus::proxy::CacheProperties;
use zbus::zvariant::{OwnedObjectPath, Value};

use super::objects::{self, DRIVE, FILESYSTEM};
use super::{UDISKS, UDISKS_PATH};
use crate::{Bus, connect_within_timeout};

/// The longest one action may take, a polkit password dialog included.
pub const ACTION_TIMEOUT: Duration = Duration::from_mins(2);

const MOUNT: &str = "Mount";
const UNMOUNT: &str = "Unmount";
const EJECT: &str = "Eject";
const POWER_OFF: &str = "PowerOff";

/// Error names, and what each means for a client.
const BUSY: [&str; 1] = ["org.freedesktop.UDisks2.Error.DeviceBusy"];
/// Mounting a volume that is mounted already, or unmounting one that
/// isn't: what was asked is already so.
const ALREADY_MOUNTED: &str = "org.freedesktop.UDisks2.Error.AlreadyMounted";
const NOT_MOUNTED: &str = "org.freedesktop.UDisks2.Error.NotMounted";
const MOUNT_POINTS: &str = "MountPoints";
const NOT_AUTHORIZED: [&str; 2] = [
    "org.freedesktop.UDisks2.Error.NotAuthorized",
    "org.freedesktop.UDisks2.Error.NotAuthorizedCanObtain",
];
const CANCELLED: [&str; 2] = [
    "org.freedesktop.UDisks2.Error.NotAuthorizedDismissed",
    "org.freedesktop.UDisks2.Error.Cancelled",
];
const GONE: [&str; 3] = [
    "org.freedesktop.DBus.Error.UnknownObject",
    "org.freedesktop.DBus.Error.UnknownInterface",
    "org.freedesktop.DBus.Error.UnknownMethod",
];
const NO_SERVICE: [&str; 2] = [
    "org.freedesktop.DBus.Error.ServiceUnknown",
    "org.freedesktop.DBus.Error.NameHasNoOwner",
];
const TIMED_OUT: [&str; 2] = [
    "org.freedesktop.DBus.Error.NoReply",
    "org.freedesktop.DBus.Error.Timeout",
];

/// No options: udisks2's defaults.
type Options<'a> = HashMap<&'a str, Value<'a>>;

/// Does `action` with udisks2 on `bus`; a mount answers with where the
/// volume is now.
pub fn act(bus: Bus, action: &DriveAction) -> Result<Option<PathBuf>, DriveFailure> {
    let connection =
        connect_within_timeout(bus, |builder| Ok(builder.method_timeout(ACTION_TIMEOUT)))
            .map_err(|_| DriveFailure::NoService)?;
    match action {
        DriveAction::Mount(volume) => mount(&connection, volume).map(Some),
        DriveAction::Unmount(volume) => unmount(&connection, &object(&volume.0)?).map(|()| None),
        DriveAction::Eject(drive) => eject(&connection, drive).map(|()| None),
    }
}

/// Mounts `volume`; one mounted already answers with where it is.
fn mount(connection: &Connection, volume: &VolumeId) -> Result<PathBuf, DriveFailure> {
    let path = object(&volume.0)?;
    let filesystem = proxy(connection, &path, FILESYSTEM)?;
    match filesystem.call::<_, _, String>(MOUNT, &(Options::new(),)) {
        Ok(point) => Ok(PathBuf::from(point)),
        Err(error) if is(&error, ALREADY_MOUNTED) => {
            let points: Vec<Vec<u8>> = filesystem.get_property(MOUNT_POINTS).map_err(failure)?;
            let point = points.into_iter().next().ok_or_else(|| failure(error))?;
            Ok(objects::path_from_bytes(point))
        }
        Err(error) => Err(failure(error)),
    }
}

/// Unmounts `volume`; one not mounted is left as it is.
fn unmount(connection: &Connection, volume: &OwnedObjectPath) -> Result<(), DriveFailure> {
    match proxy(connection, volume, FILESYSTEM)?.call::<_, _, ()>(UNMOUNT, &(Options::new(),)) {
        Err(error) if !is(&error, NOT_MOUNTED) => Err(failure(error)),
        Ok(()) | Err(_) => Ok(()),
    }
}

/// Whether `error` is the D-Bus error `name`.
fn is(error: &zbus::Error, name: &str) -> bool {
    matches!(error, zbus::Error::MethodError(error_name, _, _) if error_name.as_str() == name)
}

fn eject(connection: &Connection, drive: &DriveId) -> Result<(), DriveFailure> {
    let path = object(&drive.0)?;
    let objects = ObjectManagerProxy::builder(connection)
        .destination(UDISKS)
        .and_then(|builder| builder.path(UDISKS_PATH))
        .and_then(|builder| builder.cache_properties(CacheProperties::No).build())
        .and_then(|udisks| udisks.get_managed_objects().map_err(zbus::Error::from))
        .map_err(failure)?;
    let plan = objects::eject_plan(&objects, path.as_str())?;
    for volume in &plan.mounted {
        unmount(connection, volume)?;
    }
    let drive = proxy(connection, &path, DRIVE)?;
    if plan.eject {
        drive
            .call::<_, _, ()>(EJECT, &(Options::new(),))
            .map_err(failure)?;
    }
    if plan.power_off {
        drive
            .call::<_, _, ()>(POWER_OFF, &(Options::new(),))
            .map_err(failure)?;
    }
    Ok(())
}

/// udisks2's object at `path`, which a client handed over; one that can't
/// be an object path names nothing there.
fn object(path: &str) -> Result<OwnedObjectPath, DriveFailure> {
    OwnedObjectPath::try_from(path).map_err(|_| DriveFailure::Gone)
}

fn proxy<'a>(
    connection: &Connection,
    path: &'a OwnedObjectPath,
    interface: &'static str,
) -> Result<Proxy<'a>, DriveFailure> {
    proxy::Builder::new(connection)
        .destination(UDISKS)
        .and_then(|builder| builder.path(path.as_ref()))
        .and_then(|builder| builder.interface(interface))
        .and_then(|builder| builder.cache_properties(CacheProperties::No).build())
        .map_err(failure)
}

/// What `error` means for a client, by its D-Bus name.
fn failure(error: zbus::Error) -> DriveFailure {
    let zbus::Error::MethodError(name, text, _) = &error else {
        return match error {
            zbus::Error::InputOutput(_) => DriveFailure::NoService,
            other => DriveFailure::Failed(other.to_string()),
        };
    };
    let name = name.as_str();
    if BUSY.contains(&name) {
        DriveFailure::Busy
    } else if NOT_AUTHORIZED.contains(&name) {
        DriveFailure::NotAuthorized
    } else if CANCELLED.contains(&name) {
        DriveFailure::Cancelled
    } else if GONE.contains(&name) {
        DriveFailure::Gone
    } else if NO_SERVICE.contains(&name) {
        DriveFailure::NoService
    } else if TIMED_OUT.contains(&name) {
        DriveFailure::TimedOut
    } else {
        DriveFailure::Failed(text.clone().unwrap_or_else(|| name.to_owned()))
    }
}
