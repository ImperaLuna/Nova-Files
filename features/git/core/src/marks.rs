use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::{BufRead as _, BufReader, Read};
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};

use nova_files_git_api::{Mark, Marks};

use crate::git_dir::git_dir;
use crate::status::{HEADER, Headers, status_in};

const MORE: [&str; 3] = ["-z", "--ignored=matching", "--"];
const LITERAL: &str = ":(literal)";
const HERE: &str = ".";
const ORDINARY: u8 = b'1';
const RENAMED: u8 = b'2';
const UNMERGED: u8 = b'u';
const UNTRACKED: u8 = b'?';
const IGNORED: u8 = b'!';
const ORDINARY_FIELDS: usize = 9;
const RENAMED_FIELDS: usize = 10;
const UNMERGED_FIELDS: usize = 11;
const SHORT_FIELDS: usize = 2;

pub fn repo_top(folder: &Path) -> Option<(PathBuf, PathBuf)> {
    folder
        .ancestors()
        .find_map(|top| git_dir(top).map(|dir| (top.to_path_buf(), dir)))
}

pub fn marks_at(folder: &Path) -> Option<Marks> {
    let (top, git_dir) = repo_top(folder)?;
    let inside = folder.strip_prefix(&top).ok()?.to_path_buf();
    let spec = if inside.as_os_str().is_empty() {
        OsString::from(HERE)
    } else {
        let mut spec = OsString::from(LITERAL);
        spec.push(&inside);
        spec
    };
    let mut more: Vec<&OsStr> = MORE.iter().map(OsStr::new).collect();
    more.push(&spec);
    status_in(&top, &git_dir, &more, move |output| parse(output, &inside))?
}

pub fn parse(output: impl Read, inside: &Path) -> Option<Marks> {
    let mut headers = Headers::default();
    let mut entries: BTreeMap<OsString, Mark> = BTreeMap::new();
    let mut records = BufReader::new(output).split(0);
    while let Some(Ok(record)) = records.next() {
        if let Some(header) = record.strip_prefix(HEADER) {
            headers.take(header);
            continue;
        }
        let Some(&kind) = record.first() else {
            continue;
        };
        let fields = match kind {
            ORDINARY => ORDINARY_FIELDS,
            RENAMED => {
                records.next();
                RENAMED_FIELDS
            }
            UNMERGED => UNMERGED_FIELDS,
            UNTRACKED | IGNORED => SHORT_FIELDS,
            _ => continue,
        };
        let Some(path) = record.splitn(fields, |&byte| byte == b' ').nth(fields - 1) else {
            continue;
        };
        if let Some((name, mark)) = entry_mark(kind, path, inside) {
            let kept = entries.entry(name).or_insert(mark);
            *kept = (*kept).max(mark);
        }
    }
    let changed = entries.values().any(|mark| *mark != Mark::Ignored);
    Some(Marks {
        repo: headers.repo(changed)?,
        entries,
    })
}

fn entry_mark(kind: u8, path: &[u8], inside: &Path) -> Option<(OsString, Mark)> {
    let path = Path::new(OsStr::from_bytes(path));
    let rest = path.strip_prefix(inside).ok()?.as_os_str().as_bytes();
    let rest = rest.strip_suffix(b"/").unwrap_or(rest);
    let (name, deeper) = rest
        .iter()
        .position(|&byte| byte == b'/')
        .map_or((rest, false), |at| (&rest[..at], true));
    if name.is_empty() {
        return None;
    }
    let mark = match (kind, deeper) {
        (IGNORED, true) => return None,
        (IGNORED, false) => Mark::Ignored,
        (UNTRACKED, false) => Mark::New,
        _ => Mark::Changed,
    };
    Some((OsString::from_vec(name.to_vec()), mark))
}
