use std::ffi::{OsStr, OsString};
use std::io::{self, BufRead, Read as _};
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Component, Path, PathBuf};

pub const USAGE: &str = "usage: nova-files-thumbnailer <thumbnails folder>, then one job per \
                         file on stdin: <normal|large> <file> <uri> <mtime> <name>, each field \
                         ending in a NUL byte";

const NORMAL_SIDE: u32 = 128;
const LARGE_SIDE: u32 = 256;
const FIELD_END: u8 = 0;
const MAX_FIELD_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Normal,
    Large,
}

impl Size {
    const ALL: [Self; 2] = [Self::Normal, Self::Large];

    pub const fn side(self) -> u32 {
        match self {
            Self::Normal => NORMAL_SIDE,
            Self::Large => LARGE_SIDE,
        }
    }

    pub const fn folder(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Large => "large",
        }
    }

    fn from_word(word: &[u8]) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|size| word == size.folder().as_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub size: Size,
    pub source: PathBuf,
    pub uri: String,
    pub mtime: String,
    pub name: OsString,
}

pub fn folder_from_args(args: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    let mut args = args.into_iter();
    let folder = PathBuf::from(args.next()?);
    (folder.is_absolute() && args.next().is_none()).then_some(folder)
}

pub fn next_job(input: &mut impl BufRead) -> io::Result<Option<Job>> {
    let Some(size) = field(input)? else {
        return Ok(None);
    };
    let mut rest = || field(input)?.ok_or_else(|| bad("a job ended early"));
    let size = Size::from_word(&size).ok_or_else(|| bad("an unknown size"))?;
    let source = PathBuf::from(OsStr::from_bytes(&rest()?));
    let uri = String::from_utf8(rest()?).map_err(|_| bad("a URI that isn't UTF-8"))?;
    let mtime = String::from_utf8(rest()?).map_err(|_| bad("an mtime that isn't UTF-8"))?;
    let name = OsStr::from_bytes(&rest()?).to_owned();
    if !mtime.bytes().all(|byte| byte.is_ascii_digit()) || !is_plain_name(&name) {
        return Err(bad("an mtime or name out of bounds"));
    }
    Ok(Some(Job {
        size,
        source,
        uri,
        mtime,
        name,
    }))
}

fn field(input: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    input
        .by_ref()
        .take(MAX_FIELD_BYTES)
        .read_until(FIELD_END, &mut bytes)?;
    match bytes.pop() {
        None => Ok(None),
        Some(FIELD_END) => Ok(Some(bytes)),
        Some(_) => Err(bad("a field without its end")),
    }
}

fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("bad job: {what}"))
}

fn is_plain_name(name: &OsStr) -> bool {
    let mut parts = Path::new(name).components();
    matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job_bytes(fields: &[&str]) -> Vec<u8> {
        fields
            .iter()
            .flat_map(|field| field.bytes().chain([FIELD_END]))
            .collect()
    }

    fn jobs(bytes: &[u8]) -> io::Result<Vec<Job>> {
        let mut input = bytes;
        let mut jobs = Vec::new();
        while let Some(job) = next_job(&mut input)? {
            jobs.push(job);
        }
        Ok(jobs)
    }

    #[test]
    fn reads_jobs_one_after_another_until_the_input_ends() {
        let mut bytes = job_bytes(&["large", "/a.jpg", "file:///a.jpg", "17", "abc.png"]);
        bytes.extend(job_bytes(&[
            "normal",
            "/b\nc.jpg",
            "file:///b%0Ac.jpg",
            "3",
            "d.png",
        ]));
        let jobs = jobs(&bytes).unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].size, Size::Large);
        assert_eq!(jobs[0].mtime, "17");
        assert_eq!(jobs[1].source, Path::new("/b\nc.jpg"));
    }

    #[test]
    fn refuses_names_that_leave_the_folder() {
        for name in ["../x.png", "a/b.png", "/x.png", ".."] {
            let bytes = job_bytes(&["normal", "/a.jpg", "file:///a.jpg", "1", name]);
            assert!(jobs(&bytes).is_err(), "{name}");
        }
    }

    #[test]
    fn refuses_a_short_job_a_bad_mtime_or_size() {
        let short = job_bytes(&["normal", "/a.jpg", "file:///a.jpg"]);
        assert!(jobs(&short).is_err());
        let bad_mtime = job_bytes(&["normal", "/a.jpg", "file:///a.jpg", "-1", "a.png"]);
        assert!(jobs(&bad_mtime).is_err());
        let bad_size = job_bytes(&["huge", "/a.jpg", "file:///a.jpg", "1", "a.png"]);
        assert!(jobs(&bad_size).is_err());
        assert!(jobs(b"normal").is_err());
    }

    #[test]
    fn the_folder_is_the_one_absolute_argument() {
        let args = |list: &[&str]| list.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(folder_from_args(args(&["/t"])), Some(PathBuf::from("/t")));
        assert_eq!(folder_from_args(args(&["t"])), None);
        assert_eq!(folder_from_args(args(&["/t", "normal"])), None);
        assert_eq!(folder_from_args(args(&[])), None);
    }

    #[test]
    fn sizes_follow_the_spec() {
        assert_eq!(
            (Size::Normal.side(), Size::Normal.folder()),
            (128, "normal")
        );
        assert_eq!((Size::Large.side(), Size::Large.folder()), (256, "large"));
    }
}
