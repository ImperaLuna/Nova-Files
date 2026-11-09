//! What a copy keeps besides the bytes: the permission bits, the access
//! and modified times, and the extended attributes (tags such as
//! `user.xdg.tags`, a file's origin URL, ACLs).
//!
//! An attribute the target's filesystem or the user's rights refuse (a
//! `security.` label, say) is logged and left out; the copy itself counts.

use std::fs::{File, FileTimes, Metadata, Permissions};
use std::io;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

use rustix::buffer::{SpareCapacity, spare_capacity};
use rustix::fs::{XattrFlags, fgetxattr, fgetxattr_query, flistxattr, flistxattr_query, fsetxattr};
use rustix::io::Errno;

/// The permission bits a copy keeps: read, write and run for each, plus
/// set-id and sticky.
const MODE_BITS: u32 = 0o7777;

/// Gives `to` the permission bits and times of `from`.
pub fn copy_mode_and_times(metadata: &Metadata, to: &File) -> io::Result<()> {
    to.set_permissions(Permissions::from_mode(metadata.mode() & MODE_BITS))?;
    let times = FileTimes::new()
        .set_accessed(metadata.accessed()?)
        .set_modified(metadata.modified()?);
    to.set_times(times)
}

/// Copies every extended attribute of `from` it can onto `to`.
///
/// # Errors
/// When the attributes of `from` can't be read. A filesystem without them
/// isn't an error.
pub fn copy_xattrs(from: &File, to: &File) -> io::Result<()> {
    let names = match list(from) {
        Ok(names) => names,
        Err(Errno::OPNOTSUPP) => return Ok(()),
        Err(errno) => return Err(errno.into()),
    };
    for name in names
        .split(|&byte| byte == b'\0')
        .filter(|name| !name.is_empty())
    {
        let value = read_sized(fgetxattr_query(from, name)?, |buffer| {
            fgetxattr(from, name, buffer).map(drop)
        })?;
        if let Err(errno) = fsetxattr(to, name, &value, XattrFlags::empty()) {
            log::debug!(
                "left out the attribute {} on a copy: {errno}",
                String::from_utf8_lossy(name)
            );
        }
    }
    Ok(())
}

/// The names of the attributes of `file`, each ended by a NUL byte.
fn list(file: &File) -> rustix::io::Result<Vec<u8>> {
    read_sized(flistxattr_query(file)?, |buffer| {
        flistxattr(file, buffer).map(drop)
    })
}

/// Reads `size` bytes through `read`. Nothing is read for none: an empty
/// buffer would ask the system for the size again.
fn read_sized(
    size: usize,
    read: impl FnOnce(SpareCapacity<'_, u8>) -> rustix::io::Result<()>,
) -> rustix::io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(size);
    if size > 0 {
        read(spare_capacity(&mut bytes))?;
    }
    Ok(bytes)
}
