//! A picture for the quick look: one file decoded, turned upright and
//! scaled down to fit a box, sent on standard output.
//!
//! `nova-files-thumbnailer --preview <file> <width> <height>`
//!
//! Before it reads the file the helper locks itself in to reading alone
//! (see `sandbox`). It decodes JPEG, PNG and WebP itself (see `render`),
//! within the same decode limit as thumbnails; anything else, or a file
//! that won't decode, exits with failure and prints nothing. The answer is
//! a line `<width> <height>`, then that many RGBA pixels, row by row. A
//! picture smaller than the box keeps its size.

use std::ffi::OsString;
use std::io::{self, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;

use crate::render;
use crate::sandbox;

/// The first argument that asks for a preview rather than a batch. The
/// core passes the same word.
pub const FLAG: &str = "--preview";
pub const USAGE: &str = "usage: nova-files-thumbnailer --preview <file> <width> <height>";

/// What the core asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub file: PathBuf,
    pub width: u32,
    pub height: u32,
}

impl Request {
    /// The arguments after [`FLAG`]; `None` when they don't form a request.
    pub fn from_args(args: impl IntoIterator<Item = OsString>) -> Option<Self> {
        let mut args = args.into_iter();
        let file = PathBuf::from(args.next()?);
        let side = |arg: Option<OsString>| {
            arg?.into_string()
                .ok()?
                .parse::<u32>()
                .ok()
                .filter(|side| *side > 0)
        };
        let width = side(args.next())?;
        let height = side(args.next())?;
        (file.is_absolute() && args.next().is_none()).then_some(Self {
            file,
            width,
            height,
        })
    }
}

/// Answers `request` on standard output.
pub fn run(request: &Request) -> ExitCode {
    if let Err(error) = sandbox::lock_in_read_only() {
        writeln!(io::stderr(), "could not start the sandbox: {error}").ok();
        return ExitCode::FAILURE;
    }
    if !render::built_in(&request.file) {
        return ExitCode::FAILURE;
    }
    let Ok(image) = render::decode(&request.file) else {
        return ExitCode::FAILURE;
    };
    let fits = image.width() <= request.width && image.height() <= request.height;
    // Averaging areas: smooth when scaling far down, and quicker than a
    // filter (a 24 MP photo to 4K in 0.6 s against 0.75 s, decode included).
    let image = if fits {
        image
    } else {
        image.thumbnail(request.width, request.height)
    };
    let pixels = image.into_rgba8();
    let mut out = io::stdout().lock();
    let sent = writeln!(out, "{} {}", pixels.width(), pixels.height())
        .and_then(|()| out.write_all(pixels.as_raw()))
        .and_then(|()| out.flush());
    if sent.is_ok() {
        ExitCode::SUCCESS
    } else {
        // Nobody is reading the answer any more.
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn a_request_names_an_absolute_file_and_a_box() {
        assert_eq!(
            Request::from_args(args(&["/a/b.jpg", "800", "600"])),
            Some(Request {
                file: PathBuf::from("/a/b.jpg"),
                width: 800,
                height: 600
            })
        );
    }

    #[test]
    fn other_arguments_are_refused() {
        for bad in [
            &["b.jpg", "800", "600"][..],
            &["/b.jpg", "800"],
            &["/b.jpg", "0", "600"],
            &["/b.jpg", "800", "-1"],
            &["/b.jpg", "800", "600", "extra"],
            &[],
        ] {
            assert_eq!(Request::from_args(args(bad)), None, "{bad:?}");
        }
    }
}
