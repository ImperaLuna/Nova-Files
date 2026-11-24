use std::ffi::OsStr;
use std::io::{self, BufRead as _, BufReader, Read};
use std::path::Path;

use nova_files_git_api::{Head, Repo};

use crate::git::{GIT, git, read};
use crate::git_dir::git_dir;
use crate::repo_config::runs_programs;

const SAFE: [&str; 4] = [
    "-c",
    "core.fsmonitor=false",
    "-c",
    "core.untrackedCache=false",
];
const STATUS: [&str; 6] = [
    "--no-optional-locks",
    "status",
    "--porcelain=v2",
    "--branch",
    "--untracked-files=normal",
    "--ignore-submodules=all",
];
pub const HEADER: &[u8] = b"# ";
const OID: &str = "branch.oid ";
const BRANCH: &str = "branch.head ";
const AHEAD_BEHIND: &str = "branch.ab ";
const DETACHED: &str = "(detached)";
const SHORT_ID: usize = 7;

pub fn repo_at(folder: &Path) -> Option<Repo> {
    let git_dir = git_dir(folder)?;
    status_in(folder, &git_dir, &[], parse)?
}

pub fn status_in<T: Send + 'static>(
    top: &Path,
    git_dir: &Path,
    more: &[&OsStr],
    parse: impl FnOnce(std::process::ChildStdout) -> T + Send + 'static,
) -> Option<T> {
    if runs_programs(git_dir) {
        log::info!("{}: its git config runs programs, not read", top.display());
        return None;
    }
    let mut command = git();
    command
        .args(SAFE)
        .arg("-C")
        .arg(top)
        .args(STATUS)
        .args(more);
    match read(command, parse) {
        Ok(answer) => Some(answer),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            log::debug!("no {GIT} program: {error}");
            None
        }
        Err(error) => {
            log::warn!("git status in {}: {error}", top.display());
            None
        }
    }
}

#[derive(Default)]
pub struct Headers {
    oid: Option<String>,
    branch: Option<String>,
    ahead: u32,
    behind: u32,
}

impl Headers {
    pub fn take(&mut self, header: &[u8]) {
        let header = String::from_utf8_lossy(header);
        if let Some(id) = header.strip_prefix(OID) {
            self.oid = Some(id.chars().take(SHORT_ID).collect());
        } else if let Some(name) = header.strip_prefix(BRANCH) {
            self.branch = Some(name.to_owned());
        } else if let Some(counts) = header.strip_prefix(AHEAD_BEHIND) {
            (self.ahead, self.behind) = ahead_behind(counts);
        }
    }

    pub fn repo(self, changed: bool) -> Option<Repo> {
        let head = match self.branch? {
            name if name == DETACHED => Head::Detached(self.oid?),
            name => Head::Branch(name),
        };
        Some(Repo {
            head,
            changed,
            ahead: self.ahead,
            behind: self.behind,
        })
    }
}

pub fn parse(output: impl Read) -> Option<Repo> {
    let mut headers = Headers::default();
    let mut changed = false;
    for line in BufReader::new(output).split(b'\n') {
        let Ok(line) = line else { break };
        let Some(header) = line.strip_prefix(HEADER) else {
            changed = true;
            break;
        };
        headers.take(header);
    }
    headers.repo(changed)
}

fn ahead_behind(counts: &str) -> (u32, u32) {
    let mut count = counts
        .split(' ')
        .map(|count| count.trim_start_matches(['+', '-']).parse().unwrap_or(0));
    (count.next().unwrap_or(0), count.next().unwrap_or(0))
}
