//! Runs the sandboxed helper on one batch and reads its answers, or on one
//! picture for the quick look and reads the picture.
//!
//! The helper gets only what it needs from the environment (where programs,
//! MIME data and thumbnailers are: [`PASSED_VARS`]) and runs in a process
//! group of its own; its output is read on a thread of its own. A batch
//! has [`PER_FILE`] for each of its files; past that the helper is killed.
//! Once it is done the rest of its group is killed too, so nothing a
//! thumbnailer left running outlives the batch. It is always waited on. What it made is read from the cache, not from its
//! answers, so a helper killed part-way still counts the files it saved.
//! A preview's picture is read from the helper's output: its size line
//! first, which must fit the box asked for, then exactly that many pixels
//! into a buffer of that size, and nothing after. A newer preview stops
//! the helper still decoding an older one (see [`Running`]).

use std::env;
use std::ffi::OsString;
use std::io::{self, Read};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, PoisonError, mpsc};
use std::thread;
use std::time::Duration;

use nova_files_api::{Fit, Picture};
use rustix::process::{self, Pid, Signal};

/// How long each file of a batch may take; the helper gives a system
/// thumbnailer less than this, so a batch of slow files still finishes.
const PER_FILE: Duration = Duration::from_secs(15);
/// Where the helper finds the system's thumbnailers and MIME data.
const PASSED_VARS: [&str; 4] = ["PATH", "HOME", "XDG_DATA_HOME", "XDG_DATA_DIRS"];
const READER_THREAD: &str = "nova-files-thumbnail-reader";
/// The first argument that asks the helper for a preview; the helper
/// checks for the same word.
const PREVIEW_FLAG: &str = "--preview";
/// A picture's pixels are RGBA.
const BYTES_PER_PIXEL: u64 = 4;
/// The size line: two numbers of at most ten digits, a space, a newline.
const MAX_SIZE_LINE: usize = 22;
const SIZE_LINE_END: u8 = b'\n';
const SIZE_SEPARATOR: char = ' ';

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
    let mut command = command(helper);
    command.arg(thumbnails);
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
    end(&mut child);
}

/// The preview helper one client has running, so that a newer request can
/// stop it.
#[derive(Debug, Default)]
pub struct Running(Mutex<Option<Pid>>);

impl Running {
    /// Nothing running yet.
    #[must_use]
    pub const fn new() -> Self {
        Self(Mutex::new(None))
    }

    /// Kills the helper running and what it started, if any. Its group id
    /// names it alone: the helper is taken out of here before it is
    /// waited on, under the same lock.
    pub fn stop(&self) {
        // Held through the kill, so the group can't be waited on and its
        // id reused before the signal lands.
        let running = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(group) = *running {
            process::kill_process_group(group, Signal::KILL).ok();
        }
        drop(running);
    }

    /// Counts `group` as running, unless it is no longer `wanted`: then
    /// it is killed at once. Asked under the lock [`Self::stop`] takes, so
    /// a request replaced just before can't keep its helper running.
    fn start(&self, group: Option<Pid>, wanted: impl Fn() -> bool) {
        let mut running = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if wanted() {
            *running = group;
        } else if let Some(group) = group {
            process::kill_process_group(group, Signal::KILL).ok();
        }
        drop(running);
    }

