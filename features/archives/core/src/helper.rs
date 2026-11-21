use std::ffi::OsStr;
use std::io::{self, Read, Write as _};
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::process::{self, Pid, PidfdFlags, Signal};

use super::ArchiveFailure;

const RUN_TIME: Duration = Duration::from_mins(10);
const STOP_CHECK: Duration = Duration::from_millis(100);
const MAX_OUTPUT: u64 = 67_108_864;
const READER_THREAD: &str = "nova-files-archive-reader";
const LIST: &str = "list";
const DONE: i32 = 0;
const BROKEN: i32 = 4;
const UNSUPPORTED: i32 = 5;
const GONE: i32 = 6;
const DENIED: i32 = 7;
const NEEDS_PASSWORD: i32 = 8;
const WRONG_PASSWORD: i32 = 9;
const NO_SPACE: i32 = 10;
const READ_ONLY: i32 = 11;
const NOT_WRITTEN: i32 = 12;

pub fn list(
    helper: &Path,
    lead: &[&OsStr],
    (archive, password): (&Path, Option<&str>),
    stopped: &dyn Fn() -> bool,
) -> Result<Vec<u8>, ArchiveFailure> {
    let deadline = Instant::now() + RUN_TIME;
    let mut args = lead.to_vec();
    args.extend([LIST.as_ref(), archive.as_os_str()]);
    let mut child = spawn(helper, &args, Stdio::piped())?;
    give(&mut child, password);
    let output = read_output(&mut child, deadline, stopped);
    if output == Err(ArchiveFailure::Cancelled) {
        end(child, Duration::ZERO);
        return output;
    }
    let status = end(child, deadline.saturating_duration_since(Instant::now()));
    ended(status)?;
    output
}

pub fn spawn(helper: &Path, args: &[&OsStr], input: Stdio) -> Result<Child, ArchiveFailure> {
    Command::new(helper)
        .args(args)
        .env_clear()
        .stdin(input)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|_| ArchiveFailure::NotRead)
}

pub fn give(child: &mut Child, password: Option<&str>) {
    if let (Some(mut input), Some(password)) = (child.stdin.take(), password) {
        input.write_all(password.as_bytes()).ok();
    }
}

pub fn ended(status: Option<ExitStatus>) -> Result<(), ArchiveFailure> {
    Err(match status.and_then(|status| status.code()) {
        Some(DONE) => return Ok(()),
        Some(BROKEN) => ArchiveFailure::Broken,
        Some(UNSUPPORTED) => ArchiveFailure::Unsupported,
        Some(GONE) => ArchiveFailure::Gone,
        Some(DENIED) => ArchiveFailure::Denied,
        Some(NEEDS_PASSWORD) => ArchiveFailure::NeedsPassword,
        Some(WRONG_PASSWORD) => ArchiveFailure::WrongPassword,
        Some(NO_SPACE) => ArchiveFailure::NoSpace,
        Some(READ_ONLY) => ArchiveFailure::ReadOnly,
        Some(NOT_WRITTEN) => ArchiveFailure::NotWritten,
        _ => ArchiveFailure::NotRead,
    })
}

fn capped(reader: impl Read, cap: u64) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    reader.take(cap.saturating_add(1)).read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= cap).then_some(bytes))
}

fn read_output(
    child: &mut Child,
    deadline: Instant,
    stopped: &dyn Fn() -> bool,
) -> Result<Vec<u8>, ArchiveFailure> {
    let output = child.stdout.take().ok_or(ArchiveFailure::NotRead)?;
    let (done, finished) = mpsc::channel();
    thread::Builder::new()
        .name(READER_THREAD.to_owned())
        .spawn(move || {
            done.send(capped(output, MAX_OUTPUT)).ok();
        })
        .map_err(|_| ArchiveFailure::NotRead)?;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match finished.recv_timeout(left.min(STOP_CHECK)) {
            Ok(Ok(Some(bytes))) => return Ok(bytes),
            Ok(Ok(None)) => return Err(ArchiveFailure::Broken),
            Err(RecvTimeoutError::Timeout) if stopped() => return Err(ArchiveFailure::Cancelled),
            Err(RecvTimeoutError::Timeout) if !left.is_zero() => {}
            Ok(Err(_)) | Err(_) => return Err(ArchiveFailure::NotRead),
        }
    }
}

pub fn end(mut child: Child, wait: Duration) -> Option<ExitStatus> {
    let pid = Pid::from_child(&child);
    if let Ok(exited) = process::pidfd_open(pid, PidfdFlags::empty())
        && let Ok(left) = Timespec::try_from(wait)
    {
        poll(&mut [PollFd::new(&exited, PollFlags::IN)], Some(&left)).ok();
    }
    process::kill_process_group(pid, Signal::KILL).ok();
    child.wait().ok()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;
    use std::os::unix::process::ExitStatusExt as _;

    use super::*;

    #[test]
    fn reads_up_to_the_cap_and_no_more() {
        assert_eq!(capped(&b"abcd"[..], 4).unwrap(), Some(b"abcd".to_vec()));
        assert_eq!(capped(&b"abcde"[..], 4).unwrap(), None);
        assert_eq!(capped(&b""[..], 0).unwrap(), Some(Vec::new()));
    }

    #[test]
    fn a_stopped_listing_ends_its_helper_at_once() {
        let root = std::env::temp_dir().join(format!("nova-list-stop-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let helper = root.join("helper");
        std::fs::write(&helper, "#!/bin/sh\nwhile :; do :; done\n").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        let started = Instant::now();
        let listed = list(&helper, &[], (Path::new("/a.zip"), None), &|| true);
        assert_eq!(listed, Err(ArchiveFailure::Cancelled));
        assert!(started.elapsed() < RUN_TIME / 60);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn exit_codes_say_how_the_run_ended() {
        let ended = |code: i32| ended(Some(ExitStatus::from_raw(code << 8)));
        assert_eq!(ended(DONE), Ok(()));
        assert_eq!(ended(BROKEN), Err(ArchiveFailure::Broken));
        assert_eq!(ended(UNSUPPORTED), Err(ArchiveFailure::Unsupported));
        assert_eq!(ended(GONE), Err(ArchiveFailure::Gone));
        assert_eq!(ended(DENIED), Err(ArchiveFailure::Denied));
        assert_eq!(ended(NEEDS_PASSWORD), Err(ArchiveFailure::NeedsPassword));
        assert_eq!(ended(WRONG_PASSWORD), Err(ArchiveFailure::WrongPassword));
        assert_eq!(ended(NO_SPACE), Err(ArchiveFailure::NoSpace));
        assert_eq!(ended(READ_ONLY), Err(ArchiveFailure::ReadOnly));
        assert_eq!(ended(NOT_WRITTEN), Err(ArchiveFailure::NotWritten));
        assert_eq!(ended(1), Err(ArchiveFailure::NotRead));
        assert_eq!(super::ended(None), Err(ArchiveFailure::NotRead));
    }
}
