mod mpv;
mod sandbox;
mod words;

use std::ffi::{CString, OsString};
use std::io::{self, BufRead as _, Write as _};
use std::os::unix::ffi::OsStringExt as _;
use std::path::PathBuf;
use std::process::{self, ExitCode};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use mpv::{Event, FORMAT_DOUBLE, FORMAT_FLAG, FORMAT_STRING, Mpv, Value};
use words::{Box as Room, Heard, Told};

const USAGE: &str = "usage: nova-files-player <file> <width> <height> <volume>";
const READER_THREAD: &str = "nova-files-player-input";
const FRAME_WAIT: Duration = Duration::from_millis(50);
const BYTES_PER_PIXEL: usize = 4;
const ALPHA: usize = 3;
const OPAQUE: u8 = 255;
const OPTIONS: [(&str, &str); 10] = [
    ("vo", "libmpv"),
    ("config", "no"),
    ("terminal", "no"),
    ("keep-open", "yes"),
    ("input-default-bindings", "no"),
    ("osc", "no"),
    ("ytdl", "no"),
    ("load-scripts", "no"),
    ("hwdec", "auto-copy-safe"),
    ("audio-display", "embedded-first"),
];
const PROPERTIES: [(&std::ffi::CStr, i32); 7] = [
    (c"eof-reached", FORMAT_FLAG),
    (c"time-pos", FORMAT_DOUBLE),
    (c"duration", FORMAT_DOUBLE),
    (c"pause", FORMAT_FLAG),
    (c"metadata/by-key/title", FORMAT_STRING),
    (c"metadata/by-key/artist", FORMAT_STRING),
    (c"metadata/by-key/album", FORMAT_STRING),
];

struct Request {
    file: PathBuf,
    room: Room,
    volume: u8,
}

fn main() -> ExitCode {
    let Some(request) = request(std::env::args_os().skip(1)) else {
        writeln!(io::stderr(), "{USAGE}").ok();
        return ExitCode::FAILURE;
    };
    if let Err(error) = sandbox::lock_in() {
        writeln!(io::stderr(), "could not start the sandbox: {error}").ok();
        return ExitCode::FAILURE;
    }
    let Some(mut mpv) = Mpv::load() else {
        return say(&Heard::Missing);
    };
    let volume = request.volume.to_string();
    let set = OPTIONS
        .iter()
        .all(|(name, value)| mpv.set_option(name, value))
        && mpv.set_option("volume", &volume);
    if !set || !mpv.start() {
        return ExitCode::FAILURE;
    }
    for (name, format) in PROPERTIES {
        mpv.observe(name, format);
    }
    let Ok(file) = CString::new(request.file.into_os_string().into_vec()) else {
        return ExitCode::FAILURE;
    };
    if !mpv.load_file(&file) {
        return ExitCode::FAILURE;
    }
    play(&mpv, request.room)
}

fn request(args: impl IntoIterator<Item = OsString>) -> Option<Request> {
    let mut args = args.into_iter();
    let file = PathBuf::from(args.next()?);
    let mut number = || args.next()?.into_string().ok()?.parse::<u32>().ok();
    let room = Room::new(number()?, number()?)?;
    let volume = u8::try_from(number()?)
        .ok()
        .filter(|volume| *volume <= words::MAX_VOLUME)?;
    (file.is_absolute() && args.next().is_none()).then_some(Request { file, room, volume })
}

fn play(mpv: &Mpv, room: Room) -> ExitCode {
    let room = Arc::new(AtomicU64::new(room.packed()));
    let commander = mpv.commander();
    let listening_room = Arc::clone(&room);
    let listening = thread::Builder::new()
        .name(READER_THREAD.to_owned())
        .spawn(move || {
            for line in io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                match Told::parse(&line) {
                    Some(Told::Room(size)) => listening_room.store(size.packed(), Ordering::SeqCst),
                    Some(told) => {
                        if let Some(command) = told.command() {
                            commander.run(&command);
                        }
                    }
                    None => {}
                }
            }
            process::exit(0);
        });
    if listening.is_err() {
        return ExitCode::FAILURE;
    }
    let mut pixels = Vec::new();
    let mut out = io::stdout().lock();
    let mut drawn = None;
    loop {
        let woken = mpv.wait_for_frame(FRAME_WAIT);
        while let Some(event) = mpv.next_event() {
            let heard = match event {
                Event::Shutdown => return ExitCode::SUCCESS,
                Event::Ended => Some(Heard::Ended),
                Event::Resized => None,
                Event::Property { name, value } => heard(&name, value),
            };
            if heard.is_some_and(|heard| heard.write(&mut out).is_err()) {
                return ExitCode::FAILURE;
            }
        }
        let wanted = Room::unpacked(room.load(Ordering::SeqCst));
        let fresh = woken && mpv.has_new_frame();
        if !fresh && (drawn.is_none() || drawn == Some(wanted)) {
            continue;
        }
        let Some((width, height)) = frame_size(mpv, wanted) else {
            continue;
        };
        let length = (width as usize) * (height as usize) * BYTES_PER_PIXEL;
        pixels.resize(length, 0);
        let (Ok(wide), Ok(high)) = (i32::try_from(width), i32::try_from(height)) else {
            continue;
        };
        if !mpv.draw(wide, high, &mut pixels) {
            continue;
        }
        for pixel in pixels.as_chunks_mut::<BYTES_PER_PIXEL>().0 {
            pixel[ALPHA] = OPAQUE;
        }
        let sent = writeln!(out, "{} {width} {height}", words::FRAME)
            .and_then(|()| out.write_all(&pixels))
            .and_then(|()| out.flush());
        if sent.is_err() {
            return ExitCode::FAILURE;
        }
        drawn = Some(wanted);
    }
}

fn frame_size(mpv: &Mpv, room: Room) -> Option<(u32, u32)> {
    let width = mpv.number(c"dwidth")?;
    let height = mpv.number(c"dheight")?;
    room.fitted(width, height)
}

fn heard(name: &str, value: Value) -> Option<Heard> {
    match (name, value) {
        ("time-pos", Value::Number(seconds)) => {
            Some(Heard::At(Duration::try_from_secs_f64(seconds).ok()?))
        }
        ("duration", Value::Number(seconds)) => {
            Some(Heard::Length(Duration::try_from_secs_f64(seconds).ok()?))
        }
        ("pause", Value::Flag(paused)) => Some(Heard::Paused(paused)),
        ("eof-reached", Value::Flag(true)) => Some(Heard::Ended),
        (tag, Value::Text(text)) => {
            let tag = tag.rsplit('/').next()?;
            Some(Heard::Tag(words::Tag::named(tag)?, text))
        }
        _ => None,
    }
}

fn say(heard: &Heard) -> ExitCode {
    let mut out = io::stdout().lock();
    if heard.write(&mut out).and_then(|()| out.flush()).is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
