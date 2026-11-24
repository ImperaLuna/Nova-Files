use std::fs;
use std::io::{self, Read as _};
use std::path::Path;

use crate::git::{git, read};
use crate::git_dir::common_dir;

const FILES: [&str; 2] = ["config", "config.worktree"];
const LARGEST: u64 = 1 << 20;
const LIST_NAMES: [&str; 3] = ["--no-includes", "--name-only", "--list"];
const FILE: &str = "--file";
const CONFIG: &str = "config";
const RUNNING_SECTIONS: [&str; 3] = ["filter.", "include.", "includeif."];
const RUNNING_KEYS: [&str; 2] = ["core.fsmonitor", "core.attributesfile"];

pub fn runs_programs(git_dir: &Path) -> bool {
    let dirs = [Some(git_dir.to_path_buf()), common_dir(git_dir)];
    dirs.iter()
        .flatten()
        .flat_map(|dir| FILES.iter().map(move |name| dir.join(name)))
        .any(|file| match names_in(&file) {
            Ok(Some(names)) => names_a_program(&names),
            Ok(None) => false,
            Err(error) => {
                log::info!("{}: {error}", file.display());
                true
            }
        })
}

fn names_in(file: &Path) -> io::Result<Option<String>> {
    let size = match fs::metadata(file) {
        Ok(found) => found.len(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if size > LARGEST {
        return Err(io::ErrorKind::FileTooLarge.into());
    }
    let mut command = git();
    command.arg(CONFIG).arg(FILE).arg(file).args(LIST_NAMES);
    read(command, |output| {
        let mut names = String::new();
        output
            .take(LARGEST)
            .read_to_string(&mut names)
            .map(|_| names)
    })?
    .map(Some)
}

pub fn names_a_program(names: &str) -> bool {
    names.lines().map(str::to_ascii_lowercase).any(|name| {
        RUNNING_SECTIONS
            .iter()
            .any(|section| name.starts_with(section))
            || RUNNING_KEYS.contains(&name.as_str())
    })
}
