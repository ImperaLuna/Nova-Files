use std::fs::{File, FileTimes, Metadata, Permissions};
use std::io;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

use nova_files_api::MODE_BITS;
use rustix::buffer::{SpareCapacity, spare_capacity};
use rustix::fs::{XattrFlags, fgetxattr, fgetxattr_query, flistxattr, flistxattr_query, fsetxattr};
use rustix::io::Errno;

pub fn copy_mode_and_times(metadata: &Metadata, to: &File) -> io::Result<()> {
    to.set_permissions(Permissions::from_mode(metadata.mode() & MODE_BITS))?;
    let times = FileTimes::new()
        .set_accessed(metadata.accessed()?)
        .set_modified(metadata.modified()?);
    to.set_times(times)
}

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

fn list(file: &File) -> rustix::io::Result<Vec<u8>> {
    read_sized(flistxattr_query(file)?, |buffer| {
        flistxattr(file, buffer).map(drop)
    })
}

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
