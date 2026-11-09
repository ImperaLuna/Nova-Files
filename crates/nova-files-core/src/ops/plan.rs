//! A copy's plan: everything it will make, found before it changes
//! anything. Its totals are known up front, and since links are never
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

/// One thing to make: `to`, a copy of `from`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub from: PathBuf,
    pub to: PathBuf,
    pub kind: Kind,
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
/// [`Stop`] when cancelled, or when a path isn't absolute, or a source
/// can't be read, has no name, or is a folder `into` is inside of.
pub fn plan(sources: &[PathBuf], into: &Path, control: &Control) -> Result<Plan, Stop> {
    let mut plan = Plan::default();
    if !into.is_absolute() {
        return Err(Stop::because(into, Failure::InvalidName));
    }
    for source in sources {
        let (true, Some(name)) = (source.is_absolute(), source.file_name()) else {
            return Err(Stop::because(source, Failure::InvalidName));
        };
        if into.starts_with(source) {
            return Err(Stop::because(source, Failure::IntoItself));
        }
        add_tree(&mut plan, source, &into.join(name), control)?;
    }
    Ok(plan)
}

/// Adds `from` and, for a folder, all it holds.
fn add_tree(plan: &mut Plan, from: &Path, to: &Path, control: &Control) -> Result<(), Stop> {
    let mut pending = vec![(from.to_path_buf(), to.to_path_buf())];
    while let Some((from, to)) = pending.pop() {
        control.checkpoint()?;
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
        plan.items.push(Item { from, to, kind });
    }
    Ok(())
}

/// What `metadata` describes.
fn kind(metadata: &fs::Metadata) -> Kind {
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
