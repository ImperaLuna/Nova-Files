//! An index being built in memory, and saving it.
//!
//! The crawl adds rows in depth-first order with [`Built::push`] and
//! [`Built::close`]s each folder once everything inside it is added, which
//! records where the folder's run ends. [`Built::save`] writes the whole
//! file to a temp file in the same folder and renames it over the old one:
//! readers that mapped the old file keep reading it, and new readers get
//! the new one, never a mix. The file isn't synced to disk first; after a
//! crash a short or damaged file fails [`crate::Index::open`]'s checks and
//! is crawled again.

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, BufWriter, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

use crate::format::{self, Header, NO_PARENT, Row};
use crate::{FileId, Flags, Kind, RowId, Stamp};

/// The index file and its folder are the user's alone.
const FILE_MODE: u32 = 0o600;
const FOLDER_MODE: u32 = 0o700;
/// Added to the index file's name for the file being written.
const TEMP_SUFFIX: &str = ".new";
/// Beside the index; held while a save writes the temp file and renames
/// it, so two saves, even from two processes, never share the temp file.
const LOCK_SUFFIX: &str = ".lock";

/// What the crawl learned about one entry, apart from its name and place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    pub kind: Kind,
    pub flags: Flags,
    pub id: FileId,
    pub size: u64,
    pub modified: Option<Stamp>,
    pub created: Option<Stamp>,
}

/// The index can't hold more: more rows than a row number counts, more
/// name bytes than a name offset reaches, or a name longer than its length
/// field. Far past any home folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooBig;

/// A whole index, in memory, ready to save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    rows: Vec<Row>,
    names: Vec<u8>,
    crawled: Option<Stamp>,
}

impl Built {
    /// An empty index for a crawl that started at `crawled`.
    pub(crate) const fn new(crawled: Option<Stamp>) -> Self {
        Self {
            rows: Vec::new(),
            names: Vec::new(),
            crawled,
        }
    }

    /// Adds a row under `parent` (`None` only for the first row). Until
    /// it is closed, a folder's run ends right after it.
    pub(crate) fn push(
        &mut self,
        parent: Option<RowId>,
        name: &[u8],
        facts: Facts,
    ) -> Result<RowId, TooBig> {
        let at = u32::try_from(self.rows.len()).map_err(|_| TooBig)?;
        let end = at
            .checked_add(1)
            .filter(|&end| end != NO_PARENT)
            .ok_or(TooBig)?;
        let name_at = u32::try_from(self.names.len()).map_err(|_| TooBig)?;
        let name_len = u16::try_from(name.len()).map_err(|_| TooBig)?;
        name_at.checked_add(u32::from(name_len)).ok_or(TooBig)?;
        self.names.extend_from_slice(name);
        self.rows.push(Row {
            parent: parent.map_or(NO_PARENT, |RowId(row)| row),
            end,
            name_at,
            name_len,
            kind: Some(facts.kind),
            flags: facts.flags.0,
            id: facts.id,
            size: facts.size,
            modified: format::stored(facts.modified),
            created: format::stored(facts.created),
        });
        Ok(RowId(at))
    }

    /// Ends `folder`'s run after the last row added so far.
    pub(crate) fn close(&mut self, RowId(folder): RowId) {
        let end = u32::try_from(self.rows.len()).unwrap_or(NO_PARENT);
        if let Some(row) = usize::try_from(folder)
            .ok()
            .and_then(|at| self.rows.get_mut(at))
        {
            row.end = end;
        }
    }

    /// Marks `row` with `flags`, on top of what it has.
    pub(crate) fn flag(&mut self, RowId(row): RowId, flags: Flags) {
        if let Some(row) = usize::try_from(row)
            .ok()
            .and_then(|at| self.rows.get_mut(at))
        {
            row.flags = Flags(row.flags).with(flags).0;
        }
    }

