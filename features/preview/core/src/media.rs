use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use nova_files_api::{EventSink, Fit};
use nova_files_disk::ports::Mimes;
use nova_files_preview_api::{Control, Event, MAX_VOLUME, MediaTag, Played, PreviewId};
use nova_files_thumbnailer_launch::{self as launch, Heard, Player, Running, Tag, Told};

use crate::Current;

const VIDEO: &str = "video/";
const AUDIO: &str = "audio/";
const NOT_PLAYED: [&str; 7] = [
    "mpegurl", "scpls", "x-ms-asx", "x-ms-wax", "x-ms-wvx", "playlist", "midi",
];
const VIDEO_EXTENSIONS: [&str; 8] = ["mp4", "m4v", "mkv", "webm", "mov", "avi", "ogv", "wmv"];
const AUDIO_EXTENSIONS: [&str; 9] = [
    "mp3", "flac", "ogg", "oga", "opus", "m4a", "wav", "aac", "wma",
];

#[derive(Debug)]
pub struct Playing {
    running: Arc<Running>,
    player: Mutex<Option<(PreviewId, Player)>>,
    volume: AtomicU8,
}

impl Default for Playing {
    fn default() -> Self {
        Self {
            running: Arc::new(Running::new()),
            player: Mutex::new(None),
            volume: AtomicU8::new(MAX_VOLUME),
        }
    }
}

impl Playing {
    pub fn stop(&self) {
        self.running.stop();
        let player = self
            .player
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        drop(player);
    }
}

pub fn player_beside(helper: &Path) -> Option<PathBuf> {
    Some(launch::player_beside(helper)).filter(|player| player.exists())
}

pub fn kind(types: &dyn Mimes, name: &OsStr) -> Option<bool> {
    match types.of(name) {
        Some(mime) if NOT_PLAYED.iter().any(|word| mime.contains(word)) => None,
        Some(mime) if mime.starts_with(VIDEO) => Some(true),
        Some(mime) if mime.starts_with(AUDIO) => Some(false),
        Some(_) => None,
        None => {
            let extension = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
            let listed = |list: &[&str]| list.contains(&extension.as_str());
            if listed(&VIDEO_EXTENSIONS) {
                Some(true)
            } else {
                listed(&AUDIO_EXTENSIONS).then_some(false)
            }
        }
    }
}

pub fn start(
    events: &Arc<dyn EventSink>,
    current: &Arc<Current>,
    (id, file, fit): (PreviewId, &Path, Fit),
    player: &Path,
) {
    let playing = &current.playing;
    let volume = playing.volume.load(Ordering::SeqCst);
    let sink = Arc::clone(events);
    let watching = Arc::clone(current);
    let heard = move |heard: Heard| {
        watching.is(id)
            && sink
                .deliver(Event::Played {
                    id,
                    played: played(heard),
                })
                .is_ok()
    };
    let started = launch::play(player, file, (fit, volume), &playing.running, heard);
    let mut slot = playing
        .player
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    match started {
        Some(started) if current.is(id) => *slot = Some((id, started)),
        Some(_) => {}
        None if !current.is(id) => {}
        None => {
            drop(slot);
            let unplayable = Event::Played {
                id,
                played: Played::Unplayable,
            };
            events.deliver(unplayable).ok();
        }
    }
}

pub fn control(current: &Current, id: PreviewId, control: Control) {
    let playing = &current.playing;
    let told = match control {
        Control::Play => Told::Play,
        Control::Pause => Told::Pause,
        Control::Seek(at) => Told::Seek(at),
        Control::Volume(volume) => {
            let volume = volume.min(MAX_VOLUME);
            playing.volume.store(volume, Ordering::SeqCst);
            Told::Volume(volume)
        }
        Control::Fit(fit) => Told::Room(fit),
    };
    let mut slot = playing
        .player
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some((shown, player)) = slot.as_mut()
        && *shown == id
    {
        player.tell(told);
    }
}

fn played(heard: Heard) -> Played {
    match heard {
        Heard::Missing => Played::Unplayable,
        Heard::Frame(frame) => Played::Frame(frame),
        Heard::At(at) => Played::At(at),
        Heard::Length(length) => Played::Length(length),
        Heard::Paused(paused) => Played::Paused(paused),
        Heard::Ended => Played::Ended,
        Heard::Tag(tag, text) => Played::Tag(
            match tag {
                Tag::Title => MediaTag::Title,
                Tag::Artist => MediaTag::Artist,
                Tag::Album => MediaTag::Album,
            },
            text,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Named;

    impl nova_files_api::FileTypes for Named {
        fn of(&self, name: &OsStr) -> Option<&str> {
            match Path::new(name).extension()?.to_str()? {
                "m3u8" => Some("application/vnd.apple.mpegurl"),
                "pls" => Some("audio/x-scpls"),
                "mid" => Some("audio/midi"),
                "flac" => Some("audio/flac"),
                "mkv" => Some("video/x-matroska"),
                _ => None,
            }
        }

        fn is_a(&self, mime: &str, wanted: &str) -> bool {
            mime == wanted
        }
    }

    impl Mimes for Named {
        fn ancestry(&self, mime: &str) -> Vec<String> {
            vec![mime.to_owned()]
        }

        fn name(&self, _: &str) -> Option<String> {
            None
        }

        fn icon_names(&self, _: &str) -> Vec<String> {
            Vec::new()
        }

        fn icon_kind(&self, _: &str) -> nova_files_api::IconKind {
            nova_files_api::IconKind::Text
        }
    }

    #[test]
    fn playlists_and_midi_are_not_played() {
        let of = |name: &str| kind(&Named, OsStr::new(name));
        assert_eq!(of("film.mkv"), Some(true));
        assert_eq!(of("song.flac"), Some(false));
        assert_eq!(of("SONG.OPUS"), Some(false));
        assert_eq!(of("list.m3u8"), None);
        assert_eq!(of("radio.pls"), None);
        assert_eq!(of("tune.mid"), None);
        assert_eq!(of("notes.txt"), None);
    }
}
