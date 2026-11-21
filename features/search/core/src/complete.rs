use std::fs::{self, DirEntry};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use nova_files_api::EventSink;
use nova_files_search_api::{CompleteFailed, Completion, Event, MAX_HITS};

const COMPLETE_THREAD: &str = "nova-files-complete";
const HOME: char = '~';
const SEPARATOR: char = '/';
const DOT: char = '.';

pub fn complete(events: &Arc<dyn EventSink>, home: Option<PathBuf>, typed: String, limit: u32) {
    let sink = Arc::clone(events);
    let limit = usize::try_from(limit.min(MAX_HITS)).unwrap_or_default();
    let asked = typed.clone();
    let spawned = thread::Builder::new()
        .name(COMPLETE_THREAD.to_owned())
        .spawn(move || {
            let event = match completions(&typed, home.as_deref(), limit) {
                Ok(entries) => Event::Completed { typed, entries },
                Err(reason) => Event::CompleteFailed { typed, reason },
            };
            sink.deliver(event).ok();
        });
    if spawned.is_err() {
        let reason = CompleteFailed::Unreadable;
        events
            .deliver(Event::CompleteFailed {
                typed: asked,
                reason,
            })
            .ok();
    }
}

fn completions(
    typed: &str,
    home: Option<&Path>,
    limit: usize,
) -> Result<Vec<Completion>, CompleteFailed> {
    let (folder, start) = folder_and_start(typed, home)?;
    let entries = match fs::read_dir(&folder) {
        Ok(entries) => entries,
        Err(error) if gone(&error) => return Ok(Vec::new()),
        Err(_) => return Err(CompleteFailed::Unreadable),
    };
    let start = start.to_lowercase();
    let dots = start.starts_with(DOT);
    let mut found: Vec<(String, Completion)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let shown = dots || !name.starts_with(DOT);
            (shown && name.starts_with(&start)).then(|| (name, completion(&entry)))
        })
        .collect();
    found.sort_unstable_by(|(name, entry), (other_name, other)| {
        other
            .folder
            .cmp(&entry.folder)
            .then_with(|| name.cmp(other_name))
            .then_with(|| entry.path.cmp(&other.path))
    });
    Ok(found
        .into_iter()
        .take(limit)
        .map(|(_, entry)| entry)
        .collect())
}

fn folder_and_start<'a>(
    typed: &'a str,
    home: Option<&Path>,
) -> Result<(PathBuf, &'a str), CompleteFailed> {
    let (folder, start) = typed.rsplit_once(SEPARATOR).unwrap_or((typed, ""));
    if typed.starts_with(SEPARATOR) {
        let folder = if folder.is_empty() {
            Path::new("/")
        } else {
            Path::new(folder)
        };
        return Ok((folder.to_path_buf(), start));
    }
    let inside = folder
        .strip_prefix(HOME)
        .filter(|inside| inside.is_empty() || inside.starts_with(SEPARATOR))
        .ok_or(CompleteFailed::NotAPath)?;
    let home = home.ok_or(CompleteFailed::NoHome)?;
    Ok((home.join(inside.trim_start_matches(SEPARATOR)), start))
}

fn gone(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

fn completion(entry: &DirEntry) -> Completion {
    let path = entry.path();
    let folder = match entry.file_type() {
        Ok(kind) if kind.is_symlink() => fs::metadata(&path).is_ok_and(|target| target.is_dir()),
        Ok(kind) => kind.is_dir(),
        Err(_) => false,
    };
    Completion { path, folder }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME_FOLDER: &str = "/home/me";

    fn split(typed: &str) -> Result<(PathBuf, &str), CompleteFailed> {
        folder_and_start(typed, Some(Path::new(HOME_FOLDER)))
    }

    #[test]
    fn a_typed_path_names_its_folder_and_how_the_name_starts() {
        let home = PathBuf::from(HOME_FOLDER);
        assert_eq!(split("~"), Ok((home.clone(), "")));
        assert_eq!(split("~/"), Ok((home.clone(), "")));
        assert_eq!(split("~/Doc"), Ok((home.clone(), "Doc")));
        assert_eq!(split("~/Documents/"), Ok((home.join("Documents"), "")));
        assert_eq!(split("~/a/b/c"), Ok((home.join("a/b"), "c")));
        assert_eq!(split("/"), Ok((PathBuf::from("/"), "")));
        assert_eq!(split("/et"), Ok((PathBuf::from("/"), "et")));
        assert_eq!(split("/etc/sy"), Ok((PathBuf::from("/etc"), "sy")));
    }

    #[test]
    fn only_paths_from_root_or_home_complete() {
        for typed in ["", "Doc", "docs/report", "~alice", "~alice/Doc", "./x"] {
            assert_eq!(split(typed), Err(CompleteFailed::NotAPath), "{typed}");
        }
        assert_eq!(folder_and_start("~/Doc", None), Err(CompleteFailed::NoHome));
        assert_eq!(
            folder_and_start("/etc", None),
            Ok((PathBuf::from("/"), "etc"))
        );
    }
}