    /// How many rows it has.
    pub const fn len(&self) -> usize {
        self.rows.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The whole file, as [`Built::save`] writes it.
    fn write_to(&self, out: &mut impl io::Write) -> io::Result<()> {
        let rows = u64::try_from(self.rows.len()).map_err(io::Error::other)?;
        let names_len = u64::try_from(self.names.len()).map_err(io::Error::other)?;
        let header = Header::current(rows, names_len, format::stored(self.crawled));
        out.write_all(&header.write())?;
        for row in &self.rows {
            out.write_all(&row.write())?;
        }
        out.write_all(&self.names)
    }

    /// Writes the index to `path`, replacing what is there in one step,
    /// and makes its folder if it is missing.
    ///
    /// # Errors
    /// When the folder or file can't be written, for example on a full or
    /// read-only disk, or [`io::ErrorKind::WouldBlock`] when another save
    /// of the same index is running: waiting could take as long as that
    /// save, or forever if its process hangs. The old index, if any, is
    /// left as it was.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(folder) = path.parent() {
            DirBuilder::new()
                .recursive(true)
                .mode(FOLDER_MODE)
                .create(folder)?;
        }
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(FILE_MODE)
            .open(beside(path, LOCK_SUFFIX))?;
        lock.try_lock()?;
        let temp = beside(path, TEMP_SUFFIX);
        let written = self
            .write_temp(&temp)
            .and_then(|()| fs::rename(&temp, path));
        if written.is_err() {
            fs::remove_file(&temp).ok();
        }
        written
    }

    fn write_temp(&self, temp: &Path) -> io::Result<()> {
        // A temp file left by a crash is stale: the lock keeps out other
        // saves.
        // If it can't be removed, creating the new one says why.
        fs::remove_file(temp).ok();
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(FILE_MODE)
            .open(temp)?;
        let mut out = BufWriter::new(file);
        self.write_to(&mut out)?;
        out.flush()
    }
}

fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(kind: Kind) -> Facts {
        Facts {
            kind,
            flags: Flags::default(),
            id: FileId {
                device: 1,
                inode: 2,
            },
            size: 3,
            modified: None,
            created: Some(Stamp(4)),
        }
    }

    #[test]
    fn closing_a_folder_ends_its_run_after_the_last_row() {
        let mut built = Built::new(None);
        let root = built.push(None, b"/home/a", facts(Kind::Folder)).unwrap();
        let folder = built
            .push(Some(root), b"docs", facts(Kind::Folder))
            .unwrap();
        let file = built
            .push(Some(folder), b"a.txt", facts(Kind::File))
            .unwrap();
        built.close(folder);
        built.close(root);
        assert_eq!(built.rows[0].end, 3);
        assert_eq!(built.rows[1].end, 3);
        assert_eq!(built.rows[2].end, 3);
        assert_eq!(built.rows[2].parent, folder.0);
        assert_eq!(built.rows[0].parent, NO_PARENT);
        assert_eq!(file, RowId(2));
        assert_eq!(built.len(), 3);
        assert!(!built.is_empty());
    }

    #[test]
    fn a_name_longer_than_its_length_field_is_refused() {
        let mut built = Built::new(None);
        let long = vec![b'a'; usize::from(u16::MAX) + 1];
        assert_eq!(built.push(None, &long, facts(Kind::File)), Err(TooBig));
        assert!(built.is_empty());
        assert_eq!(built.names, Vec::<u8>::new());
    }

    #[test]
    fn flags_add_to_what_a_row_has() {
        let mut built = Built::new(None);
        let row = built.push(None, b"/", facts(Kind::Folder)).unwrap();
        built.flag(row, Flags::UNREADABLE);
        built.flag(row, Flags::OTHER_DRIVE);
        assert_eq!(
            Flags(built.rows[0].flags),
            Flags::UNREADABLE.with(Flags::OTHER_DRIVE)
        );
    }

    #[test]
    fn the_temp_and_lock_files_sit_next_to_the_index() {
        let index = Path::new("/cache/nova/index");
        assert_eq!(
            beside(index, TEMP_SUFFIX),
            Path::new("/cache/nova/index.new")
        );
        assert_eq!(
            beside(index, LOCK_SUFFIX),
            Path::new("/cache/nova/index.lock")
        );
    }
}
