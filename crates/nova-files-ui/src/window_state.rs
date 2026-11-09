//! The windows' state between runs: for now, the size a picker had when it
//! closed, so the next one opens at that size, apart from the main
//! window's.
//!
//! The core keeps the state as bytes it never reads; this module owns what
//! they say. The first line names the format and its version, then each
//! line is one window's entry, such as `picker 900x600`. A state with an
//! entry this version doesn't know, or anything else it can't read, reads
//! as no state, and the theme's sizes apply; a later version that adds
//! entries raises the version.

use std::sync::mpsc;
use std::time::Duration;

use gpui::{Pixels, Size, px, size};
use nova_files_api::{Command, Connector, Event};

use crate::theme;

/// The first line, naming the format and its version.
const HEADER: &str = "nova-files window-state 1";
/// The key of a picker's entry.
const PICKER: &str = "picker";
/// Between an entry's width and height.
const BY: char = 'x';
/// The smallest a window opens at, whatever was saved.
const MIN_WIDTH: Pixels = px(400.0);
const MIN_HEIGHT: Pixels = px(300.0);
/// How long a picker's opening waits for the state. The core runs in this
/// program, so it answers in well under a millisecond; this only bounds a
/// core that is shutting down.
const FETCH_WAIT: Duration = Duration::from_millis(100);

/// The windows' state, as saved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WindowState {
    /// A picker's size, in whole logical pixels.
    picker: Option<(f32, f32)>,
}

impl WindowState {
    /// The state `bytes` say, or none when they aren't one.
    pub fn parse(bytes: &[u8]) -> Self {
        let Ok(text) = str::from_utf8(bytes) else {
            return Self::default();
        };
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            return Self::default();
        }
        let mut state = Self::default();
        for line in lines.filter(|line| !line.is_empty()) {
            match line.split_once(' ') {
                Some((PICKER, value)) => state.picker = parse_size(value),
                _ => return Self::default(),
            }
        }
        state
    }

    /// The bytes that say this state.
    pub fn to_bytes(&self) -> Vec<u8> {
        let picker = self
            .picker
            .map(|(width, height)| format!("{PICKER} {width}{BY}{height}"));
        let lines = std::iter::once(HEADER.to_owned()).chain(picker);
        lines
            .map(|line| line + "\n")
            .collect::<String>()
            .into_bytes()
    }

    /// The size a picker opens at: the one saved, kept within the smallest
    /// size and the `display`, or the theme's.
    pub fn picker_size(&self, display: Option<Size<Pixels>>) -> Size<Pixels> {
        let Some((width, height)) = self.picker else {
            return size(theme::window::PICK_WIDTH, theme::window::PICK_HEIGHT);
        };
        let mut width = px(width).max(MIN_WIDTH);
        let mut height = px(height).max(MIN_HEIGHT);
        if let Some(display) = display {
            width = width.min(display.width);
            height = height.min(display.height);
        }
        size(width, height)
    }

    /// This state with a picker's size set to `picker`.
    #[must_use]
    pub fn with_picker(mut self, picker: Size<Pixels>) -> Self {
        let whole = |pixels: Pixels| f32::from(pixels).round();
        self.picker = Some((whole(picker.width), whole(picker.height)));
        self
    }
}

/// `900x600` as a width and a height, both finite and above zero.
fn parse_size(value: &str) -> Option<(f32, f32)> {
    let (width, height) = value.split_once(BY)?;
    let width: f32 = width.parse().ok()?;
    let height: f32 = height.parse().ok()?;
    let fits = |side: f32| side.is_finite() && side > 0.0;
    (fits(width) && fits(height)).then_some((width, height))
}

/// The state the core keeps, asked for over a connection of its own, or
/// none when the core doesn't answer in time.
pub fn fetch(connector: &Connector) -> WindowState {
    let (sink, events) = mpsc::channel();
    let Ok(connection) = connector.connect(sink) else {
        return WindowState::default();
    };
    if connection.send(Command::WindowState).is_err() {
        return WindowState::default();
    }
    match events.recv_timeout(FETCH_WAIT) {
        Ok(Event::WindowState(bytes)) => WindowState::parse(&bytes),
        _ => WindowState::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_reads_back_what_it_wrote() {
        let state = WindowState::parse(b"nova-files window-state 1\n\npicker 900x600\n");
        assert_eq!(state.picker, Some((900.0, 600.0)));
        let written = state.with_picker(size(px(1000.4), px(700.6))).to_bytes();
        assert_eq!(written, b"nova-files window-state 1\npicker 1000x701\n");
        assert_eq!(WindowState::parse(&written).picker, Some((1000.0, 701.0)));
        assert_eq!(
            WindowState::default().to_bytes(),
            b"nova-files window-state 1\n"
        );
    }

    #[test]
    fn anything_else_reads_as_no_state() {
        for bytes in [
            &b""[..],
            b"picker 900x600\n",
            b"nova-files window-state 2\npicker 900x600\n",
            b"nova-files window-state 1\npicker 900x600\nmain 1200x800\n",
            b"nova-files window-state 1\npicker\n",
            b"\xff\xfe",
        ] {
            assert_eq!(
                WindowState::parse(bytes),
                WindowState::default(),
                "{bytes:?}"
            );
        }
        for value in [
            "900", "900x", "x600", "0x600", "900x0", "-1x600", "infx600", "NaNx600", "ninexsix",
        ] {
            assert_eq!(parse_size(value), None, "{value}");
        }
    }

    #[test]
    fn a_picker_opens_at_its_saved_size_within_bounds_or_the_themes() {
        let theme = size(theme::window::PICK_WIDTH, theme::window::PICK_HEIGHT);
        assert_eq!(WindowState::default().picker_size(None), theme);
        let saved = |width, height| WindowState {
            picker: Some((width, height)),
        };
        assert_eq!(
            saved(1000.0, 700.0).picker_size(None),
            size(px(1000.0), px(700.0))
        );
        assert_eq!(
            saved(10.0, 10.0).picker_size(None),
            size(MIN_WIDTH, MIN_HEIGHT)
        );
        let display = size(px(1280.0), px(720.0));
        assert_eq!(saved(5000.0, 5000.0).picker_size(Some(display)), display);
        assert_eq!(
            saved(1000.0, 700.0).picker_size(Some(display)),
            size(px(1000.0), px(700.0))
        );
    }
}