    fn finish(&self) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// `file` decoded by the helper, scaled down to fit `fit`; `None` when it
/// isn't a picture the helper decodes, it ran out of time, or it stopped
/// being `wanted` (whoever changes that then calls `running.stop()`).
pub fn picture(
    helper: &Path,
    file: &Path,
    fit: Fit,
    running: &Running,
    wanted: impl Fn() -> bool,
) -> Option<Picture> {
    let fit = fit.clamped();
    let mut command = command(helper);
    command
        .arg(PREVIEW_FLAG)
        .arg(file)
        .arg(fit.width.to_string())
        .arg(fit.height.to_string());
    let mut child = command.spawn().ok()?;
    running.start(
        i32::try_from(child.id()).ok().and_then(Pid::from_raw),
        wanted,
    );
    let output = child.stdout.take();
    let (done, finished) = mpsc::channel();
    // A reader that couldn't start drops `done`, which ends the wait at once.
    thread::Builder::new()
        .name(READER_THREAD.to_owned())
        .spawn(move || {
            let picture = output.and_then(|output| read_picture(output, fit));
            done.send(picture).ok();
        })
        .ok();
    let picture = finished.recv_timeout(PER_FILE).ok().flatten();
    running.finish();
    end(&mut child);
    picture
}

/// The helper with only the environment it needs, in a process group of
/// its own, its output piped.
fn command(helper: &Path) -> Command {
    let mut command = Command::new(helper);
    command
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
    command
}

/// Kills what is left of the helper's group and waits for the helper.
fn end(child: &mut Child) {
    // The group is the helper's own: it, if still running, and what it
    // started. Until the helper is waited on, its id can't name another
    // group.
    if let Some(group) = i32::try_from(child.id()).ok().and_then(Pid::from_raw) {
        process::kill_process_group(group, Signal::KILL).ok();
    }
    child.wait().ok();
}

/// The picture the helper writes to `output`: a size line no bigger than
/// `fit`, then exactly that many pixels.
fn read_picture(mut output: impl Read, fit: Fit) -> Option<Picture> {
    let mut line = Vec::with_capacity(MAX_SIZE_LINE);
    let mut byte = [0];
    while line.len() < MAX_SIZE_LINE {
        output.read_exact(&mut byte).ok()?;
        if byte[0] == SIZE_LINE_END {
            break;
        }
        line.push(byte[0]);
    }
    let (width, height) = std::str::from_utf8(&line)
        .ok()?
        .split_once(SIZE_SEPARATOR)?;
    let (width, height): (u32, u32) = (width.parse().ok()?, height.parse().ok()?);
    let fits = (1..=fit.width).contains(&width) && (1..=fit.height).contains(&height);
    if !fits {
        return None;
    }
    let length = u64::from(width) * u64::from(height) * BYTES_PER_PIXEL;
    let mut pixels = vec![0; usize::try_from(length).ok()?];
    output.read_exact(&mut pixels).ok()?;
    // Anything after the pixels means the answer isn't what it says.
    (output.read(&mut byte).ok()? == 0).then(|| Picture {
        width,
        height,
        pixels: pixels.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIT: Fit = Fit {
        width: 4,
        height: 3,
    };

    fn answer(line: &str, pixels: usize) -> Vec<u8> {
        let mut answer = line.as_bytes().to_vec();
        answer.extend(std::iter::repeat_n(7, pixels * 4));
        answer
    }

    fn parse(answer: &[u8], fit: Fit) -> Option<Picture> {
        read_picture(answer, fit)
    }

    #[test]
    fn a_size_line_and_its_pixels_make_a_picture() {
        let picture = parse(&answer("2 3\n", 6), FIT).unwrap();
        assert_eq!((picture.width, picture.height), (2, 3));
        assert_eq!(picture.pixels.len(), 24);
        assert!(picture.pixels.iter().all(|byte| *byte == 7));
    }

    #[test]
    fn answers_that_dont_agree_or_fit_are_dropped() {
        for (line, pixels) in [
            ("2 3\n", 5),
            ("2 3\n", 7),
            ("5 3\n", 15),
            ("4 4\n", 16),
            ("0 3\n", 0),
            ("2x3\n", 6),
            ("2 3", 6),
            ("", 0),
        ] {
            assert!(
                parse(&answer(line, pixels), FIT).is_none(),
                "{line:?} {pixels}"
            );
        }
        // The line must end within its longest length.
        let long = format!("{}2 3\n", "0".repeat(MAX_SIZE_LINE));
        assert!(parse(&answer(&long, 6), FIT).is_none());
    }

    #[test]
    fn stopping_with_nothing_running_does_nothing() {
        let running = Running::new();
        running.stop();
        running.finish();
        running.stop();
    }

    #[test]
    fn a_helper_no_longer_wanted_is_killed_as_it_starts() {
        use std::os::unix::process::{CommandExt as _, ExitStatusExt as _};
        let mut child = Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        let group = i32::try_from(child.id()).ok().and_then(Pid::from_raw);
        Running::new().start(group, || false);
        let status = child.wait().unwrap();
        assert_eq!(status.signal(), Some(Signal::KILL.as_raw()));
    }
}
