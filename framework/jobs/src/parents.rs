use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::running::Running;
use crate::stop::Stop;

pub fn make_folders(
    running: &Running,
    targets: &[&Path],
    touched: &mut BTreeSet<PathBuf>,
) -> Result<(), Stop> {
    for parent in targets.iter().filter_map(|target| target.parent()) {
        let restored = targets.iter().any(|other| parent.starts_with(other));
        if restored || parent.symlink_metadata().is_ok() {
            continue;
        }
        running.shared.control.checkpoint()?;
        let first = parent.ancestors().find(|above| {
            above
                .parent()
                .is_none_or(|up| up.symlink_metadata().is_ok())
        });
        fs::create_dir_all(parent).map_err(|error| Stop::at(parent, &error))?;
        touched.extend(first.and_then(Path::parent).map(Path::to_path_buf));
    }
    Ok(())
}
