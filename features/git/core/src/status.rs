use std::env;
use std::io;
use std::io::{BufRead as _, BufReader, Read};
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use nova_files_git_api::{Head, Repo};

use crate::git_dir::git_dir;
use crate::repo_config::runs_programs;

const GIT: &str = "git";
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
const GIT_ENV: &[u8] = b"GIT_";
const READ_WAIT: Duration = Duration::from_secs(10);
const READER_THREAD: &str = "nova-files-git-read";
const HEADER: &[u8] = b"# ";
const OID: &str = "branch.oid ";
const BRANCH: &str = "branch.head ";
const AHEAD_BEHIND: &str = "branch.ab ";
const DETACHED: &str = "(detached)";
const SHORT_ID: usize = 7;

pub fn repo_at(folder: &Path) -> Option<Repo> {
    let git_dir = git_dir(folder)?;
    if runs_programs(&git_dir) {
        log::info!(
            "{}: its git config runs programs, not read",
            folder.display()
        );
        return None;
    }
    match read_status(folder) {
        Ok(repo) => repo,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            log::debug!("no {GIT} program: {error}");
            None
        }
        Err(error) => {
            log::warn!("git status in {}: {error}", folder.display());
            None
        }
    }
}

fn git_command(folder: &Path) -> Command {
    let mut command = Command::new(GIT);
    for (name, _) in env::vars_os() {
        if name.as_bytes().starts_with(GIT_ENV) {
            command.env_remove(name);
        }
    }
    command.args(SAFE).arg("-C").arg(folder).args(STATUS);
    command
}

fn read_status(folder: &Path) -> io::Result<Option<Repo>> {
    let mut child = git_command(folder)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let answer = read_output(&mut child);
    end(&mut child)?;
    answer
}

fn read_output(child: &mut Child) -> io::Result<Option<Repo>> {
    let output = child.stdout.take().ok_or(io::ErrorKind::BrokenPipe)?;
    let (done, read) = mpsc::channel();
    thread::Builder::new()
        .name(READER_THREAD.to_owned())
        .spawn(move || {
            done.send(parse(output)).ok();
        })?;
    read.recv_timeout(READ_WAIT)
        .map_err(|_| io::ErrorKind::TimedOut.into())
}

fn end(child: &mut Child) -> io::Result<()> {
    child.kill()?;
    child.wait()?;
    Ok(())
}

pub fn parse(output: impl Read) -> Option<Repo> {
    let mut oid = None;
    let mut branch = None;
    let (mut ahead, mut behind) = (0, 0);
    let mut changed = false;
    for line in BufReader::new(output).split(b'\n') {
        let Ok(line) = line else { break };
        let Some(header) = line.strip_prefix(HEADER) else {
            changed = true;
            break;
        };
        let header = String::from_utf8_lossy(header);
        if let Some(id) = header.strip_prefix(OID) {
            oid = Some(id.chars().take(SHORT_ID).collect::<String>());
        } else if let Some(name) = header.strip_prefix(BRANCH) {
            branch = Some(name.to_owned());
        } else if let Some(counts) = header.strip_prefix(AHEAD_BEHIND) {
            (ahead, behind) = ahead_behind(counts);
        }
    }
    let head = match branch? {
        name if name == DETACHED => Head::Detached(oid?),
        name => Head::Branch(name),
    };
    Some(Repo {
        head,
        changed,
        ahead,
        behind,
    })
}

fn ahead_behind(counts: &str) -> (u32, u32) {
    let mut count = counts
        .split(' ')
        .map(|count| count.trim_start_matches(['+', '-']).parse().unwrap_or(0));
    (count.next().unwrap_or(0), count.next().unwrap_or(0))
}
