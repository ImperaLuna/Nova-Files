use std::io::{self, Write};
use std::time::Duration;

pub const FRAME: &str = "frame";
pub const MAX_VOLUME: u8 = 100;
const MAX_SIDE: u32 = 8192;
const MAX_TAG_CHARS: usize = 512;
const HALF_WORD: u32 = 32;
const LOW_HALF: u64 = u32::MAX as u64;
const SEPARATOR: char = ' ';
const MISSING: &str = "missing";
const AT: &str = "at";
const LENGTH: &str = "length";
const PAUSED: &str = "paused";
const TAG: &str = "tag";
const ENDED: &str = "ended";
const PLAY: &str = "play";
const PAUSE: &str = "pause";
const SEEK: &str = "seek";
const VOLUME: &str = "volume";
const ROOM: &str = "room";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Box {
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Title,
    Artist,
    Album,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Heard {
    Missing,
    At(Duration),
    Length(Duration),
    Paused(bool),
    Tag(Tag, String),
    Ended,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Told {
    Play,
    Pause,
    Seek(Duration),
    Volume(u8),
    Room(Box),
}

impl Box {
    pub fn new(width: u32, height: u32) -> Option<Self> {
        let fits = |side: u32| (1..=MAX_SIDE).contains(&side);
        (fits(width) && fits(height)).then_some(Self { width, height })
    }

    pub fn packed(self) -> u64 {
        (u64::from(self.width) << HALF_WORD) | u64::from(self.height)
    }

    pub fn unpacked(packed: u64) -> Self {
        let width = u32::try_from(packed >> HALF_WORD).unwrap_or(1);
        let height = u32::try_from(packed & LOW_HALF).unwrap_or(1);
        Self { width, height }
    }

    pub fn fitted(self, width: f64, height: f64) -> Option<(u32, u32)> {
        if !(width >= 1.0 && height >= 1.0) {
            return None;
        }
        let scale = (f64::from(self.width) / width)
            .min(f64::from(self.height) / height)
            .min(1.0);
        let side = |side: f64| {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a side already within the box, at least 1"
            )]
            let side = (side * scale).round().max(1.0) as u32;
            side
        };
        Some((side(width).min(self.width), side(height).min(self.height)))
    }
}

impl Tag {
    pub fn named(name: &str) -> Option<Self> {
        [Self::Title, Self::Artist, Self::Album]
            .into_iter()
            .find(|tag| name.eq_ignore_ascii_case(tag.word()))
    }

    const fn word(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Artist => "artist",
            Self::Album => "album",
        }
    }
}

impl Heard {
    pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
        match self {
            Self::Missing => writeln!(out, "{MISSING}"),
            Self::At(at) => writeln!(out, "{AT} {}", at.as_millis()),
            Self::Length(length) => writeln!(out, "{LENGTH} {}", length.as_millis()),
            Self::Paused(paused) => writeln!(out, "{PAUSED} {}", u8::from(*paused)),
            Self::Tag(tag, text) => {
                let text: String = text
                    .chars()
                    .map(|char| if char.is_control() { SEPARATOR } else { char })
                    .take(MAX_TAG_CHARS)
                    .collect();
                writeln!(out, "{TAG} {} {text}", tag.word())
            }
            Self::Ended => writeln!(out, "{ENDED}"),
        }
    }
}

impl Told {
    pub fn parse(line: &str) -> Option<Self> {
        let mut words = line.split(SEPARATOR);
        let word = words.next()?;
        let mut number = || words.next()?.parse::<u64>().ok();
        let told = match word {
            PLAY => Self::Play,
            PAUSE => Self::Pause,
            SEEK => Self::Seek(Duration::from_millis(number()?)),
            VOLUME => Self::Volume(
                u8::try_from(number()?)
                    .ok()
                    .filter(|volume| *volume <= MAX_VOLUME)?,
            ),
            ROOM => {
                let width = u32::try_from(number()?).ok()?;
                let height = u32::try_from(number()?).ok()?;
                Self::Room(Box::new(width, height)?)
            }
            _ => return None,
        };
        words.next().is_none().then_some(told)
    }

    pub fn command(&self) -> Option<String> {
        Some(match self {
            Self::Play => "set pause no".to_owned(),
            Self::Pause => "set pause yes".to_owned(),
            Self::Seek(at) => format!("seek {:.3} absolute+exact", at.as_secs_f64()),
            Self::Volume(volume) => format!("set volume {volume}"),
            Self::Room(_) => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_fits_the_box_and_never_grows() {
        let room = Box::new(400, 300).unwrap();
        assert_eq!(room.fitted(1920.0, 1080.0), Some((400, 225)));
        assert_eq!(room.fitted(200.0, 100.0), Some((200, 100)));
        assert_eq!(room.fitted(0.0, 100.0), None);
        assert_eq!(room.fitted(f64::NAN, 100.0), None);
        assert_eq!(Box::unpacked(room.packed()), room);
        assert_eq!(Box::new(0, 10), None);
    }

    #[test]
    fn what_the_core_says_is_read_word_by_word() {
        assert_eq!(Told::parse("play"), Some(Told::Play));
        assert_eq!(
            Told::parse("seek 83500"),
            Some(Told::Seek(Duration::from_millis(83_500)))
        );
        assert_eq!(Told::parse("volume 40"), Some(Told::Volume(40)));
        assert_eq!(Told::parse("volume 400"), None);
        assert_eq!(
            Told::parse("room 640 360"),
            Some(Told::Room(Box::new(640, 360).unwrap()))
        );
        assert_eq!(Told::parse("play now"), None);
        assert_eq!(Told::parse("jump 3"), None);
        assert_eq!(
            Told::Seek(Duration::from_millis(83_500))
                .command()
                .as_deref(),
            Some("seek 83.500 absolute+exact")
        );
    }

    #[test]
    fn a_tag_stays_on_one_line() {
        let mut out = Vec::new();
        Heard::Tag(Tag::Title, "Two\nLines".to_owned())
            .write(&mut out)
            .unwrap();
        assert_eq!(out, b"tag title Two Lines\n");
        assert_eq!(Tag::named("ARTIST"), Some(Tag::Artist));
    }
}
