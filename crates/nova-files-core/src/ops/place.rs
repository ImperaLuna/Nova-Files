//! Putting a file or folder under a new name without ever replacing what
//! already has that name.
//!
//! The kernel does it in one step with `renameat2(RENAME_NOREPLACE)`. Some
//! filesystems (some FUSE ones, NFS, exFAT) refuse the flag. Then a file is
//! hard-linked to its new name, which fails on a taken name just the same,
//! and its old name removed, only while it still names the same file. A
//! folder can't be linked, nor can anything on a filesystem without links;
//! for those the new name is first taken with an empty entry of our own,
//! made only if the name is free (`O_EXCL`, or a new folder), and the
//! rename then replaces just that. Only a program that removed our empty
//! entry and put its own there in that moment could lose it, so the
//! fallback is logged.
//!
//! On a filesystem that ignores case, a new name that differs from the old
//! only in case finds the entry itself taking it. That is no clash: the
//! entry is renamed in place.

use std::fs::{self, Metadata, OpenOptions};
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

/// Puts `new` in the place of `old`, which is removed only once `new`
/// holds its name, and never a folder. Where the filesystem can swap two
/// names in one step it does; elsewhere `old` steps aside to `backup`
/// first, and comes back if `new` can't take its name. Should it not
/// come back either, the error says where it was left.
pub fn replace(new: &Path, old: &Path, backup: &Path) -> io::Result<()> {
    replace_or_fall_back(new, old, backup, |new, old| {
        renameat_with(CWD, new, CWD, old, RenameFlags::EXCHANGE)
    })
}

/// Tries `exchange`, and the backup route when the filesystem refuses.
fn replace_or_fall_back(
    new: &Path,
    old: &Path,
    backup: &Path,
    exchange: impl Fn(&Path, &Path) -> rustix::io::Result<()>,
) -> io::Result<()> {
    let ours = new.symlink_metadata()?;
    // A folder that took the name since it was looked at is never
    // replaced: it goes back, and the name counts as taken.
    match exchange(new, old) {
        // `new` now holds what was `old`.
        Ok(()) if is_folder(new) => {
            // Swapped back only while `old` holds what `new` put there:
            // a file saved there since is another program's, and stays.
            let still_ours = old
                .symlink_metadata()
                .is_ok_and(|there| same_entry(&ours, &there));
            let back = if still_ours {
                exchange(new, old).map_err(io::Error::from)
            } else {
                Err(io::ErrorKind::AlreadyExists.into())
            };
            Err(taken_back(back, old, new))
        }
        Ok(()) => fs::remove_file(new),
        Err(errno) if refuses_flag(errno) => {
            rename_no_replace(old, backup)?;
            if is_folder(backup) {
                return Err(taken_back(rename_no_replace(backup, old), old, backup));
            }
            if let Err(error) = rename_no_replace(new, old) {
                return Err(match rename_no_replace(backup, old) {
                    Ok(()) => error,
                    Err(stuck) => left_at(old, backup, &error, &stuck),
                });
            }
            fs::remove_file(backup)
        }
        Err(errno) => Err(errno.into()),
    }
}

/// Whether a folder, not a link to one, is at `path`.
fn is_folder(path: &Path) -> bool {
    path.symlink_metadata().is_ok_and(|there| there.is_dir())
}

/// The error for a folder at `old` that stepped to `at` and went `back`.
fn taken_back(back: io::Result<()>, old: &Path, at: &Path) -> io::Error {
    let taken = io::ErrorKind::AlreadyExists.into();
    match back {
        Ok(()) => taken,
        Err(stuck) => left_at(old, at, &taken, &stuck),
    }
}

/// What was at `old` is stuck at `at`: `error`, then `stuck` on the way
/// back. Not a name to look at again: the job ends here, and its journal
/// says where it is.
fn left_at(old: &Path, at: &Path, error: &io::Error, stuck: &io::Error) -> io::Error {
    io::Error::other(format!(
        "{} is left at {} ({error}, then {stuck})",
        old.display(),
        at.display()
    ))
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

/// Removes `from`, now linked to `to`. When it can't be removed, or no
/// longer names the linked file (another program saved a new one there
/// since), the link is taken back: the file doesn't end up under both
/// names, and a newer file at `from` is never removed.
fn unlink_or_take_back(from: &Path, to: &Path) -> io::Result<()> {
    let still_linked = match (from.symlink_metadata(), to.symlink_metadata()) {
        (Ok(from), Ok(to)) => same_entry(&from, &to),
        _ => false,
    };
    let unlinked = if still_linked {
        unlinkat(CWD, from, AtFlags::empty())
    } else {
        Err(Errno::AGAIN)
    };
    let Err(errno) = unlinked else {
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

/// Renames `from` to `to` once an empty entry of our own holds `to`, or
/// when `to` is `from` itself.
fn check_then_rename(from: &Path, to: &Path) -> io::Result<()> {
    if is_case_change(from, to) {
        return fs::rename(from, to);
    }
    let folder = from.symlink_metadata()?.is_dir();
    // Fails on a taken name, as the rename would have.
    if folder {
        fs::create_dir(to)?;
    } else {
        OpenOptions::new().write(true).create_new(true).open(to)?;
    }
    log::warn!(
        "{} can't be renamed without replacing on its filesystem; took {} empty first",
        from.display(),
        to.display()
    );
    let renamed = fs::rename(from, to);
    if renamed.is_err() {
        let removed = if folder {
            fs::remove_dir(to)
        } else {
            fs::remove_file(to)
        };
        if let Err(error) = removed {
            log::warn!("can't remove the empty {}: {error}", to.display());
        }
    }
    renamed
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
