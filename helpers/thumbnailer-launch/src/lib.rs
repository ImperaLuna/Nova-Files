mod document;

use std::env;
use std::ffi::OsString;
use std::io::{BufRead as _, BufReader, Read};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, PoisonError, mpsc};
use std::thread;
use std::time::Duration;

use nova_files_api::{Fit, Picture, ThumbnailSize};
use rustix::process::{self, Pid, Signal};

const PER_FILE: Duration = Duration::from_secs(15);
const PASSED_VARS: [&str; 4] = ["PATH", "HOME", "XDG_DATA_HOME", "XDG_DATA_DIRS"];
const READER_THREAD: &str = "nova-files-thumbnail-reader";
const PREVIEW_FLAG: &str = "--preview";
const BYTES_PER_PIXEL: u64 = 4;
const MAX_SIZE_LINE: usize = 32;
const MAX_FRAMES: usize = 2000;
const MIN_MOVING_FRAMES: usize = 2;
const MAX_MOVING_BYTES: u64 = 256 * 1024 * 1024;
const LINE_END: u8 = b'\n';
const SIZE_SEPARATOR: char = ' ';

pub use document::{Document, Opened, PageSize, document};

#[derive(Debug, Clone)]
pub struct Job {
    pub source: PathBuf,
    pub uri: String,
    pub mtime: u64,
    pub name: String,
}

pub fn run(
    helper: &Path,
    thumbnails: &Path,
    size: ThumbnailSize,
    jobs: &[Job],
    mut finished: impl FnMut(usize) -> bool,
) {
    let mut command = command(helper);
    command.arg(thumbnails).arg(size.folder());
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
    let (answered, answers) = mpsc::channel();
    thread::Builder::new()
        .name(READER_THREAD.to_owned())
        .spawn(move || {
            let Some(output) = output else {
                return;
            };
            for line in BufReader::new(output).split(LINE_END) {
                if line.is_err() || answered.send(()).is_err() {
                    return;
                }
            }
        })
        .ok();
    for index in 0..jobs.len() {
        if answers.recv_timeout(PER_FILE).is_err() || !finished(index) {
            break;
        }
    }
    end(&mut child);
}

#[derive(Debug, Default)]
pub struct Running(Mutex<Option<Pid>>);

impl Running {
    #[must_use]
    pub const fn new() -> Self {
        Self(Mutex::new(None))
    }

    pub fn stop(&self) {
        let running = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(group) = *running {
            process::kill_process_group(group, Signal::KILL).ok();
        }
        drop(running);
    }

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

    fn forget(&self, group: Option<Pid>) {
        let mut running = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if *running == group {
            *running = None;
        }
        drop(running);
    }
}

