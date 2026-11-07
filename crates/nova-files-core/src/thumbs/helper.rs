//! Runs the sandboxed helper on one batch and reads its answers.
//!
//! The helper gets only `PATH` from the environment, its output is read on
//! a thread of its own, and the whole batch has [`BATCH_TIMEOUT`]; a helper
//! that takes longer is killed. It is always waited on.

use std::env;
use std::ffi::OsString;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// How long one batch may take before the helper is killed.
const BATCH_TIMEOUT: Duration = Duration::from_secs(30);
const PATH_VAR: &str = "PATH";
const READER_THREAD: &str = "nova-files-thumbnail-reader";
const MADE: &str = "ok";

/// One file for the helper.
#[derive(Debug, Clone)]
pub struct Job {
    pub source: PathBuf,
    pub uri: String,
    pub mtime: u64,
    pub name: String,
}

/// Which of `jobs` the helper made a thumbnail for, in order. A helper that
/// can't start, fails or times out made none.
pub fn run(helper: &Path, thumbnails: &Path, jobs: &[Job]) -> Vec<bool> {
    let made = answers(helper, thumbnails, jobs).unwrap_or_default();
    let mut made: Vec<bool> = made.iter().map(|answer| answer == MADE).collect();
    made.resize(jobs.len(), false);
    made
}

fn answers(helper: &Path, thumbnails: &Path, jobs: &[Job]) -> Option<Vec<String>> {
    let mut command = Command::new(helper);
    command
        .arg(thumbnails)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
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
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let (send, receive) = mpsc::channel();
    let reader = thread::Builder::new()
        .name(READER_THREAD.to_owned())
        .spawn(move || {
            let mut text = String::new();
            let read = stdout.read_to_string(&mut text).map(|_| text);
            send.send(read).ok();
        });
    let text = match reader {
        Ok(_) => receive
            .recv_timeout(BATCH_TIMEOUT)
            .ok()
            .and_then(Result::ok),
        Err(_) => None,
    };
    if text.is_none() {
        child.kill().ok();
    }
    let status = child.wait().ok()?;
    let text = text?;
    status
        .success()
        .then(|| text.lines().map(str::to_owned).collect())
}
