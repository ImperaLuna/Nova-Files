use std::ffi::OsString;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use nova_files_api::{Failure, Outcome};
use nova_files_jobs::Ended;
use nova_files_jobs::failure::failure;
use nova_files_jobs::keep_both::{Naming, free_path};
use nova_files_jobs::running::Running;
use nova_files_jobs::stop::Stop;
use nova_files_jobs::sync::sync_folders;

const BESIDE: &str = "Link to ";

pub fn make_links(running: &mut Running, sources: &[PathBuf], into: &Path) -> Ended {
    if !into.is_absolute() {
        return failed(into, Failure::InvalidName);
    }
    for source in sources {
        if let Err(stop) = running.shared.control.checkpoint().map_err(Stop::from) {
            return (stop.outcome(), vec![into.to_path_buf()]);
        }
        let Some(wanted) = link_name(source, into) else {
            return failed(source, Failure::InvalidName);
        };
        match link_at(source, &into.join(wanted)) {
            Ok(made) => {
                running.steps.insert(source.clone(), made);
            }
            Err(error) => {
                let reason = failure(&error);
                return (
                    Outcome::Failed {
                        path: source.clone(),
                        reason,
                    },
                    vec![into.to_path_buf()],
                );
            }
        }
    }
    sync_folders([into]);
    (Outcome::Done, vec![into.to_path_buf()])
}

fn link_name(source: &Path, into: &Path) -> Option<OsString> {
    let name = source.file_name()?;
    if !source.is_absolute() {
        return None;
    }
    if source.parent() != Some(into) {
        return Some(name.to_os_string());
    }
    let mut beside = OsString::from(BESIDE);
    beside.push(name);
    Some(beside)
}

fn link_at(source: &Path, wanted: &Path) -> io::Result<PathBuf> {
    source.symlink_metadata()?;
    let naming = if source.is_dir() {
        Naming::Folder
    } else {
        Naming::File
    };
    let mut at = wanted.to_path_buf();
    loop {
        match symlink(source, &at) {
            Ok(()) => return Ok(at),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                at = free_path(wanted, naming)?;
            }
            Err(error) => return Err(error),
        }
    }
}

fn failed(path: &Path, reason: Failure) -> Ended {
    let path = path.to_path_buf();
    (Outcome::Failed { path, reason }, Vec::new())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use nova_files_test_folder::TestFolder;

    use super::*;

    #[test]
    fn links_point_at_their_files_and_never_replace_a_name() {
        let folder = TestFolder::new("links-make");
        let notes = folder.file("notes.txt", "mine");
        let into = folder.path().join("into");
        fs::create_dir(&into).unwrap();
        fs::write(into.join("notes.txt"), "theirs").unwrap();
        let mut running = Running::for_test();

        let ended = make_links(&mut running, std::slice::from_ref(&notes), &into);
        assert_eq!(ended.0, Outcome::Done);
        let made = into.join("notes (2).txt");
        assert_eq!(fs::read_link(&made).unwrap(), notes);
        assert_eq!(
            fs::read_to_string(into.join("notes.txt")).unwrap(),
            "theirs"
        );

        let beside = make_links(&mut running, std::slice::from_ref(&notes), folder.path());
        assert_eq!(beside.0, Outcome::Done);
        let link = folder.path().join("Link to notes.txt");
        assert_eq!(fs::read_link(&link).unwrap(), notes);
        assert_eq!(running.steps.get(&notes), Some(&link));
    }

    #[test]
    fn a_relative_source_or_folder_makes_nothing() {
        let folder = TestFolder::new("links-relative");
        let mut running = Running::for_test();
        let relative = make_links(&mut running, &[PathBuf::from("a")], folder.path());
        assert!(matches!(
            relative.0,
            Outcome::Failed {
                reason: Failure::InvalidName,
                ..
            }
        ));
        let into = make_links(&mut running, &[folder.path().join("a")], Path::new("rel"));
        assert!(matches!(
            into.0,
            Outcome::Failed {
                reason: Failure::InvalidName,
                ..
            }
        ));
    }

    #[test]
    fn a_source_that_isnt_there_makes_no_link() {
        let folder = TestFolder::new("links-gone");
        let gone = folder.path().join("bundle.zip").join("inner.txt");
        let into = folder.path().join("into");
        fs::create_dir(&into).unwrap();
        let mut running = Running::for_test();
        let ended = make_links(&mut running, &[gone], &into);
        assert!(matches!(
            ended.0,
            Outcome::Failed {
                reason: Failure::NotFound,
                ..
            }
        ));
        assert_eq!(fs::read_dir(&into).unwrap().count(), 0);
    }
}
