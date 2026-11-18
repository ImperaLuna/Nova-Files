use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use nova_files_api::Packing;
use std::process::{Child, ChildStdout, Stdio};
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::{self, Errno};
use rustix::process::{self, Pid, Signal};

use super::helper::{end, ended, give, spawn};
use super::{ArchiveFailure, Archiver, Recipe};

pub const TICK: Duration = Duration::from_millis(100);
const EXIT_TIME: Duration = Duration::from_secs(10);
const EXTRACT: &str = "extract";
const COMPRESS: &str = "compress";
const DECIMAL: u32 = 10;
const READ_SIZE: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extracting {
    Entry(u64),
    Waiting,
    Held,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Go,
    Hold,
    Stop,
}

pub fn extract(
    archiver: &Archiver,
    (archive, picked): (&Path, &[PathBuf]),
    into: &Path,
    password: Option<&str>,
    tell: impl FnMut(Extracting) -> Flow,
) -> Result<(), ArchiveFailure> {
    let mut args = archiver.lead();
    args.extend([EXTRACT.as_ref(), archive.as_os_str(), into.as_os_str()]);
    args.extend(picked.iter().map(|path| path.as_os_str()));
    run(&archiver.helper, &args, password, tell)
}

pub fn compress(
    archiver: &Archiver,
    sources: &[PathBuf],
    archive: &Path,
    recipe: &Recipe<'_>,
    tell: impl FnMut(Extracting) -> Flow,
) -> Result<(), ArchiveFailure> {
    let level = recipe.level.to_string();
    let mut args = archiver.lead();
    args.extend([
        COMPRESS.as_ref(),
        packing_word(recipe.packing).as_ref(),
        level.as_ref(),
        archive.as_os_str(),
    ]);
    args.extend(sources.iter().map(|source| source.as_os_str()));
    run(&archiver.helper, &args, recipe.password, tell)
}

const fn packing_word(packing: Packing) -> &'static str {
    match packing {
        Packing::ZipDeflate => "zip-deflate",
        Packing::ZipZstd => "zip-zstd",
        Packing::TarZstd => "tar-zst",
        Packing::TarGzip => "tar-gz",
        Packing::TarXz => "tar-xz",
        Packing::SevenZip => "7z",
    }
}

fn run(
    helper: &Path,
    args: &[&OsStr],
    password: Option<&str>,
    mut tell: impl FnMut(Extracting) -> Flow,
) -> Result<(), ArchiveFailure> {
    let mut child = spawn(helper, args, Stdio::piped())?;
    give(&mut child, password);
    let followed = follow(&mut child, &mut tell);
    let wait = if followed.is_ok() {
        EXIT_TIME
    } else {
        Duration::ZERO
    };
    let status = end(child, wait);
    followed?;
    ended(status)
}

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

#[derive(Default)]
struct Line(Option<u64>);

impl Line {
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

fn ready(output: &ChildStdout) -> Result<bool, ArchiveFailure> {
    let tick = Timespec::try_from(TICK).map_err(|_| ArchiveFailure::NotRead)?;
    match poll(&mut [PollFd::new(output, PollFlags::IN)], Some(&tick)) {
        Ok(ready) => Ok(ready > 0),
        Err(Errno::INTR) => Ok(false),
        Err(_) => Err(ArchiveFailure::NotRead),
    }
}

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
