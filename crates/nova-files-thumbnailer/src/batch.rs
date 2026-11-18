//! The batch the core asks for, read from the command line.
//!
//! A thumbnail name comes from the core but is still checked: it must be a
//! plain file name, so a write can't leave the thumbnails folder. The size
//! is one of the spec's, named by its folder.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

pub const USAGE: &str = "usage: nova-files-thumbnailer <thumbnails folder> <normal|large> \
                          (<file> <uri> <mtime> <name>)...";

/// The spec's "normal" thumbnails fit in this square.
const NORMAL_SIDE: u32 = 128;
/// The spec's "large" thumbnails fit in this square.
const LARGE_SIDE: u32 = 256;

/// One of the freedesktop spec's thumbnail sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Normal,
    Large,
}

impl Size {
    const ALL: [Self; 2] = [Self::Normal, Self::Large];

    /// The square its thumbnails fit in, in pixels.
    pub const fn side(self) -> u32 {
        match self {
            Self::Normal => NORMAL_SIDE,
            Self::Large => LARGE_SIDE,
        }
    }

    /// Its folder under the thumbnails folder, also the word that names it.
    pub const fn folder(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Large => "large",
        }
    }

    fn from_word(word: &OsString) -> Option<Self> {
        Self::ALL.into_iter().find(|size| word == size.folder())
    }
}

/// One file to make a thumbnail for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub source: PathBuf,
    /// The file's URI, stored in the thumbnail as `Thumb::URI`.
    pub uri: String,
    /// The file's mtime in whole seconds, stored as `Thumb::MTime`.
    pub mtime: String,
    /// The thumbnail's file name: the MD5 of the URI, then `.png`.
    pub name: OsString,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    /// The freedesktop thumbnails folder (`$XDG_CACHE_HOME/thumbnails`).
    pub folder: PathBuf,
    pub size: Size,
    pub jobs: Vec<Job>,
}

impl Batch {
    /// `None` when the arguments don't form a batch.
    pub fn from_args(args: impl IntoIterator<Item = OsString>) -> Option<Self> {
        let mut args = args.into_iter();
        let folder = PathBuf::from(args.next()?);
        if !folder.is_absolute() {
            return None;
        }
        let size = Size::from_word(&args.next()?)?;
        let mut jobs = Vec::new();
        while let Some(source) = args.next() {
            let uri = args.next()?.into_string().ok()?;
            let mtime = args.next()?.into_string().ok()?;
            let name = args.next()?;
            if !mtime.bytes().all(|byte| byte.is_ascii_digit()) || !is_plain_name(&name) {
                return None;
            }
            jobs.push(Job {
                source: PathBuf::from(source),
                uri,
                mtime,
                name,
            });
        }
        Some(Self { folder, size, jobs })
    }
}

fn is_plain_name(name: &OsString) -> bool {
    let mut parts = Path::new(name).components();
    matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn reads_jobs_in_fours() {
        let batch = Batch::from_args(args(&[
            "/cache/thumbnails",
            "large",
            "/a.jpg",
            "file:///a.jpg",
            "17",
            "abc.png",
        ]))
        .unwrap();
        assert_eq!(batch.folder, Path::new("/cache/thumbnails"));
        assert_eq!(batch.size, Size::Large);
        assert_eq!(batch.jobs.len(), 1);
        assert_eq!(batch.jobs[0].mtime, "17");
    }

    #[test]
    fn refuses_names_that_leave_the_folder() {
        for name in ["../x.png", "a/b.png", "/x.png", ".."] {
            let list = ["/t", "normal", "/a.jpg", "file:///a.jpg", "1", name];
            assert_eq!(Batch::from_args(args(&list)), None, "{name}");
        }
    }

    #[test]
    fn refuses_a_short_job_a_bad_mtime_or_size_or_a_relative_folder() {
        assert_eq!(
            Batch::from_args(args(&["/t", "normal", "/a.jpg", "file:///a.jpg"])),
            None
        );
        let bad_mtime = ["/t", "normal", "/a.jpg", "file:///a.jpg", "-1", "a.png"];
        assert_eq!(Batch::from_args(args(&bad_mtime)), None);
        let bad_size = ["/t", "huge", "/a.jpg", "file:///a.jpg", "1", "a.png"];
        assert_eq!(Batch::from_args(args(&bad_size)), None);
        assert_eq!(Batch::from_args(args(&["/t"])), None);
        assert_eq!(Batch::from_args(args(&["t"])), None);
        assert_eq!(Batch::from_args(args(&[])), None);
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
