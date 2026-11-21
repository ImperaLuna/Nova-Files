use std::ffi::OsString;
use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use nova_files_entries_core::MeasureFolders;
use nova_files_entries_core::sizes::Told;
use nova_files_properties_api::Size;

use super::EVERY;
use super::tally::Tally;
use super::walk::{Root, device_of, walk};

pub struct FolderMeasure;

impl MeasureFolders for FolderMeasure {
    fn measure(
        &self,
        parent: &Path,
        folders: &[OsString],
        stopped: &(dyn Fn() -> bool + Sync),
        told: Told<'_>,
    ) -> Option<Vec<u64>> {
        let device = device_of(&parent.canonicalize().ok()?)?;
        let roots: Vec<Root> = folders
            .iter()
            .enumerate()
            .map(|(slot, name)| Root {
                path: parent.join(name),
                slot,
                listed_in: Some(device),
            })
            .collect();
        let tally = Tally::new(folders.len());
        let last_told = Mutex::new(Instant::now());
        let progress = || {
            let mut last = last_told.lock().unwrap_or_else(PoisonError::into_inner);
            if last.elapsed() >= EVERY {
                *last = Instant::now();
                drop(last);
                told(bytes_of(tally.take_changed()));
            }
        };
        walk(&roots, &tally, stopped, &progress);
        (!stopped()).then(|| tally.sizes().into_iter().map(|size| size.bytes).collect())
    }
}

fn bytes_of(changed: Vec<(usize, Size)>) -> Vec<(usize, u64)> {
    changed
        .into_iter()
        .map(|(slot, size)| (slot, size.bytes))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use nova_files_test_folder::TestFolder;

    use super::*;

    #[test]
    fn each_folder_counts_the_bytes_it_holds() {
        let folder = TestFolder::new("folder-measure");
        let root = folder.path();
        fs::create_dir_all(root.join("big/inner")).unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        fs::write(root.join("big/one"), [0_u8; 300]).unwrap();
        fs::write(root.join("big/inner/two"), [0_u8; 200]).unwrap();
        let told = Mutex::new(Vec::new());
        let tell = |sizes: Vec<(usize, u64)>| {
            told.lock().unwrap().extend(sizes);
        };

        let totals = FolderMeasure.measure(
            root,
            &["big".into(), "empty".into(), "gone".into()],
            &|| false,
            &tell,
        );

        assert_eq!(totals, Some(vec![500, 0, 0]));
    }

    #[test]
    fn a_folder_opened_through_a_link_measures_what_its_folders_hold() {
        let root = std::env::temp_dir().join(format!(
            "nova-files-folder-measure-link-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&root).ok();
        fs::create_dir_all(root.join("real/inner")).unwrap();
        fs::write(root.join("real/inner/one"), [0_u8; 300]).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();

        let totals =
            FolderMeasure.measure(&root.join("link"), &["inner".into()], &|| false, &|_| {});

        fs::remove_dir_all(&root).ok();
        assert_eq!(totals, Some(vec![300]));
    }

    #[test]
    fn a_stopped_measure_gives_no_totals() {
        let folder = TestFolder::new("folder-measure-stopped");

        let totals = FolderMeasure.measure(folder.path(), &["anything".into()], &|| true, &|_| {});

        assert_eq!(totals, None);
    }
}
