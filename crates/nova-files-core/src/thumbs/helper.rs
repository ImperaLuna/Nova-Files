//! Runs the sandboxed helper on one batch and reads its answers.
//!
//! The helper gets only `PATH` from the environment and runs in a process
//! group of its own; its output is read on a thread of its own. A batch
//! has [`PER_FILE`] for each of its files; past that the whole group is
//! killed, so a thumbnailer the helper started can't outlive it. It is
//! always waited on. What it made is read from the cache, not from its
//! answers, so a helper killed part-way still counts the files it saved.

use std::env;
use std::ffi::OsString;
use std::io;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use rustix::process::{self, Pid, Signal};

/// How long each file of a batch may take; the helper gives a system
/// thumbnailer less than this, so a batch of slow files still finishes.
const PER_FILE: Duration = Duration::from_secs(15);
const PATH_VAR: &str = "PATH";
const READER_THREAD: &str = "nova-files-thumbnail-reader";

/// One file for the helper.
#[derive(Debug, Clone)]
pub struct Job {
    pub source: PathBuf,
    pub uri: String,
    pub mtime: u64,
    pub name: String,
}

/// Runs the helper on `jobs` until it exits or runs out of time.
pub fn run(helper: &Path, thumbnails: &Path, jobs: &[Job]) {
    let mut command = Command::new(helper);
    command
        .arg(thumbnails)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    if let Some(path) = env::var_os(PATH_VAR) {
        command.env(PATH_VAR, path);
    }
    for job in jobs {
        command
            .arg(&job.source)
            .arg(&job.uri)
            .arg(job.mtime.to_string())
            .arg(OsString::from(&job.name));
    }
    let Ok(mut child) = command.spawn() else {
        return;
    };
    let output = child.stdout.take();
    let (done, finished) = mpsc::channel();
    let reader = thread::Builder::new()
        .name(READER_THREAD.to_owned())
        .spawn(move || {
            if let Some(mut output) = output {
                io::copy(&mut output, &mut io::sink()).ok();
            }
            done.send(()).ok();
        });
    let deadline = PER_FILE.saturating_mul(u32::try_from(jobs.len()).unwrap_or(u32::MAX));
    let in_time = reader.is_ok() && finished.recv_timeout(deadline).is_ok();
    if !in_time {
        // The group is the helper's own: it and what it started.
        if let Some(group) = i32::try_from(child.id()).ok().and_then(Pid::from_raw) {
            process::kill_process_group(group, Signal::KILL).ok();
        }
        child.kill().ok();
    }
    child.wait().ok();
}
