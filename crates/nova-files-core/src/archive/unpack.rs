//! Runs the helper to unpack an archive, following it as it goes.
//!
//! The password goes on the helper's standard input, which is then
//! closed. The helper writes a line for each entry it unpacked; between
//! lines the caller is asked every [`TICK`] whether to go on. While the
//! caller holds it, the helper's group is stopped.

use std::io::Write as _;
use std::path::Path;
use std::process::{Child, ChildStdout, Stdio};
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::{self, Errno};
use rustix::process::{self, Pid, Signal};

use super::ArchiveFailure;
use super::helper::{end, ended, spawn};

/// How long the caller waits between asks.
pub const TICK: Duration = Duration::from_millis(100);
/// How long the helper has to exit once it has closed its output.
const EXIT_TIME: Duration = Duration::from_secs(10);
const EXTRACT: &str = "extract";
const DECIMAL: u32 = 10;
const READ_SIZE: usize = 4_096;

/// What the caller hears while an archive unpacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extracting {
    /// One more entry is unpacked; it held this many bytes.
    Entry(u64),
    /// Nothing new since the last ask.
    Waiting,
    /// The helper is stopped, as the caller asked; asked again until the
    /// caller lets it go on or stops it.
    Held,
}

/// What the caller wants of the unpacking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Go,
    /// Stop the helper where it is until the caller says otherwise.
    Hold,
    /// Kill the helper. What it unpacked stays.
    Stop,
}

pub fn extract(
    helper: &Path,
    archive: &Path,
    into: &Path,
    password: Option<&str>,
    mut tell: impl FnMut(Extracting) -> Flow,
) -> Result<(), ArchiveFailure> {
    let mut child = spawn(
        helper,
        &[EXTRACT.as_ref(), archive.as_os_str(), into.as_os_str()],
        Stdio::piped(),
    )?;
    if let (Some(mut input), Some(password)) = (child.stdin.take(), password) {
        // A helper that exits before reading it has said why.
        input.write_all(password.as_bytes()).ok();
    }
    let followed = follow(&mut child, &mut tell);
    let status = end(child, EXIT_TIME);
    followed?;
    ended(status)
}

/// Reads the helper's lines until it closes its output, asking `tell`.
fn follow(
    child: &mut Child,
    tell: &mut impl FnMut(Extracting) -> Flow,
) -> Result<(), ArchiveFailure> {
    let group = Pid::from_child(child);
    let output = child.stdout.take().ok_or(ArchiveFailure::NotRead)?;
    let mut line = Line::default();
    let mut buffer = [0; READ_SIZE];
    loop {
        if !ready(&output)? {
            steer(group, tell(Extracting::Waiting), tell)?;
            continue;
        }
        let read = match io::retry_on_intr(|| io::read(&output, &mut buffer)) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(_) => return Err(ArchiveFailure::NotRead),
        };
        for &byte in &buffer[..read] {
            if let Some(bytes) = line.take(byte)? {
                steer(group, tell(Extracting::Entry(bytes)), tell)?;
            }
        }
    }
}

/// One line of the helper's output: the number read so far.
#[derive(Default)]
struct Line(Option<u64>);

impl Line {
    /// Takes the next byte; the line's number once `byte` ends it.
    fn take(&mut self, byte: u8) -> Result<Option<u64>, ArchiveFailure> {
        if byte == b'\n' {
            return self.0.take().map(Some).ok_or(ArchiveFailure::Broken);
        }
        let digit = char::from(byte)
            .to_digit(DECIMAL)
            .ok_or(ArchiveFailure::Broken)?;
        let number = self
            .0
            .unwrap_or_default()
            .checked_mul(u64::from(DECIMAL))
            .and_then(|number| number.checked_add(u64::from(digit)))
            .ok_or(ArchiveFailure::Broken)?;
        self.0 = Some(number);
        Ok(None)
    }
}

