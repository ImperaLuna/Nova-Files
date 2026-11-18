//! Making an archive. The helper writes it under a hidden part name in
//! the target folder (see `archive`); once whole, it takes its name, or
//! the next free one when that is taken.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use nova_files_api::{ConflictKind, Failure, Outcome, Packing};

use super::Ended;
use super::extract::{ask_password, steer};
use super::journal::PART_PREFIX;
use super::keep_both::take_free_name;
use super::name::is_plain_name;
use super::running::Running;
use super::stop::Stop;
use crate::archive::{ArchiveFailure, Archiver, Recipe};

/// What a compress job is asked.
pub struct Asked<'a> {
    pub sources: &'a [PathBuf],
    pub into: &'a Path,
    pub name: &'a OsStr,
    pub packing: Packing,
    pub level: u8,
    pub locked: bool,
}

/// Makes the archive `asked` names, with `archiver`.
pub fn compress(running: &mut Running, archiver: Option<&Archiver>, asked: &Asked<'_>) -> Ended {
    match make(running, archiver, asked) {
        Ok(Some(made)) => {
            running.steps.insert(made.0, made.1);
            (Outcome::Done, vec![asked.into.to_path_buf()])
        }
        Ok(None) => (Outcome::Cancelled, Vec::new()),
        Err(stop) => (stop.outcome(), Vec::new()),
    }
}

/// Makes the archive; the part file it was written as, and where it
/// went. `None` when the user skipped its password.
fn make(
    running: &mut Running,
    archiver: Option<&Archiver>,
    asked: &Asked<'_>,
) -> Result<Option<(PathBuf, PathBuf)>, Stop> {
    let target = asked.into.join(asked.name);
    if !asked.into.is_absolute() || !is_plain_name(asked.name) {
        return Err(Stop::because(&target, Failure::InvalidName));
    }
    let first = asked
        .sources
        .first()
        .ok_or_else(|| Stop::because(&target, Failure::NotFound))?;
    for source in asked.sources {
        if !source.is_absolute() || source.file_name().is_none() {
            return Err(Stop::because(source, Failure::InvalidName));
        }
        source
            .symlink_metadata()
            .map_err(|error| Stop::at(source, &error))?;
    }
    let archiver = archiver.ok_or_else(|| Stop::because(&target, Failure::Other))?;
    let (bytes, items) = totals(asked.sources, &running.shared.control)?;
    running.reporter.planned(bytes, items);
    let password = if asked.locked {
        let Some(password) = locking_password(running, first, asked.into)? else {
            return Ok(None);
        };
        Some(password)
    } else {
        None
    };
    running.shared.control.checkpoint()?;
    let part = asked
        .into
        .join(format!("{PART_PREFIX}{}-archive", running.names));
    let recipe = Recipe {
        packing: asked.packing,
        level: asked.level,
        password: password.as_deref(),
    };
    let made = archiver.compress(asked.sources, &part, &recipe, |told| steer(running, told));
    if let Err(failure) = made {
        fs::remove_file(&part).ok();
        return Err(stop_for(failure, first, &target));
    }
    match take_free_name(&part, &target) {
        Ok(placed) => Ok(Some((part, placed))),
        Err(error) => {
            fs::remove_file(&part).ok();
            Err(Stop::at(&target, &error))
        }
    }
}

/// The password to lock the archive with, asked until one is given;
/// `None` when the user skips it.
fn locking_password(
    running: &mut Running,
    first: &Path,
    into: &Path,
) -> Result<Option<String>, Stop> {
    loop {
        match ask_password(running, ConflictKind::PasswordNeeded, first, into)? {
            Some(password) if password.is_empty() => {}
            given => return Ok(given),
        }
    }
}

/// The bytes the files under `sources` hold, and how many entries there
/// are, links not followed. Held or stopped through `control` at each
/// entry.
fn totals(sources: &[PathBuf], control: &super::control::Control) -> Result<(u64, u64), Stop> {
    let mut bytes = 0_u64;
    let mut items = 0_u64;
    let mut left: Vec<PathBuf> = sources.to_vec();
    while let Some(path) = left.pop() {
        control.checkpoint()?;
        let Ok(metadata) = path.symlink_metadata() else {
            continue;
        };
        items += 1;
        if metadata.is_file() {
            bytes = bytes.saturating_add(metadata.len());
        } else if metadata.is_dir()
            && let Ok(entries) = fs::read_dir(&path)
        {
            left.extend(entries.flatten().map(|entry| entry.path()));
        }
    }
    Ok((bytes, items))
}

/// Why making `target` of sources starting with `first` stopped, from
/// the helper's `failure`.
fn stop_for(failure: ArchiveFailure, first: &Path, target: &Path) -> Stop {
    let (path, reason) = match failure {
        ArchiveFailure::Cancelled => return Stop::Cancelled,
        ArchiveFailure::Gone => (first, Failure::NotFound),
        ArchiveFailure::Denied => (first, Failure::PermissionDenied),
        ArchiveFailure::NoSpace => (target, Failure::NoSpace),
        ArchiveFailure::ReadOnly => (target, Failure::ReadOnly),
        ArchiveFailure::Broken
        | ArchiveFailure::Unsupported
        | ArchiveFailure::NotRead
        | ArchiveFailure::NotWritten
        | ArchiveFailure::NeedsPassword
        | ArchiveFailure::WrongPassword => (target, Failure::Other),
    };
    Stop::because(path, reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::test_folder::TestFolder;

    #[test]
    fn totals_count_file_bytes_and_every_entry_once() {
        let folder = TestFolder::new("compress-totals");
        fs::create_dir_all(folder.path().join("docs/inner")).unwrap();
        folder.file("docs/a.txt", "hello");
        folder.file("docs/inner/b.txt", "deep");
        let notes = folder.file("notes.txt", "notes");
        let sources = [folder.path().join("docs"), notes];
        assert_eq!(
            totals(&sources, &crate::ops::control::Control::default()),
            Ok((14, 5))
        );
    }

    #[test]
    fn says_where_and_why_it_stopped() {
        let (first, target) = (Path::new("/a/x"), Path::new("/b/x.zip"));
        let stop = |failure| stop_for(failure, first, target);
        assert_eq!(stop(ArchiveFailure::Cancelled), Stop::Cancelled);
        assert_eq!(
            stop(ArchiveFailure::Gone),
            Stop::because(first, Failure::NotFound)
        );
        assert_eq!(
            stop(ArchiveFailure::Denied),
            Stop::because(first, Failure::PermissionDenied)
        );
        assert_eq!(
            stop(ArchiveFailure::NoSpace),
            Stop::because(target, Failure::NoSpace)
        );
        assert_eq!(
            stop(ArchiveFailure::ReadOnly),
            Stop::because(target, Failure::ReadOnly)
        );
        assert_eq!(
            stop(ArchiveFailure::NotWritten),
            Stop::because(target, Failure::Other)
        );
    }
}
