use std::io;
use std::path::PathBuf;

use crate::control::{Cancelled, Control};

pub fn still_there(paths: &[PathBuf], control: &Control) -> Result<Vec<PathBuf>, Cancelled> {
    let mut there = Vec::with_capacity(paths.len());
    for path in paths {
        control.checkpoint()?;
        let gone = matches!(
            path.symlink_metadata(),
            Err(error) if error.kind() == io::ErrorKind::NotFound
        );
        if !gone {
            there.push(path.clone());
        }
    }
    Ok(there)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_files_test_folder::TestFolder;

    #[test]
    fn a_resumed_job_cancelled_while_its_paths_are_looked_at_stops() {
        let folder = TestFolder::new("still-there-cancel");
        let control = Control::default();
        control.cancel();

        assert_eq!(
            still_there(&[folder.file("file", "f")], &control),
            Err(Cancelled)
        );
    }

    #[test]
    fn a_path_it_cant_look_at_stays_and_a_gone_one_goes() {
        let folder = TestFolder::new("still-there");
        let file = folder.file("file", "f");
        let unreachable = file.join("x");
        let gone = folder.path().join("gone");

        let left = still_there(
            &[file.clone(), unreachable.clone(), gone],
            &Control::default(),
        )
        .unwrap();

        assert_eq!(left, vec![file, unreachable]);
    }
}
