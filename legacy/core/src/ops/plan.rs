use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use nova_files_api::Failure;

use super::control::Control;
use super::stop::Stop;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Folder,
    File { size: u64 },
    Link,
    Special,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub from: PathBuf,
    pub to: PathBuf,
    pub kind: Kind,
    pub entry: Entry,
}

// A filesystem such as ext4 may give a removed entry's inode number to a new one.
// https://man7.org/linux/man-pages/man7/inode.7.html
#[derive(Debug, Clone, Copy, Default, Eq)]
pub struct Entry {
    pub device: u64,
    pub inode: u64,
    pub born: Option<u128>,
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        (self.device, self.inode) == (other.device, other.inode)
    }
}

impl Entry {
    pub fn of(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            born: metadata
                .created()
                .ok()
                .and_then(|born| born.duration_since(UNIX_EPOCH).ok())
                .map(|since| since.as_nanos()),
        }
    }

    pub const fn of_stat(stat: &rustix::fs::Stat) -> Self {
        Self {
            device: stat.st_dev,
            inode: stat.st_ino,
            born: None,
        }
    }

    pub fn is(&self, other: &Self) -> bool {
        self == other
            && match (self.born, other.born) {
                (Some(born), Some(other)) => born == other,
                _ => true,
            }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub items: Vec<Item>,
    pub bytes: u64,
}

#[cfg(test)]
pub fn plan(sources: &[PathBuf], into: &Path, control: &Control) -> Result<Plan, Stop> {
    check(sources, into)?;
    add_pairs(&pairs_into(sources, into), control)
}

pub fn pairs_into(sources: &[PathBuf], into: &Path) -> Vec<(PathBuf, PathBuf)> {
    sources
        .iter()
        .filter_map(|source| Some((source.clone(), into.join(source.file_name()?))))
        .collect()
}

pub fn plan_pairs(pairs: &[(PathBuf, PathBuf)], control: &Control) -> Result<Plan, Stop> {
    check_pairs(pairs)?;
    add_pairs(pairs, control)
}

fn add_pairs(pairs: &[(PathBuf, PathBuf)], control: &Control) -> Result<Plan, Stop> {
    let mut plan = Plan::default();
    for (source, target) in pairs {
        add_tree(&mut plan, source, target, control, &|_| true)?;
    }
    Ok(plan)
}

pub fn check_pairs(pairs: &[(PathBuf, PathBuf)]) -> Result<(), Stop> {
    for (source, target) in pairs {
        if !target.is_absolute() || target.file_name().is_none() {
            return Err(Stop::because(target, Failure::InvalidName));
        }
        if !source.is_absolute() || source.file_name().is_none() {
            return Err(Stop::because(source, Failure::InvalidName));
        }
        if target
            .parent()
            .is_some_and(|parent| parent.starts_with(source))
        {
            return Err(Stop::because(source, Failure::IntoItself));
        }
        source
            .symlink_metadata()
            .map_err(|error| Stop::at(source, &error))?;
    }
    Ok(())
}

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
