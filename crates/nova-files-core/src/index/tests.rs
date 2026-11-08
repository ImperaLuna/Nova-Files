//! Building and reopening an index in a temp cache folder.

use std::fs;

use super::*;

/// A cache folder and two folders to index, removed on drop.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("nova-files-index-{name}-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        for folder in ["home/docs", "bench/photos"] {
            fs::create_dir_all(root.join(folder)).unwrap();
        }
        fs::write(root.join("home/docs/a.txt"), b"a").unwrap();
        Self { root }
    }

    fn place(&self) -> IndexPlace {
        IndexPlace {
            folder: self.root.join("cache/nova-files"),
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
}

#[test]
fn a_built_index_opens_for_its_own_folder() {
    let scratch = Scratch::new("own");
    let place = scratch.place();
    let home = scratch.root.join("home");
    place.build(&home).unwrap();
    let index = place.open(&home).unwrap();
    assert_eq!(index.root(), home);
    assert!(index.rows() > 1);
}

#[test]
fn an_index_of_another_folder_is_stale() {
    let scratch = Scratch::new("stale");
    let place = scratch.place();
    place.build(&scratch.root.join("bench")).unwrap();
    assert!(matches!(
        place.open(&scratch.root.join("home")),
        Err(Unusable::Stale)
    ));
}

#[test]
fn no_index_yet_is_missing() {
    let scratch = Scratch::new("missing");
    assert!(matches!(
        scratch.place().open(&scratch.root.join("home")),
        Err(Unusable::Open(OpenError::Missing))
    ));
}

#[test]
fn failures_say_what_failed() {
    let scratch = Scratch::new("failures");
    let place = scratch.place();
    let error = place
        .build(&scratch.root.join("home/docs/a.txt"))
        .unwrap_err();
    assert_eq!(error.to_string(), "can't index the folder: NotAFolder");
    // A file where the cache folder should be.
    fs::create_dir_all(scratch.root.join("cache")).unwrap();
    fs::write(&place.folder, b"").unwrap();
    let error = place.build(&scratch.root.join("home")).unwrap_err();
    assert!(
        error.to_string().starts_with("can't save the index: "),
        "{error}"
    );
}

#[test]
fn the_files_sit_in_the_place() {
    let place = IndexPlace {
        folder: PathBuf::from("/cache/nova-files"),
    };
    assert_eq!(place.index(), Path::new("/cache/nova-files/index"));
    assert_eq!(place.changes(), Path::new("/cache/nova-files/changes"));
}

#[test]
fn the_place_is_in_the_cache_folder() {
    assert_eq!(
        IndexPlace::from_env().map(|place| place.folder),
        places::cache_home().map(|cache| cache.join(FOLDER))
    );
    assert!(IndexPlace::from_env().is_some());
}