/// Whether `output` has something to read within a [`TICK`].
fn ready(output: &ChildStdout) -> Result<bool, ArchiveFailure> {
    let tick = Timespec::try_from(TICK).map_err(|_| ArchiveFailure::NotRead)?;
    match poll(&mut [PollFd::new(output, PollFlags::IN)], Some(&tick)) {
        Ok(ready) => Ok(ready > 0),
        Err(Errno::INTR) => Ok(false),
        Err(_) => Err(ArchiveFailure::NotRead),
    }
}

/// Does what `flow` says to the helper's `group`.
fn steer(
    group: Pid,
    mut flow: Flow,
    tell: &mut impl FnMut(Extracting) -> Flow,
) -> Result<(), ArchiveFailure> {
    let mut held = false;
    loop {
        match flow {
            Flow::Go => {
                if held {
                    process::kill_process_group(group, Signal::CONT).ok();
                }
                return Ok(());
            }
            Flow::Stop => return Err(ArchiveFailure::Cancelled),
            Flow::Hold => {
                if !held {
                    process::kill_process_group(group, Signal::STOP).ok();
                    held = true;
                }
                flow = tell(Extracting::Held);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::CommandExt as _;
    use std::process::Command;
    use std::time::Instant;

    use super::*;

    /// How long a test waits for the kernel to stop or wake a process.
    const SETTLE: Duration = Duration::from_secs(5);

    fn line(bytes: &[u8]) -> Result<Vec<u64>, ArchiveFailure> {
        let mut line = Line::default();
        let mut numbers = Vec::new();
        for &byte in bytes {
            numbers.extend(line.take(byte)?);
        }
        Ok(numbers)
    }

    #[test]
    fn lines_are_decimal_numbers() {
        assert_eq!(line(b"12\n0\n"), Ok(vec![12, 0]));
        assert_eq!(line(b"18446744073709551615\n"), Ok(vec![u64::MAX]));
        assert_eq!(line(b"7"), Ok(Vec::new()));
        assert_eq!(line(b"18446744073709551616\n"), Err(ArchiveFailure::Broken));
        assert_eq!(line(b"\n"), Err(ArchiveFailure::Broken));
        assert_eq!(line(b"1a\n"), Err(ArchiveFailure::Broken));
    }

    fn sleeper() -> Child {
        Command::new("sleep")
            .arg("30")
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap()
    }

    #[test]
    fn output_is_ready_only_once_written() {
        let mut quiet = sleeper();
        assert_eq!(ready(quiet.stdout.as_ref().unwrap()), Ok(false));
        quiet.kill().ok();
        quiet.wait().ok();

        let mut talking = Command::new("echo").stdout(Stdio::piped()).spawn().unwrap();
        assert_eq!(ready(talking.stdout.as_ref().unwrap()), Ok(true));
        talking.wait().ok();
    }

    /// The state letter `/proc` gives `pid`, once it is `want` or
    /// [`SETTLE`] has passed.
    fn state(pid: Pid, want: char) -> char {
        let path = format!("/proc/{}/stat", pid.as_raw_nonzero());
        let until = Instant::now() + SETTLE;
        loop {
            let stat = std::fs::read_to_string(&path).unwrap();
            let after = stat.rsplit_once(") ").unwrap().1;
            let now = after.chars().next().unwrap();
            if now == want || Instant::now() > until {
                return now;
            }
            std::thread::yield_now();
        }
    }

    #[test]
    fn holding_stops_the_group_until_it_goes_on() {
        let mut child = sleeper();
        let group = Pid::from_child(&child);
        let mut held = Vec::new();
        let steered = steer(group, Flow::Hold, &mut |extracting| {
            held.push(state(group, 'T'));
            assert_eq!(extracting, Extracting::Held);
            if held.len() < 2 { Flow::Hold } else { Flow::Go }
        });
        assert_eq!(steered, Ok(()));
        assert_eq!(held, ['T', 'T']);
        assert_eq!(state(group, 'S'), 'S');
        assert_eq!(
            steer(group, Flow::Stop, &mut |_| Flow::Go),
            Err(ArchiveFailure::Cancelled)
        );
        child.kill().ok();
        child.wait().ok();
    }
}
