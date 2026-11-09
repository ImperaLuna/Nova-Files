//! Putting a file or folder under a new name without ever replacing what
//! already has that name.
//!
//! The kernel does it in one step with `renameat2(RENAME_NOREPLACE)`. Some
//! filesystems (some FUSE ones, NFS, exFAT) refuse the flag. Then a file is
//! hard-linked to its new name, which fails on a taken name just the same,
//! and its old name removed. A folder can't be linked, nor can anything on
//! a filesystem without links; for those the new name is checked and then
//! renamed to, which leaves a moment in which another program could take
//! the name, so it is logged.
//!
//! On a filesystem that ignores case, a new name that differs from the old
//! only in case finds the entry itself taking it. That is no clash: the
//! entry is renamed in place.

use std::fs::{self, Metadata};
use std::io;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;

use rustix::fs::{AtFlags, CWD, RenameFlags, linkat, renameat_with, unlinkat};
use rustix::io::Errno;

/// Renames `from` to `to`, failing with [`io::ErrorKind::AlreadyExists`]
/// when something else has the name `to`.
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    rename_or_fall_back(from, to, |from, to| {
        renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE)
    })
}

/// Tries `rename`, and the fallbacks when it says the filesystem refuses.
fn rename_or_fall_back(
    from: &Path,
    to: &Path,
    rename: impl FnOnce(&Path, &Path) -> rustix::io::Result<()>,
) -> io::Result<()> {
    match rename(from, to) {
        Err(errno) if refuses_flag(errno) => link_or_check(from, to),
        Err(Errno::EXIST) if is_case_change(from, to) => fs::rename(from, to),
        result => result.map_err(io::Error::from),
    }
}

/// Whether `errno` says the filesystem can't rename without replacing,
/// rather than that this rename is wrong.
const fn refuses_flag(errno: Errno) -> bool {
    matches!(errno, Errno::INVAL | Errno::NOSYS | Errno::OPNOTSUPP)
}

/// The fallbacks, in order: a link, then a check.
fn link_or_check(from: &Path, to: &Path) -> io::Result<()> {
    match linkat(CWD, from, CWD, to, AtFlags::empty()) {
        Ok(()) => unlink_or_take_back(from, to),
        // Taken, or no links here: the check tells which.
        Err(_) => check_then_rename(from, to),
    }
}

/// Removes `from`, now linked to `to`. When it can't be removed the link
/// is taken back, so the file doesn't end up under both names.
fn unlink_or_take_back(from: &Path, to: &Path) -> io::Result<()> {
    let Err(errno) = unlinkat(CWD, from, AtFlags::empty()) else {
        return Ok(());
    };
    if let Err(stuck) = unlinkat(CWD, to, AtFlags::empty()) {
        log::warn!(
            "{} is left under the second name {}: {stuck}",
            from.display(),
            to.display()
        );
    }
    Err(errno.into())
}

/// Renames `from` to `to` once `to` is found free, or to be `from` itself.
fn check_then_rename(from: &Path, to: &Path) -> io::Result<()> {
    // A name that can't be looked at can't be renamed to either; the
    // rename says why.
    if to.symlink_metadata().is_ok() && !is_case_change(from, to) {
        return Err(io::ErrorKind::AlreadyExists.into());
    }
    log::warn!(
        "{} can't be renamed without replacing on its filesystem; checked {} is free first",
        from.display(),
        to.display()
    );
    fs::rename(from, to)
}

/// Whether `to` is `from` under a name that differs only in case, as a
/// filesystem that ignores case finds it.
fn is_case_change(from: &Path, to: &Path) -> bool {
    let (Some(old), Some(new)) = (from.file_name(), to.file_name()) else {
        return false;
    };
    let same_name = match (old.to_str(), new.to_str()) {
        (Some(old), Some(new)) => old.to_lowercase() == new.to_lowercase(),
        _ => old
            .as_encoded_bytes()
            .eq_ignore_ascii_case(new.as_encoded_bytes()),
    };
    same_name
        && from.parent() == to.parent()
        && match (from.symlink_metadata(), to.symlink_metadata()) {
            (Ok(from), Ok(to)) => same_entry(&from, &to),
            _ => false,
        }
}

fn same_entry(a: &Metadata, b: &Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(test)]
mod tests;
