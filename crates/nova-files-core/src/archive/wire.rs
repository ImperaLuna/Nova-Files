use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, TimeZone as _};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    File,
    Folder,
    Link,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub kind: ArchiveKind,
    pub locked: bool,
    pub size: u64,
    pub modified: i64,
    pub mode: u32,
    pub path: PathBuf,
    pub link: PathBuf,
}

pub const MAX_ENTRIES: usize = 1_000_000;
const FIELDS: usize = 8;
const END: u8 = 0;
const MODE_BITS: u32 = 0o7777;

pub fn entries(bytes: &[u8]) -> Option<Vec<ArchiveEntry>> {
    let body = match bytes.split_last() {
        None => return Some(Vec::new()),
        Some((&END, body)) => body,
        Some(_) => return None,
    };
    let mut fields = body.split(|&byte| byte == END);
    let mut entries = Vec::new();
    loop {
        let Some(kind) = fields.next() else {
            return Some(entries);
        };
        if entries.len() == MAX_ENTRIES {
            return None;
        }
        let mut record = [kind; FIELDS];
        for slot in &mut record[1..] {
            *slot = fields.next()?;
        }
        let entry = entry(&record)?;
        if stays_inside(&entry.path) {
            entries.push(entry);
        }
    }
}

fn entry(record: &[&[u8]; FIELDS]) -> Option<ArchiveEntry> {
    let &[kind, locked, size, modified, clock, mode, path, link] = record;
    let kind = match kind {
        b"f" => ArchiveKind::File,
        b"d" => ArchiveKind::Folder,
        b"l" => ArchiveKind::Link,
        _ => return None,
    };
    let locked = match locked {
        b"0" => false,
        b"1" => true,
        _ => return None,
    };
    let modified = number(modified)?;
    let modified = match clock {
        b"u" => modified,
        b"l" => from_local(modified),
        _ => return None,
    };
    let mode = number::<u32>(mode)?;
    if mode & !MODE_BITS != 0 {
        return None;
    }
    Some(ArchiveEntry {
        kind,
        locked,
        size: number(size)?,
        modified,
        mode,
        path: PathBuf::from(OsStr::from_bytes(path)),
        link: PathBuf::from(OsStr::from_bytes(link)),
    })
}

fn from_local(wall: i64) -> i64 {
    DateTime::from_timestamp(wall, 0)
        .and_then(|utc| Local.from_local_datetime(&utc.naive_utc()).earliest())
        .map_or(wall, |local| local.timestamp())
}

fn number<T: std::str::FromStr>(field: &[u8]) -> Option<T> {
    std::str::from_utf8(field).ok()?.parse().ok()
}

fn stays_inside(path: &Path) -> bool {
    let bytes = path.as_os_str().as_bytes();
    !bytes.is_empty()
        && bytes
            .split(|&byte| byte == b'/')
            .all(|part| !part.is_empty() && part != b"." && part != b"..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(path: &[u8]) -> Vec<u8> {
        [b"f".as_slice(), b"0", b"3", b"-1", b"u", b"420", path, b""]
            .iter()
            .flat_map(|field| field.iter().copied().chain([END]))
            .collect()
    }

    #[test]
    fn reads_each_field() {
        let bytes = b"l\x001\x0012\x001700000000\x00u\x00511\x00a/b\xff\x00../t\x00";
        assert_eq!(
            entries(bytes),
            Some(vec![ArchiveEntry {
                kind: ArchiveKind::Link,
                locked: true,
                size: 12,
                modified: 1_700_000_000,
                mode: 0o777,
                path: PathBuf::from(OsStr::from_bytes(b"a/b\xff")),
                link: PathBuf::from("../t"),
            }])
        );
        let folder = entries(b"d\x000\x000\x00-1\x00u\x00493\x00d\x00\x00").unwrap();
        assert_eq!(folder[0].kind, ArchiveKind::Folder);
        assert!(!folder[0].locked);
        assert_eq!(folder[0].modified, -1);
        assert_eq!(entries(&record(b"x")).unwrap()[0].kind, ArchiveKind::File);
    }

    #[test]
    fn local_wall_time_takes_the_system_zone() {
        let wall = 1_700_000_000;
        let bytes = b"f\x000\x000\x001700000000\x00l\x00420\x00a\x00\x00";
        let read = entries(bytes).unwrap()[0].modified;
        let offset = Local
            .timestamp_opt(read, 0)
            .unwrap()
            .offset()
            .local_minus_utc();
        assert_eq!(read + i64::from(offset), wall);
    }

    #[test]
    fn nothing_is_an_empty_listing() {
        assert_eq!(entries(b""), Some(Vec::new()));
    }

    #[test]
    fn leaves_out_paths_that_reach_outside() {
        let mut bytes = Vec::new();
        for path in [
            &b"/etc/passwd"[..],
            b"../up",
            b"a/../../up",
            b"a//b",
            b"./a",
            b"a/.",
            b"ok",
        ] {
            bytes.extend(record(path));
        }
        let kept: Vec<PathBuf> = entries(&bytes)
            .unwrap()
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        assert_eq!(kept, vec![PathBuf::from("ok")]);
    }

    #[test]
    fn anything_off_the_layout_is_unreadable() {
        let good = record(b"a");
        assert_eq!(entries(&good[..good.len() - 1]), None);
        assert_eq!(entries(b"f\x000\x00"), None);
        for bad in [
            &b"x\x000\x003\x00-1\x00u\x00420\x00a\x00\x00"[..],
            b"f\x002\x003\x00-1\x00u\x00420\x00a\x00\x00",
            b"f\x000\x00-3\x00-1\x00u\x00420\x00a\x00\x00",
            b"f\x000\x003\x00x\x00u\x00420\x00a\x00\x00",
            b"f\x000\x003\x00-1\x00z\x00420\x00a\x00\x00",
            b"f\x000\x003\x00-1\x00u\x0070000\x00a\x00\x00",
        ] {
            assert_eq!(entries(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn more_than_the_cap_is_unreadable() {
        let one = record(b"a");
        assert_eq!(
            entries(&one.repeat(MAX_ENTRIES)).map(|all| all.len()),
            Some(MAX_ENTRIES)
        );
        assert_eq!(entries(&one.repeat(MAX_ENTRIES + 1)), None);
    }
}
