use std::env;
use std::io::{BufReader, Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use nova_files_api::{Fit, Picture};
use rustix::process::Pid;

use crate::{BYTES_PER_PIXEL, Running, SIZE_SEPARATOR, command, end, read_line_up_to};

const PLAYER_NAME: &str = "nova-files-player";
const PLAYER_THREAD: &str = "nova-files-player-reader";
const PLAYER_VARS: [&str; 5] = [
    "XDG_RUNTIME_DIR",
    "LD_LIBRARY_PATH",
    "PULSE_SERVER",
    "PIPEWIRE_REMOTE",
    "LANG",
];
const MAX_LINE: usize = 2048;
const FRAME: &str = "frame";
const MISSING: &str = "missing";
const AT: &str = "at";
const LENGTH: &str = "length";
const PAUSED: &str = "paused";
const TAG: &str = "tag";
const ENDED: &str = "ended";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Title,
    Artist,
    Album,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    Missing,
    Frame(Picture),
    At(Duration),
    Length(Duration),
    Paused(bool),
    Tag(Tag, String),
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Told {
    Play,
    Pause,
    Seek(Duration),
    Volume(u8),
    Room(Fit),
}

#[derive(Debug)]
pub struct Player {
    child: Option<Child>,
    group: Option<Pid>,
    running: Arc<Running>,
    input: ChildStdin,
}

#[must_use]
pub fn player_beside(helper: &Path) -> PathBuf {
    helper.with_file_name(PLAYER_NAME)
}

pub fn play(
    player: &Path,
    file: &Path,
    (fit, volume): (Fit, u8),
    running: &Arc<Running>,
    mut heard: impl FnMut(Heard) -> bool + Send + 'static,
) -> Option<Player> {
    let fit = fit.clamped();
    let mut command = command(player);
    command
        .stdin(Stdio::piped())
        .arg(file)
        .arg(fit.width.to_string())
        .arg(fit.height.to_string())
        .arg(volume.to_string());
    for var in PLAYER_VARS {
        if let Some(value) = env::var_os(var) {
            command.env(var, value);
        }
    }
    let mut child = command.spawn().ok()?;
    let group = i32::try_from(child.id()).ok().and_then(Pid::from_raw);
    running.start(group, || true);
    let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
        running.forget(group);
        end(&mut child);
        return None;
    };
    let reading = thread::Builder::new()
        .name(PLAYER_THREAD.to_owned())
        .spawn(move || {
            let mut output = BufReader::new(output);
            while let Some(said) = read_heard(&mut output) {
                if !heard(said) {
                    return;
                }
            }
        });
    let player = Player {
        child: Some(child),
        group,
        running: Arc::clone(running),
        input,
    };
    reading.is_ok().then_some(player)
}

impl Player {
    pub fn tell(&mut self, told: Told) -> bool {
        let line = match told {
            Told::Play => "play".to_owned(),
            Told::Pause => "pause".to_owned(),
            Told::Seek(at) => format!("seek {}", at.as_millis()),
            Told::Volume(volume) => format!("volume {volume}"),
            Told::Room(fit) => {
                let fit = fit.clamped();
                format!("room {} {}", fit.width, fit.height)
            }
        };
        writeln!(self.input, "{line}")
            .and_then(|()| self.input.flush())
            .is_ok()
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.running.forget(self.group);
        if let Some(mut child) = self.child.take() {
            end(&mut child);
        }
    }
}

fn read_heard(output: &mut impl Read) -> Option<Heard> {
    let line = read_line_up_to(output, MAX_LINE)?;
    let (word, rest) = line.split_once(SIZE_SEPARATOR).unwrap_or((&line, ""));
    let millis = |rest: &str| rest.parse::<u64>().ok().map(Duration::from_millis);
    Some(match word {
        FRAME => Heard::Frame(read_frame(rest, output)?),
        MISSING => Heard::Missing,
        AT => Heard::At(millis(rest)?),
        LENGTH => Heard::Length(millis(rest)?),
        PAUSED => Heard::Paused(rest == "1"),
        ENDED => Heard::Ended,
        TAG => {
            let (tag, text) = rest.split_once(SIZE_SEPARATOR).unwrap_or((rest, ""));
            let tag = match tag {
                "title" => Tag::Title,
                "artist" => Tag::Artist,
                "album" => Tag::Album,
                _ => return None,
            };
            Heard::Tag(tag, text.to_owned())
        }
        _ => return None,
    })
}

fn read_frame(words: &str, output: &mut impl Read) -> Option<Picture> {
    let (width, height) = words.split_once(SIZE_SEPARATOR)?;
    let side = |word: &str| {
        word.parse::<u32>()
            .ok()
            .filter(|side| (1..=nova_files_api::MAX_FIT_SIDE).contains(side))
    };
    let (width, height) = (side(width)?, side(height)?);
    let length = u64::from(width) * u64::from(height) * BYTES_PER_PIXEL;
    let mut pixels = vec![0; usize::try_from(length).ok()?];
    output.read_exact(&mut pixels).ok()?;
    Some(Picture {
        width,
        height,
        pixels: pixels.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_player_says_is_read_line_by_line() {
        let mut said =
            b"at 1500\nlength 3023\npaused 1\ntag artist Two Words\nended\nframe 2 1\n".to_vec();
        said.extend([5; 8]);
        said.extend(b"missing\n");
        let mut output = said.as_slice();
        let mut heard = Vec::new();
        while let Some(one) = read_heard(&mut output) {
            heard.push(one);
        }
        assert_eq!(heard.len(), 7);
        assert_eq!(heard[0], Heard::At(Duration::from_millis(1500)));
        assert_eq!(heard[2], Heard::Paused(true));
        assert_eq!(heard[3], Heard::Tag(Tag::Artist, "Two Words".to_owned()));
        let Heard::Frame(frame) = &heard[5] else {
            panic!("expected a frame, got {:?}", heard[5]);
        };
        assert_eq!((frame.width, frame.height), (2, 1));
        assert_eq!(heard[6], Heard::Missing);
    }

    #[test]
    fn a_frame_cut_short_or_too_big_ends_the_reading() {
        assert_eq!(read_heard(&mut b"frame 2 1\n1234".as_slice()), None);
        assert_eq!(read_heard(&mut b"frame 90000 1\n".as_slice()), None);
        assert_eq!(read_heard(&mut b"shout 1\n".as_slice()), None);
    }
}
