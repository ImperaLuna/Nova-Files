//! The trash's `directorysizes` cache: one line per trashed folder, its
//! size in bytes, its info file's modified time in seconds, and its name
//! in `files/`, escaped as info files escape paths. Other file managers
//! read it to show a trashed folder's size without walking it.
//!
//! A folder's line replaces any older line for its name. Lines are
//! added by writing the whole cache anew beside it and
//! renaming that over it (see `private_file`), once per job and trash,
//! so a reader never sees half a line. Another
//! app writing at the same moment may lose its line or ours; it is only
//! a cache, and a reader walks a folder it doesn't find there.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;
use std::time::UNIX_EPOCH;

use super::info::escape;
use crate::private_file;

/// The cache's file, in the trash's folder.
const FILE: &str = "directorysizes";
/// A line's fields: size, modified time, name.
const FIELDS: usize = 3;
/// The most of an existing cache kept: a cache bigger than this is
/// started anew. 4 MiB.
const MAX_BYTES: usize = 4 * 1024 * 1024;

/// The size of the folder at `folder` with all it holds, links counted
/// as links, or `None` when `go_on` says to stop first.
///
/// # Errors
/// When a folder in it can't be read.
pub fn folder_size(folder: &Path, go_on: &dyn Fn() -> bool) -> io::Result<Option<u64>> {
    let mut size = 0;
    let mut pending = vec![folder.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(&folder)? {
            if !go_on() {
                return Ok(None);
            }
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                size += metadata.size();
            }
        }
    }
    Ok(Some(size))
}

/// Adds each of `folders` (its name in the trash at `root`, its info
/// file, and its size in bytes) to the trash's cache.
///
/// # Errors
/// When the cache can't be written, or an info file can't be looked at.
pub fn add_folder_sizes(root: &Path, folders: &[(&OsStr, &Path, u64)]) -> io::Result<()> {
    let path = root.join(FILE);
    let cache = match private_file::read(&path, MAX_BYTES) {
        private_file::Read::Bytes(bytes) => bytes,
        // Started anew: it is only a cache.
        private_file::Read::Missing | private_file::Read::TooBig => Vec::new(),
        private_file::Read::Failed(error) => return Err(error),
    };
    let mut lines = Vec::new();
    for &(name, info, size) in folders {
        let modified = match fs::symlink_metadata(info).and_then(|there| there.modified()) {
            Ok(modified) => modified
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
            // Gone from the trash already (restored, or emptied): no line.
            Err(error) => {
                log::info!("{} has no size line: {error}", info.display());
                continue;
            }
        };
        let mut line = format!("{size} {modified} ").into_bytes();
        line.extend(escape(name.as_bytes()));
        lines.push(line);
    }
    // A name's old line, from an entry gone since, makes way for its new.
    let renamed: Vec<&[u8]> = lines.iter().map(|line| name_of(line)).collect();
    let mut cache: Vec<u8> = cache
        .split(|&byte| byte == b'\n')
        .filter(|line| !line.is_empty() && !renamed.contains(&name_of(line)))
        .flat_map(|line| line.iter().copied().chain(*b"\n"))
        .collect();
    for line in lines {
        cache.extend(line);
        cache.push(b'\n');
    }
    private_file::replace(&path, &cache, private_file::FILE_MODE)
}

/// The escaped name a cache line ends with: what follows its second
/// space.
fn name_of(line: &[u8]) -> &[u8] {
    line.splitn(FIELDS, |&byte| byte == b' ')
        .nth(FIELDS - 1)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;
    use crate::ops::TestFolder;

    #[test]
    fn a_folder_counts_its_files_and_links_but_not_what_links_point_to() {
        let folder = TestFolder::new("trash-size");
        fs::create_dir_all(folder.path().join("tree/sub")).unwrap();
        fs::write(folder.path().join("tree/a"), "12345").unwrap();
        fs::write(folder.path().join("tree/sub/b"), "123").unwrap();
        let big = folder.file("big", &"x".repeat(1000));
        symlink(&big, folder.path().join("tree/link")).unwrap();
        let link_size = fs::symlink_metadata(folder.path().join("tree/link"))
            .unwrap()
            .size();

        let size = folder_size(&folder.path().join("tree"), &|| true).unwrap();

        assert_eq!(size, Some(8 + link_size));
    }

    #[test]
    fn a_stopped_walk_has_no_size() {
        let folder = TestFolder::new("trash-size-stop");
        fs::create_dir(folder.path().join("tree")).unwrap();
        fs::write(folder.path().join("tree/a"), "a").unwrap();

        assert_eq!(
            folder_size(&folder.path().join("tree"), &|| false).unwrap(),
            None
        );
    }

    #[test]
    fn each_folder_adds_a_line_to_the_cache() {
        let folder = TestFolder::new("trash-sizes");
        let info = folder.file("a.trashinfo", "");
        let modified = fs::metadata(&info)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // Another app's line, and an old line for the same name, without
        // the last newline.
        fs::write(folder.path().join(FILE), "1 2 other\n3 4 My%20Folder").unwrap();
        let gone = folder.path().join("gone.trashinfo");

        add_folder_sizes(
            folder.path(),
            &[
                (OsStr::new("My Folder"), &info, 42),
                (OsStr::new("gone"), &gone, 7),
            ],
        )
        .unwrap();

        let cache = fs::read_to_string(folder.path().join(FILE)).unwrap();
        assert_eq!(cache, format!("1 2 other\n42 {modified} My%20Folder\n"));
        let left: Vec<_> = fs::read_dir(folder.path()).unwrap().collect();
        assert_eq!(left.len(), 2, "no new cache is left behind");
    }
}
