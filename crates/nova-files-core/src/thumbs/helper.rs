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
//! A preview's picture is read from the helper's output, at most as many
//! bytes as the box asked for holds, and kept only when its size line and
//! its pixels agree and fit the box.

use std::env;
use std::ffi::OsString;
use std::io::{self, Read as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
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

/// `file` decoded by the helper, scaled down to fit `fit`; `None` when it
/// isn't a picture the helper decodes, or it ran out of time.
pub fn picture(helper: &Path, file: &Path, fit: Fit) -> Option<Picture> {
    let fit = fit.clamped();
    let mut command = command(helper);
    command
        .arg(PREVIEW_FLAG)
        .arg(file)
        .arg(fit.width.to_string())
        .arg(fit.height.to_string());
    let mut child = command.spawn().ok()?;
    let output = child.stdout.take();
    let pixels = u64::from(fit.width) * u64::from(fit.height) * BYTES_PER_PIXEL;
    // One byte past the most a right answer holds tells a wrong one apart.
    let limit = pixels + u64::try_from(MAX_SIZE_LINE).unwrap_or(u64::MAX) + 1;
    let (done, finished) = mpsc::channel();
    thread::Builder::new()
        .name(READER_THREAD.to_owned())
        .spawn(move || {
            let mut answer = Vec::new();
            if let Some(output) = output {
                output.take(limit).read_to_end(&mut answer).ok();
            }
            done.send(answer).ok();
        })
        .ok();
    let answer = finished.recv_timeout(PER_FILE).ok();
    end(&mut child);
    parse(answer?, fit)
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

/// The picture in the helper's `answer`: a size line, then exactly that
/// many pixels, no bigger than `fit`.
fn parse(mut answer: Vec<u8>, fit: Fit) -> Option<Picture> {
    let line_end = answer
        .iter()
        .take(MAX_SIZE_LINE)
        .position(|byte| *byte == SIZE_LINE_END)?;
    let line = std::str::from_utf8(&answer[..line_end]).ok()?;
    let (width, height) = line.split_once(SIZE_SEPARATOR)?;
    let (width, height): (u32, u32) = (width.parse().ok()?, height.parse().ok()?);
    let fits = (1..=fit.width).contains(&width) && (1..=fit.height).contains(&height);
    let expected = u64::from(width) * u64::from(height) * BYTES_PER_PIXEL;
    answer.drain(..=line_end);
    (fits && u64::try_from(answer.len()).ok()? == expected).then(|| Picture {
        width,
        height,
        pixels: answer.into(),
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

    #[test]
    fn a_size_line_and_its_pixels_make_a_picture() {
        let picture = parse(answer("2 3\n", 6), FIT).unwrap();
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
                parse(answer(line, pixels), FIT).is_none(),
                "{line:?} {pixels}"
            );
        }
        // The line must end within its longest length.
        let long = format!("{}2 3\n", "0".repeat(MAX_SIZE_LINE));
        assert!(parse(answer(&long, 6), FIT).is_none());
    }
}
