//! A job's plan: everything a copy or move will make, or a delete will
//! remove, found before it changes anything. Its totals are known up front, and since links are never
//! followed and nothing is read twice, a tree that holds a link to itself
//! can't make it loop.
//!
//! Items come folder first, then what the folder holds, so each folder
//! exists before anything goes into it.

use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use nova_files_api::Failure;

use super::control::Control;
use super::stop::Stop;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Folder,
    File {
        size: u64,
    },
    Link,
    /// A socket, device or pipe: the copy asks to skip it when it gets
    /// there.
    Special,
}

/// One thing to make: `to`, a copy of `from`. In a delete's plan, `to`
/// is `from`, the thing to remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub from: PathBuf,
    pub to: PathBuf,
    pub kind: Kind,
    /// Which entry `from` was when planned.
    pub entry: Entry,
}

/// Which entry on the disk a path names: its device and inode, which a
/// new entry under the same name doesn't share.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Entry {
    pub device: u64,
    pub inode: u64,
}

impl Entry {
    /// The entry `metadata` describes.
    pub fn of(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub items: Vec<Item>,
    /// The size of every file, holes included.
    pub bytes: u64,
}

/// Plans copying each of `sources` into `into`, under its own name.
///
/// # Errors
/// [`Stop`] when cancelled, or when [`check`] refuses, or a source can't
/// be read.
pub fn plan(sources: &[PathBuf], into: &Path, control: &Control) -> Result<Plan, Stop> {
    check(sources, into)?;
    let mut plan = Plan::default();
    for source in sources {
        if let Some(name) = source.file_name() {
            add_tree(&mut plan, source, &into.join(name), control, &|_| true)?;
        }
    }
    Ok(plan)
}

/// Checks that each of `sources` can go into `into` before anything is
/// changed, so a bad one fails the job with nothing done.
///
/// # Errors
/// [`Stop`] when a path isn't absolute, or a source has no name, isn't
/// there, or is a folder `into` is inside of.
pub fn check(sources: &[PathBuf], into: &Path) -> Result<(), Stop> {
    if !into.is_absolute() {
        return Err(Stop::because(into, Failure::InvalidName));
    }
    for source in sources {
        if !source.is_absolute() || source.file_name().is_none() {
            return Err(Stop::because(source, Failure::InvalidName));
        }
        if into.starts_with(source) {
            return Err(Stop::because(source, Failure::IntoItself));
        }
        source
            .symlink_metadata()
            .map_err(|error| Stop::at(source, &error))?;
    }
    Ok(())
}

/// Plans deleting each of `paths`, folders with all they hold. Each item's
/// `to` is its `from`; a folder comes before what it holds, so the items
/// are deleted last to first. A path `wanted` refuses is left out with
/// all it holds, unread.
///
/// # Errors
/// [`Stop`] when cancelled, or when a path isn't absolute or has no name,
/// or can't be read.
pub fn plan_delete(
    paths: &[PathBuf],
    control: &Control,
    wanted: &dyn Fn(&Path) -> bool,
) -> Result<Plan, Stop> {
    let mut plan = Plan::default();
    for path in paths {
        if !path.is_absolute() || path.file_name().is_none() {
            return Err(Stop::because(path, Failure::InvalidName));
        }
        add_tree(&mut plan, path, path, control, wanted)?;
    }
    Ok(plan)
}

/// Adds `from` and, for a folder, all it holds, but for what `wanted`
/// refuses.
fn add_tree(
    plan: &mut Plan,
    from: &Path,
    to: &Path,
    control: &Control,
    wanted: &dyn Fn(&Path) -> bool,
) -> Result<(), Stop> {
    let mut pending = vec![(from.to_path_buf(), to.to_path_buf())];
    while let Some((from, to)) = pending.pop() {
        control.checkpoint()?;
        if !wanted(&from) {
            continue;
        }
        let metadata = from
            .symlink_metadata()
            .map_err(|error| Stop::at(&from, &error))?;
        let kind = kind(&metadata);
        if let Kind::File { size } = kind {
            plan.bytes += size;
        }
        if kind == Kind::Folder {
            let mut children = Vec::new();
            for entry in fs::read_dir(&from).map_err(|error| Stop::at(&from, &error))? {
                let name = entry.map_err(|error| Stop::at(&from, &error))?.file_name();
                children.push((from.join(&name), to.join(&name)));
            }
            // Reversed, so the stack hands them out in the folder's order.
            pending.extend(children.into_iter().rev());
        }
        let entry = Entry::of(&metadata);
        plan.items.push(Item {
            from,
            to,
            kind,
            entry,
        });
    }
    Ok(())
}

/// What `metadata` describes.
pub fn kind(metadata: &fs::Metadata) -> Kind {
    let file_type = metadata.file_type();
    if file_type.is_dir() {
        Kind::Folder
    } else if file_type.is_file() {
        Kind::File {
            size: metadata.size(),
        }
    } else if file_type.is_symlink() {
        Kind::Link
    } else {
        Kind::Special
    }
}

#[cfg(test)]
mod tests;