pub fn picture(
    helper: &Path,
    file: &Path,
    fit: Fit,
    running: &Running,
    wanted: impl Fn() -> bool,
) -> Option<Vec<(Picture, Duration)>> {
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

fn end(child: &mut Child) {
    if let Some(group) = i32::try_from(child.id()).ok().and_then(Pid::from_raw) {
        process::kill_process_group(group, Signal::KILL).ok();
    }
    child.wait().ok();
}

fn read_picture(mut output: impl Read, fit: Fit) -> Option<Vec<(Picture, Duration)>> {
    let line = read_line(&mut output)?;
    let mut words = line.split(SIZE_SEPARATOR);
    let width: u32 = words.next()?.parse().ok()?;
    let height: u32 = words.next()?.parse().ok()?;
    let count: usize = match words.next() {
        Some(count) => count
            .parse()
            .ok()
            .filter(|count| (MIN_MOVING_FRAMES..=MAX_FRAMES).contains(count))?,
        None => 1,
    };
    let fits = (1..=fit.width).contains(&width) && (1..=fit.height).contains(&height);
    if words.next().is_some() || !fits {
        return None;
    }
    let frame_bytes = u64::from(width) * u64::from(height) * BYTES_PER_PIXEL;
    if frame_bytes.saturating_mul(u64::try_from(count).ok()?) > MAX_MOVING_BYTES {
        return None;
    }
    let length = usize::try_from(frame_bytes).ok()?;
    let mut frames = Vec::with_capacity(count.min(MAX_FRAMES));
    for _ in 0..count {
        let delay = if count == 1 {
            Duration::ZERO
        } else {
            Duration::from_millis(read_line(&mut output)?.parse().ok()?)
        };
        let mut pixels = vec![0; length];
        output.read_exact(&mut pixels).ok()?;
        let picture = Picture {
            width,
            height,
            pixels: pixels.into(),
        };
        frames.push((picture, delay));
    }
    let mut byte = [0];
    (output.read(&mut byte).ok()? == 0).then_some(frames)
}

fn read_line(output: &mut impl Read) -> Option<String> {
    let mut line = Vec::with_capacity(MAX_SIZE_LINE);
    let mut byte = [0];
    while line.len() < MAX_SIZE_LINE {
        output.read_exact(&mut byte).ok()?;
        if byte[0] == LINE_END {
            return String::from_utf8(line).ok();
        }
        line.push(byte[0]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_files_test_folder::TestFolder;

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
        let mut frames = read_picture(answer, fit)?;
        assert_eq!(frames.len(), 1);
        Some(frames.remove(0).0)
    }

    #[test]
    fn a_moving_answer_gives_each_frame_and_its_delay() {
        let mut answer = b"2 1 2\n".to_vec();
        answer.extend(b"70\n");
        answer.extend([1; 8]);
        answer.extend(b"40\n");
        answer.extend([2; 8]);
        let frames = read_picture(answer.as_slice(), FIT).unwrap();
        let delays: Vec<_> = frames.iter().map(|(_, delay)| delay.as_millis()).collect();
        assert_eq!(delays, vec![70, 40]);
        assert_eq!(&*frames[1].0.pixels, &[2; 8]);
        answer.truncate(answer.len() - 1);
        assert!(read_picture(answer.as_slice(), FIT).is_none());
        assert!(read_picture(b"2 1 1\n".as_slice(), FIT).is_none());
        assert!(read_picture(b"2 1 4000000000\n".as_slice(), FIT).is_none());
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
        let long = format!("{}2 3\n", "0".repeat(MAX_SIZE_LINE));
        assert!(parse(&answer(&long, 6), FIT).is_none());
        let longest = format!("{:0>width$}\n", "2 3", width = MAX_SIZE_LINE - 1);
        assert!(parse(&answer(&longest, 6), FIT).is_some());
        let over = format!("{:0>width$}\n", "2 3", width = MAX_SIZE_LINE);
        assert!(parse(&answer(&over, 6), FIT).is_none());
    }

    #[test]
    fn stopping_with_nothing_running_does_nothing() {
        let running = Running::new();
        running.stop();
        running.finish();
        running.stop();
    }

    fn sleeper() -> (Child, Option<Pid>) {
        use std::os::unix::process::CommandExt as _;
        let child = Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        let group = i32::try_from(child.id()).ok().and_then(Pid::from_raw);
        (child, group)
    }

    #[test]
    fn stopping_kills_the_helper_running() {
        use std::os::unix::process::ExitStatusExt as _;
        let (mut child, group) = sleeper();
        let running = Running::new();
        running.start(group, || true);
        running.stop();
        let status = child.wait().unwrap();
        assert_eq!(status.signal(), Some(Signal::KILL.as_raw()));
    }

    #[test]
    fn a_finished_helper_is_not_stopped() {
        use std::os::unix::process::ExitStatusExt as _;
        let (mut child, group) = sleeper();
        let running = Running::new();
        running.start(group, || true);
        running.finish();
        running.stop();
        process::kill_process_group(group.unwrap(), Signal::TERM).unwrap();
        let status = child.wait().unwrap();
        assert_eq!(status.signal(), Some(Signal::TERM.as_raw()));
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

    fn helper_that_answers_then_hangs(lines: usize) -> (TestFolder, PathBuf) {
        use std::os::unix::fs::PermissionsExt as _;
        let folder = TestFolder::new("launch");
        let helper = folder.join(format!("answers-{lines}"));
        let answers = "echo ok; ".repeat(lines);
        std::fs::write(
            &helper,
            format!("#!/bin/sh\n{answers}exec sleep infinity\n"),
        )
        .unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        (folder, helper)
    }

    fn job(name: &str) -> Job {
        Job {
            source: PathBuf::from(format!("/folder/{name}")),
            uri: format!("file:///folder/{name}"),
            mtime: 1,
            name: format!("{name}.png"),
        }
    }

    #[test]
    fn each_answer_is_reported_before_the_helper_exits() {
        let (_helper_folder, helper) = helper_that_answers_then_hangs(3);
        let jobs = [job("a"), job("b"), job("c")];
        let mut reported = Vec::new();
        run(
            &helper,
            Path::new("/t"),
            ThumbnailSize::Normal,
            &jobs,
            |index| {
                reported.push(index);
                index == 0
            },
        );
        assert_eq!(reported, vec![0, 1]);
    }
}
