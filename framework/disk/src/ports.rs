use std::ffi::OsStr;

use nova_files_api::{EntryKind, FileIcon, FileTypes};

pub trait Icons: Send + Sync {
    fn of(&self, name: &OsStr, kind: EntryKind) -> FileIcon;
    fn follow_desktop(&self, named: &[String]);
}

pub trait Mimes: FileTypes {
    fn ancestry(&self, mime: &str) -> Vec<String>;
    fn name(&self, mime: &str) -> Option<String>;
}
