use std::path::PathBuf;

use crate::running::Running;
use crate::stop::Stop;

pub type Moved = (Result<(), Stop>, Vec<PathBuf>);

pub trait Mover: Send + Sync {
    fn move_pairs(&self, running: &mut Running, pairs: &[(PathBuf, PathBuf)]) -> Moved;
    fn delete(&self, running: &mut Running, paths: &[PathBuf]) -> Moved;
}
