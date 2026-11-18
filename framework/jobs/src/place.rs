use std::fs::{self, Metadata, OpenOptions};
use std::io;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use rustix::fs::{AtFlags, CWD, RenameFlags, linkat, renameat, renameat_with, unlinkat};
use rustix::io::Errno;

use super::folders::At;

pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    rename_no_replace_at(At::path(from), to)
}

pub fn rename_no_replace_at(from: At<'_>, to: &Path) -> io::Result<()> {
    rename_or_fall_back(from, to, |from, to| {
        renameat_with(from.folder, from.name, CWD, to, RenameFlags::NOREPLACE)
    })
}

fn rename_over(from: At<'_>, to: &Path) -> io::Result<()> {
    Ok(renameat(from.folder, from.name, CWD, to)?)
}

fn rename_or_fall_back(
    from: At<'_>,
    to: &Path,
    rename: impl FnOnce(At<'_>, &Path) -> rustix::io::Result<()>,
) -> io::Result<()> {
    match rename(from, to) {
        Err(errno) if refuses_flag(errno) => link_or_check(from, to),
        Err(Errno::EXIST) if is_case_change(from, to) => rename_over(from, to),
        result => result.map_err(io::Error::from),
    }
}

pub fn replace(new: &Path, old: &Path, backup: &Path) -> io::Result<PathBuf> {
    replace_or_fall_back(new, old, backup, |new, old| {
        renameat_with(CWD, new, CWD, old, RenameFlags::EXCHANGE)
    })
}

fn replace_or_fall_back(
    new: &Path,
    old: &Path,
    backup: &Path,
    exchange: impl Fn(&Path, &Path) -> rustix::io::Result<()>,
) -> io::Result<PathBuf> {
    let ours = new.symlink_metadata()?;
    match exchange(new, old) {
        Ok(()) if is_folder(new) => {
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
        Ok(()) => Ok(new.to_path_buf()),
        Err(errno) if refuses_flag(errno) => replace_through(At::path(new), old, backup),
        Err(errno) => Err(errno.into()),
    }
}

pub fn replace_through(new: At<'_>, old: &Path, backup: &Path) -> io::Result<PathBuf> {
    rename_no_replace(old, backup)?;
    if is_folder(backup) {
        return Err(taken_back(rename_no_replace(backup, old), old, backup));
    }
    if let Err(error) = rename_no_replace_at(new, old) {
        return Err(match rename_no_replace(backup, old) {
            Ok(()) => error,
            Err(stuck) => left_at(old, backup, &error, &stuck),
        });
    }
    Ok(backup.to_path_buf())
}

fn is_folder(path: &Path) -> bool {
    path.symlink_metadata().is_ok_and(|there| there.is_dir())
}

fn taken_back(back: io::Result<()>, old: &Path, at: &Path) -> io::Error {
    let taken = io::ErrorKind::AlreadyExists.into();
    match back {
        Ok(()) => taken,
        Err(stuck) => left_at(old, at, &taken, &stuck),
    }
}

fn left_at(old: &Path, at: &Path, error: &io::Error, stuck: &io::Error) -> io::Error {
    io::Error::other(format!(
        "{} is left at {} ({error}, then {stuck})",
        old.display(),
        at.display()
    ))
}

// Some filesystems (some FUSE ones, NFS, exFAT) refuse RENAME_NOREPLACE with these.
// https://man7.org/linux/man-pages/man2/rename.2.html
const fn refuses_flag(errno: Errno) -> bool {
    matches!(errno, Errno::INVAL | Errno::NOSYS | Errno::OPNOTSUPP)
}

fn link_or_check(from: At<'_>, to: &Path) -> io::Result<()> {
    match linkat(from.folder, from.name, CWD, to, AtFlags::empty()) {
        Ok(()) => unlink_or_take_back(from, to),
        Err(_) => check_then_rename(from, to),
    }
}

fn unlink_or_take_back(from: At<'_>, to: &Path) -> io::Result<()> {
    let still_linked = match (from.look(), to.symlink_metadata()) {
        (Ok(from), Ok(to)) => same_entry(&from, &to),
        _ => false,
    };
    let unlinked = if still_linked {
        unlinkat(from.folder, from.name, AtFlags::empty())
    } else {
        Err(Errno::AGAIN)
    };
    let Err(errno) = unlinked else {
        return Ok(());
    };
    if let Err(stuck) = unlinkat(CWD, to, AtFlags::empty()) {
        log::warn!(
            "{} is left under the second name {}: {stuck}",
            from.shown.display(),
            to.display()
        );
    }
    Err(errno.into())
}

fn check_then_rename(from: At<'_>, to: &Path) -> io::Result<()> {
    if is_case_change(from, to) {
        return rename_over(from, to);
    }
    let folder = from.look()?.is_dir();
    if folder {
        fs::create_dir(to)?;
    } else {
        OpenOptions::new().write(true).create_new(true).open(to)?;
    }
    log::warn!(
        "{} can't be renamed without replacing on its filesystem; took {} empty first",
        from.shown.display(),
        to.display()
    );
    let renamed = rename_over(from, to);
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

fn is_case_change(from: At<'_>, to: &Path) -> bool {
    let (Some(old), Some(new)) = (from.name.file_name(), to.file_name()) else {
        return false;
    };
    let same_name = match (old.to_str(), new.to_str()) {
        (Some(old), Some(new)) => old.to_lowercase() == new.to_lowercase(),
        _ => old
            .as_encoded_bytes()
            .eq_ignore_ascii_case(new.as_encoded_bytes()),
    };
    same_name
        && to.parent().is_some_and(|folder| from.is_in(folder))
        && match (from.look(), to.symlink_metadata()) {
            (Ok(from), Ok(to)) => same_entry(&from, &to),
            _ => false,
        }
}

pub fn same_entry(a: &Metadata, b: &Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(test)]
mod tests;
