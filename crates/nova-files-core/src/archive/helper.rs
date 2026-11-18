//! Runs the helper once and reads what it writes.
//!
//! The helper gets an empty environment and nothing on its standard
//! input. It runs in a process group of its own. Its standard output is
//! read on a thread of its own, up to [`MAX_OUTPUT`]. The whole run,
//! reading and exiting, has [`RUN_TIME`]. Once the helper has exited or
//! the time is up, its group is killed, while the helper is not yet
//! waited on and so its id still names only that group; then it is waited
//! on. How it ended is its exit code, the numbers the helper's `ending`
//! uses.

use std::io::{self, Read};
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::process::{self, Pid, PidfdFlags, Signal};

use super::ArchiveFailure;

/// How long one run may take, start to exit.
const RUN_TIME: Duration = Duration::from_mins(10);
/// The most output read from one run; more counts as broken.
const MAX_OUTPUT: u64 = 67_108_864;
const READER_THREAD: &str = "nova-files-archive-reader";
const LIST: &str = "list";
const DONE: i32 = 0;
const BROKEN: i32 = 4;
const UNSUPPORTED: i32 = 5;
const GONE: i32 = 6;
const DENIED: i32 = 7;

/// What the helper wrote listing `archive`.
pub fn list(helper: &Path, archive: &Path) -> Result<Vec<u8>, ArchiveFailure> {
    let deadline = Instant::now() + RUN_TIME;
    let mut child = Command::new(helper)
        .arg(LIST)
        .arg(archive)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|_| ArchiveFailure::NotRead)?;
    let output = read_output(&mut child, deadline);
    let status = end(child, deadline);
    ended(status)?;
    output
}

/// What the helper's exit `status` says of its run.
fn ended(status: Option<ExitStatus>) -> Result<(), ArchiveFailure> {
    Err(match status.and_then(|status| status.code()) {
        Some(DONE) => return Ok(()),
        Some(BROKEN) => ArchiveFailure::Broken,
        Some(UNSUPPORTED) => ArchiveFailure::Unsupported,
        Some(GONE) => ArchiveFailure::Gone,
        Some(DENIED) => ArchiveFailure::Denied,
        _ => ArchiveFailure::NotRead,
    })
}

/// All `reader` holds, or `None` inside when it holds more than `cap`
/// bytes.
fn capped(reader: impl Read, cap: u64) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    reader.take(cap.saturating_add(1)).read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= cap).then_some(bytes))
}

/// Everything the helper writes: broken when that is more than
/// [`MAX_OUTPUT`], not read when it didn't finish writing by `deadline`.
fn read_output(child: &mut Child, deadline: Instant) -> Result<Vec<u8>, ArchiveFailure> {
    let output = child.stdout.take().ok_or(ArchiveFailure::NotRead)?;
    let (done, finished) = mpsc::channel();
    thread::Builder::new()
        .name(READER_THREAD.to_owned())
        .spawn(move || {
            done.send(capped(output, MAX_OUTPUT)).ok();
        })
        .map_err(|_| ArchiveFailure::NotRead)?;
    match finished.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(Ok(Some(bytes))) => Ok(bytes),
        Ok(Ok(None)) => Err(ArchiveFailure::Broken),
        Ok(Err(_)) | Err(_) => Err(ArchiveFailure::NotRead),
    }
}

/// The helper's exit status, waiting for it to exit until `deadline`.
/// Its group is killed either way, then it is waited on.
fn end(mut child: Child, deadline: Instant) -> Option<ExitStatus> {
    let pid = Pid::from_child(&child);
    if let Ok(exited) = process::pidfd_open(pid, PidfdFlags::empty()) {
        let left = deadline.saturating_duration_since(Instant::now());
        if let Ok(left) = Timespec::try_from(left) {
            poll(&mut [PollFd::new(&exited, PollFlags::IN)], Some(&left)).ok();
        }
    }
    process::kill_process_group(pid, Signal::KILL).ok();
    child.wait().ok()
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt as _;

    use super::*;

    #[test]
    fn reads_up_to_the_cap_and_no_more() {
        assert_eq!(capped(&b"abcd"[..], 4).unwrap(), Some(b"abcd".to_vec()));
        assert_eq!(capped(&b"abcde"[..], 4).unwrap(), None);
        assert_eq!(capped(&b""[..], 0).unwrap(), Some(Vec::new()));
    }

    #[test]
    fn exit_codes_say_how_the_run_ended() {
        let ended = |code: i32| ended(Some(ExitStatus::from_raw(code << 8)));
        assert_eq!(ended(DONE), Ok(()));
        assert_eq!(ended(BROKEN), Err(ArchiveFailure::Broken));
        assert_eq!(ended(UNSUPPORTED), Err(ArchiveFailure::Unsupported));
        assert_eq!(ended(GONE), Err(ArchiveFailure::Gone));
        assert_eq!(ended(DENIED), Err(ArchiveFailure::Denied));
        assert_eq!(ended(1), Err(ArchiveFailure::NotRead));
        assert_eq!(super::ended(None), Err(ArchiveFailure::NotRead));
    }
}
