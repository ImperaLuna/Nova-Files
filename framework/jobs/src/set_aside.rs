use std::path::{Path, PathBuf};

use crate::runner::Ended;
use crate::running::Running;

pub enum Aside {
    Kept(PathBuf),
    Gone,
    Nowhere,
    Refused,
}

pub trait SetAside: Send + Sync {
    fn for_job(&self) -> Box<dyn SetAsideForJob>;
}

pub trait SetAsideForJob: Send {
    fn set_aside(&mut self, running: &mut Running, at: (usize, &Path), shown: &Path) -> Aside;
    fn trash(&mut self, running: &mut Running, paths: &[PathBuf]) -> Ended;
    fn put_back(&mut self, running: &mut Running, items: &[PathBuf]) -> Option<Ended>;
}
