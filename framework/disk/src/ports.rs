use std::ffi::OsStr;
use std::sync::Arc;

use nova_files_api::{EntryKind, FileIcon, FileTypes, IconKind};

pub trait Icons: Send + Sync {
    fn of(&self, name: &OsStr, kind: EntryKind) -> FileIcon;
    fn kind_name(&self, name: &OsStr, kind: EntryKind) -> Option<Arc<str>>;
    fn follow_desktop(&self, named: &[String]);
}

pub trait Mimes: FileTypes {
    fn ancestry(&self, mime: &str) -> Vec<String>;
    fn name(&self, mime: &str) -> Option<String>;
    fn icon_names(&self, mime: &str) -> Vec<String>;
    fn icon_kind(&self, mime: &str) -> IconKind;
}
