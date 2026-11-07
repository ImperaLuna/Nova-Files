//! Runs the sandboxed helper on one batch and reads its answers.
//!
//! The helper gets only what it needs from the environment (where programs,
//! MIME data and thumbnailers are: [`PASSED_VARS`]) and runs in a process
//! group of its own; its output is read on a thread of its own. A batch
//! has [`PER_FILE`] for each of its files; past that the helper is killed.
//! Once it is done the rest of its group is killed too, so nothing a
//! thumbnailer left running outlives the batch. It is always waited on. What it made is read from the cache, not from its
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
/// Where the helper finds the system's thumbnailers and MIME data.
const PASSED_VARS: [&str; 4] = ["PATH", "HOME", "XDG_DATA_HOME", "XDG_DATA_DIRS"];
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
    for var in PASSED_VARS {
        if let Some(value) = env::var_os(var) {
            command.env(var, value);
        }
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
    // A reader that couldn't start drops `done`, which ends the wait at once.
    thread::Builder::new()
        .name(READER_THREAD.to_owned())
        .spawn(move || {
            if let Some(mut output) = output {
                io::copy(&mut output, &mut io::sink()).ok();
            }
            done.send(()).ok();
        })
        .ok();
    let deadline = PER_FILE.saturating_mul(u32::try_from(jobs.len()).unwrap_or(u32::MAX));
    finished.recv_timeout(deadline).ok();
    // The group is the helper's own: it, if still running, and what it
    // started. Until the helper is waited on, its id can't name another
    // group.
    if let Some(group) = i32::try_from(child.id()).ok().and_then(Pid::from_raw) {
        process::kill_process_group(group, Signal::KILL).ok();
    }
    child.wait().ok();
}
