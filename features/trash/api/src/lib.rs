#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::time::SystemTime;

use nova_files_api::EntryKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trashed {
    pub item: PathBuf,
    pub original: PathBuf,
    pub deleted: Option<SystemTime>,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    List,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Listed(Vec<Trashed>),
}
